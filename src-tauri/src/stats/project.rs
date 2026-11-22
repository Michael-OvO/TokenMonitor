//! Per-project and per-session breakdown for Claude and Codex.
//!
//! A project is the git repository a session ran in (a worktree folds into its
//! main checkout), else the session's working directory. A session is one
//! conversation: subagent rows fold into the conversation that spawned them.

use crate::models::{ChartBucket, ChartSegment};
use crate::stats::change::conversation_key;
use crate::stats::subagent::AgentScope;
use crate::usage::parser::ParsedEntry;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Sessions listed per project; the rest still count toward the project total.
const MAX_SESSIONS_PER_PROJECT: usize = 20;

/// What a log file says about its session, shared by every row it produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SessionMeta {
    pub cwd: Option<String>,
    pub title: Option<String>,
}

/// Give every row a parser read from one session file the file's session meta.
pub fn attach_session_meta(
    entries: &mut [ParsedEntry],
    cwd: Option<String>,
    title: Option<String>,
) {
    if cwd.is_none() && title.is_none() {
        return;
    }
    let meta = Arc::new(SessionMeta { cwd, title });
    for entry in entries {
        entry.session_meta = Some(meta.clone());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUsage {
    pub key: String,
    pub title: Option<String>,
    pub cost: f64,
    pub tokens: u64,
    pub subagent_cost: f64,
    pub last_active: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectUsage {
    /// Repository root or working directory; None for rows with no directory.
    pub path: Option<String>,
    pub name: String,
    pub cost: f64,
    pub tokens: u64,
    pub session_count: u32,
    pub sessions: Vec<SessionUsage>,
}

#[derive(Default)]
struct SessionAcc {
    cwd: Option<String>,
    title: Option<String>,
    cost: f64,
    tokens: u64,
    subagent_cost: f64,
    last_active: Option<DateTime<Local>>,
}

/// `entries` must be live session rows (archive rows carry no session).
/// `codex_titles` maps a Codex thread id to its name in `session_index.jsonl`.
pub fn aggregate_project_stats(
    entries: &[ParsedEntry],
    codex_titles: &HashMap<String, String>,
) -> Option<Vec<ProjectUsage>> {
    let mut sessions: HashMap<&str, SessionAcc> = HashMap::new();
    for entry in entries {
        let acc = sessions
            .entry(conversation_key(&entry.session_key))
            .or_default();
        let cost = entry.cost_usd();
        acc.cost += cost;
        acc.tokens += entry.total_tokens();
        if entry.agent_scope == AgentScope::Subagent {
            acc.subagent_cost += cost;
        }
        acc.last_active = acc.last_active.max(Some(entry.timestamp));
        if let Some(meta) = &entry.session_meta {
            // The main conversation's directory and title win over a subagent's.
            let main = entry.agent_scope == AgentScope::Main;
            if meta.cwd.is_some() && (main || acc.cwd.is_none()) {
                acc.cwd.clone_from(&meta.cwd);
            }
            if meta.title.is_some() && (main || acc.title.is_none()) {
                acc.title.clone_from(&meta.title);
            }
        }
    }
    if sessions.is_empty() {
        return None;
    }

    let mut projects: HashMap<Option<PathBuf>, ProjectUsage> = HashMap::new();
    for (key, acc) in sessions {
        let root = acc.cwd.as_deref().map(project_root);
        let project = projects
            .entry(root.clone())
            .or_insert_with(|| ProjectUsage {
                path: root.as_ref().map(|r| r.to_string_lossy().into_owned()),
                name: root.as_deref().map(display_name).unwrap_or_default(),
                cost: 0.0,
                tokens: 0,
                session_count: 0,
                sessions: Vec::new(),
            });
        project.cost += acc.cost;
        project.tokens += acc.tokens;
        project.session_count += 1;
        let title = acc.title.or_else(|| {
            key.strip_prefix("codex:")
                .and_then(|id| codex_titles.get(id).cloned())
        });
        project.sessions.push(SessionUsage {
            key: key.to_string(),
            title,
            cost: acc.cost,
            tokens: acc.tokens,
            subagent_cost: acc.subagent_cost,
            last_active: acc.last_active.map(|t| t.to_rfc3339()).unwrap_or_default(),
        });
    }

    // Only "no project" (Cursor, Kimi, SSH rows): nothing to break down.
    if projects.keys().all(Option::is_none) {
        return None;
    }
    let mut projects: Vec<ProjectUsage> = projects.into_values().collect();
    for project in &mut projects {
        project.sessions.sort_by(|a, b| b.cost.total_cmp(&a.cost));
        project.sessions.truncate(MAX_SESSIONS_PER_PROJECT);
    }
    projects.sort_by(|a, b| b.cost.total_cmp(&a.cost));
    Some(projects)
}

/// Projects drawn by name in the chart; the rest share one "Other" segment.
const MAX_CHART_PROJECTS: usize = 7;

/// The chart's time buckets split by project instead of model. Rows without
/// a working directory (Cursor, Kimi, archived hours) are "No project".
/// Segment keys are `project:<root>`, `project:none` and `project:other`.
pub fn project_chart_buckets(entries: &[ParsedEntry], period: &str) -> Vec<ChartBucket> {
    use crate::usage::device_aggregation::{bucket_key_for_local, bucket_label_for_key};
    // (bucket, project) → (cost, tokens), plus each project's period cost.
    let mut cells: HashMap<(String, Option<PathBuf>), (f64, u64)> = HashMap::new();
    let mut totals: HashMap<Option<PathBuf>, f64> = HashMap::new();
    for entry in entries {
        let root = entry
            .session_meta
            .as_ref()
            .and_then(|m| m.cwd.as_deref())
            .map(project_root);
        let cost = entry.cost_usd();
        let cell = cells
            .entry((bucket_key_for_local(&entry.timestamp, period), root.clone()))
            .or_default();
        cell.0 += cost;
        cell.1 += entry.total_tokens();
        *totals.entry(root).or_default() += cost;
    }
    let mut ranked: Vec<_> = totals.into_iter().filter(|(r, _)| r.is_some()).collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let named: HashMap<PathBuf, usize> = ranked
        .into_iter()
        .take(MAX_CHART_PROJECTS)
        .enumerate()
        .filter_map(|(rank, (root, _))| Some((root?, rank)))
        .collect();

    // segment key → (name, rank, cost, tokens), per bucket
    type Segments = HashMap<String, (String, usize, f64, u64)>;
    let mut buckets: HashMap<String, Segments> = HashMap::new();
    for ((bucket, root), (cost, tokens)) in cells {
        let (key, name, rank) = match root {
            Some(r) => match named.get(&r) {
                Some(&rank) => (format!("project:{}", r.display()), display_name(&r), rank),
                None => (
                    "project:other".into(),
                    "Other projects".into(),
                    MAX_CHART_PROJECTS,
                ),
            },
            None => (
                "project:none".into(),
                "No project".into(),
                MAX_CHART_PROJECTS + 1,
            ),
        };
        let seg = buckets
            .entry(bucket)
            .or_default()
            .entry(key)
            .or_insert((name, rank, 0.0, 0));
        seg.2 += cost;
        seg.3 += tokens;
    }

    let mut out: Vec<ChartBucket> = buckets
        .into_iter()
        .map(|(sort_key, segs)| {
            let mut segs: Vec<_> = segs.into_iter().collect();
            segs.sort_by_key(|(_, (_, rank, _, _))| *rank);
            ChartBucket {
                label: bucket_label_for_key(&sort_key, period),
                total: segs.iter().map(|(_, s)| s.2).sum(),
                segments: segs
                    .into_iter()
                    .map(|(model_key, (model, _, cost, tokens))| ChartSegment {
                        model,
                        model_key,
                        cost,
                        tokens,
                        pricing_available: true,
                    })
                    .collect(),
                sort_key,
            }
        })
        .collect();
    out.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    out
}

fn display_name(root: &Path) -> String {
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned())
}

/// The repository root `cwd` belongs to, else `cwd` itself (also for paths
/// that only exist on a remote host). Reads `.git` and, for a worktree, its
/// `gitdir`/`commondir` pointers; never runs git.
// ponytail: cached for the process lifetime, so a repo created after the first
// lookup keeps its old grouping until restart.
fn project_root(cwd: &str) -> PathBuf {
    static CACHE: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(root) = cache.lock().ok().and_then(|c| c.get(cwd).cloned()) {
        return root;
    }
    let root = resolve_repo_root(Path::new(cwd)).unwrap_or_else(|| PathBuf::from(cwd));
    // One spelling per directory, so a worktree and its main checkout compare
    // equal. macOS rewrites `/var` to `/private/var`; Windows expands 8.3 names
    // (`RUNNER~1` → `runneradmin`). A missing directory keeps the recorded text.
    let root = std::fs::canonicalize(&root)
        .map(|p| strip_verbatim(&p))
        .unwrap_or(root);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(cwd.to_string(), root.clone());
    }
    root
}

fn resolve_repo_root(cwd: &Path) -> Option<PathBuf> {
    for dir in cwd.ancestors().take(64) {
        let dotgit = dir.join(".git");
        if dotgit.is_dir() {
            return Some(dir.to_path_buf());
        }
        if dotgit.is_file() {
            // A worktree: `.git` reads `gitdir: <main>/.git/worktrees/<name>`,
            // whose `commondir` points back at `<main>/.git`.
            let text = std::fs::read_to_string(&dotgit).ok()?;
            let gitdir = dir.join(text.trim().strip_prefix("gitdir:")?.trim());
            let common = std::fs::read_to_string(gitdir.join("commondir"))
                .map(|c| normalize_lexical(&gitdir.join(c.trim())))
                .unwrap_or(gitdir);
            // `..` is collapsed in memory. A failed canonicalize would leave
            // that `..` in place, and a path ending in `..` has no file name,
            // so the worktree would not fold into the main repo.
            return match common.file_name() {
                Some(name) if name == ".git" => common.parent().map(Path::to_path_buf),
                _ => Some(dir.to_path_buf()),
            };
        }
    }
    None
}

/// Collapse `.` and `..` without reading the filesystem.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `canonicalize` on Windows returns `\\?\C:\…`; show the plain form.
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

/// Thread names from Codex's `session_index.jsonl`; a later rename wins.
pub fn read_codex_titles(index: &Path) -> HashMap<String, String> {
    #[derive(Deserialize)]
    struct Row {
        id: String,
        thread_name: String,
    }
    let Ok(text) = std::fs::read_to_string(index) else {
        return HashMap::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Row>(line).ok())
        .map(|row| (row.id, row.thread_name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(session_key: &str, scope: AgentScope, meta: &Arc<SessionMeta>) -> ParsedEntry {
        ParsedEntry {
            timestamp: Local::now(),
            model: "claude-sonnet-4-5".into(),
            input_tokens: 1000,
            output_tokens: 100,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: 0,
            web_search_requests: 0,
            billed_cost: None,
            unique_hash: None,
            session_key: session_key.into(),
            agent_scope: scope,
            session_meta: Some(meta.clone()),
        }
    }

    fn meta(cwd: &Path, title: Option<&str>) -> Arc<SessionMeta> {
        Arc::new(SessionMeta {
            cwd: Some(cwd.to_string_lossy().into_owned()),
            title: title.map(str::to_string),
        })
    }

    #[test]
    fn groups_worktrees_and_subagents_into_project_and_session() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        let wt = dir.path().join("wt");
        std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        std::fs::create_dir_all(main.join("sub")).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(main.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
        let gitdir = main.join(".git/worktrees/wt");
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();

        let a = meta(&main.join("sub"), Some("Fix bug"));
        let a_sub = meta(&main.join("sub"), None);
        let b = meta(&wt, None);
        let entries = vec![
            entry("claude:a:main", AgentScope::Main, &a),
            entry("claude:a:subagent:x", AgentScope::Subagent, &a_sub),
            entry("codex:b", AgentScope::Main, &b),
        ];
        let titles = HashMap::from([("b".to_string(), "Codex task".to_string())]);

        let projects = aggregate_project_stats(&entries, &titles).unwrap();
        assert_eq!(projects.len(), 1, "worktree folds into its main repo");
        let p = &projects[0];
        assert_eq!(p.name, "repo");
        assert_eq!(p.session_count, 2, "subagent folds into its conversation");
        let a = p.sessions.iter().find(|s| s.key == "claude:a").unwrap();
        assert_eq!(a.title.as_deref(), Some("Fix bug"));
        assert!(a.subagent_cost > 0.0 && a.subagent_cost < a.cost);
        let b = p.sessions.iter().find(|s| s.key == "codex:b").unwrap();
        assert_eq!(b.title.as_deref(), Some("Codex task"));
    }

    /// Another spelling of the main checkout (symlink, or a Windows junction)
    /// and a worktree whose `gitdir` is the canonical path are one project.
    #[test]
    fn worktree_folds_into_main_repo_seen_through_another_path() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        let via = dir.path().join("via");
        let wt = dir.path().join("wt");
        std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        std::fs::create_dir_all(main.join("sub")).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(main.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
        let gitdir =
            strip_verbatim(&std::fs::canonicalize(main.join(".git/worktrees/wt")).unwrap());
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        link_dir(&main, &via);

        let entries = vec![
            entry(
                "claude:direct:main",
                AgentScope::Main,
                &meta(&main.join("sub"), None),
            ),
            entry(
                "claude:via:main",
                AgentScope::Main,
                &meta(&via.join("sub"), None),
            ),
            entry("codex:wt", AgentScope::Main, &meta(&wt, None)),
        ];
        let projects = aggregate_project_stats(&entries, &HashMap::new()).unwrap();
        assert_eq!(
            projects.len(),
            1,
            "symlink and worktree share the main repo"
        );
        assert_eq!(projects[0].name, "repo");
        assert_eq!(projects[0].session_count, 3);
    }

    fn link_dir(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        {
            // A directory symlink needs admin; a junction does not, and
            // `canonicalize` still follows it to the real checkout.
            let output = std::process::Command::new("cmd")
                .arg("/C")
                .arg("mklink")
                .arg("/J")
                .arg(link)
                .arg(target)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "create junction: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn missing_directory_falls_back_to_cwd_and_no_meta_to_unassigned() {
        let remote = meta(Path::new("/nonexistent/host/proj"), None);
        let mut bare = entry("ssh:box", AgentScope::Main, &remote);
        bare.session_meta = None;
        let entries = vec![entry("claude:r:main", AgentScope::Main, &remote), bare];
        let projects = aggregate_project_stats(&entries, &HashMap::new()).unwrap();
        assert!(projects.iter().any(|p| p.name == "proj"));
        assert!(projects.iter().any(|p| p.path.is_none()));
    }

    #[test]
    fn chart_buckets_split_each_day_by_project() {
        let a = meta(Path::new("/nonexistent/alpha"), None);
        let b = meta(Path::new("/nonexistent/beta"), None);
        let mut archived = entry("archive:local", AgentScope::Main, &a);
        archived.session_meta = None;
        let entries = vec![
            entry("claude:1:main", AgentScope::Main, &a),
            entry("claude:1:main", AgentScope::Main, &a),
            entry("codex:2", AgentScope::Main, &b),
            archived,
        ];
        let buckets = project_chart_buckets(&entries, "month");
        assert_eq!(buckets.len(), 1, "all rows are today");
        let segs: Vec<_> = buckets[0]
            .segments
            .iter()
            .map(|s| s.model.as_str())
            .collect();
        assert_eq!(
            segs,
            ["alpha", "beta", "No project"],
            "costliest first, no project last"
        );
        assert_eq!(buckets[0].segments[0].tokens, 2 * 1100);
        assert_eq!(
            buckets[0].segments[0].model_key,
            "project:/nonexistent/alpha"
        );
        let sum: f64 = buckets[0].segments.iter().map(|s| s.cost).sum();
        assert!((buckets[0].total - sum).abs() < 1e-9);
    }

    #[test]
    fn reads_latest_codex_thread_name() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("session_index.jsonl");
        std::fs::write(
            &index,
            "{\"id\":\"t1\",\"thread_name\":\"Old\",\"updated_at\":\"x\"}\nnot json\n{\"id\":\"t1\",\"thread_name\":\"New\"}\n",
        )
        .unwrap();
        assert_eq!(read_codex_titles(&index)["t1"], "New");
    }
}
