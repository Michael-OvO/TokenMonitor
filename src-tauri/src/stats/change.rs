// src-tauri/src/change_stats.rs

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::usage::parser::ParsedEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileCategory {
    Code,
    Docs,
    Config,
    Other,
}

pub fn classify_file(path: &str) -> FileCategory {
    // Logs can originate on another OS, so use both separators regardless of
    // the host. These are file types, not an inference about a session's intent.
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let ext = name
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, ext)| ext)
        .unwrap_or("");

    // Structured test data isn't project configuration even when it borrows a
    // manifest's filename. Do not apply this override to source or documents.
    if matches!(
        ext,
        "json" | "json5" | "jsonc" | "jsonl" | "xml" | "yaml" | "yml"
    ) && path.split(['/', '\\']).any(|part| {
        matches!(
            part.to_ascii_lowercase().as_str(),
            "fixtures" | "__fixtures__" | "testdata"
        )
    }) {
        return FileCategory::Other;
    }

    // An explicit document suffix wins over names like Dockerfile or .env.
    // Plain .txt is handled later because requirements/CMake use it too.
    if matches!(
        ext,
        "md" | "mdx" | "rst" | "adoc" | "asciidoc" | "tex" | "bib" | "org" | "rmd" | "qmd" | "typ"
    ) {
        return FileCategory::Docs;
    }

    if matches!(ext, "json" | "json5" | "jsonc" | "xml")
        && path.split(['/', '\\']).any(|part| {
            matches!(
                part.to_ascii_lowercase().as_str(),
                ".vscode" | ".idea" | ".config" | "config" | "configs"
            )
        })
    {
        return FileCategory::Config;
    }

    // Recognizable manifests, build recipes and tool settings take precedence
    // over syntax: requirements.txt is configuration and vite.config.ts is too.
    if matches!(
        name.as_str(),
        ".gitignore"
            | ".gitattributes"
            | ".gitmodules"
            | ".dockerignore"
            | ".editorconfig"
            | ".npmrc"
            | ".yarnrc"
            | ".prettierignore"
            | ".prettierrc"
            | ".eslintrc"
            | ".babelrc"
            | ".browserslistrc"
            | ".nvmrc"
            | ".python-version"
            | ".tool-versions"
            | ".env"
            | "dockerfile"
            | "containerfile"
            | "makefile"
            | "gnumakefile"
            | "justfile"
            | "cmakelists.txt"
            | "requirements.txt"
            | "pipfile"
            | "gemfile"
            | "procfile"
            | "brewfile"
            | "package.json"
            | "package-lock.json"
            | "npm-shrinkwrap.json"
            | "composer.json"
            | "deno.json"
            | "deno.jsonc"
            | "manifest.json"
            | "config.json"
            | "settings.json"
            | "tsconfig.json"
            | "jsconfig.json"
            | "pom.xml"
            | "nuget.config"
    ) || name.starts_with(".env.")
        || name.starts_with("dockerfile.")
        || name.starts_with("containerfile.")
        || name.starts_with(".eslintrc.")
        || name.starts_with(".prettierrc.")
        || name.starts_with(".babelrc.")
        || (name.starts_with("requirements-") && ext == "txt")
        || ((name.starts_with("tsconfig.") || name.starts_with("jsconfig."))
            && matches!(ext, "json" | "jsonc"))
        || ((name.contains(".config.") || name.contains(".conf."))
            && matches!(
                ext,
                "js" | "cjs" | "mjs" | "ts" | "cts" | "mts" | "json" | "json5" | "jsonc"
            ))
    {
        return FileCategory::Config;
    }

    if matches!(
        name.as_str(),
        "readme"
            | "license"
            | "licence"
            | "copying"
            | "notice"
            | "changelog"
            | "changes"
            | "authors"
            | "contributing"
    ) {
        return FileCategory::Docs;
    }

    match ext {
        // Code
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "go" | "java" | "kt"
        | "scala" | "swift" | "c" | "cc" | "cpp" | "h" | "hpp" | "cs" | "rb" | "php" | "sh"
        | "bash" | "zsh" | "sql" | "html" | "css" | "scss" | "sass" | "svelte" | "vue" | "mts"
        | "cts" | "pyi" | "pyw" | "ipynb" | "r" | "jl" | "ps1" | "psm1" | "lua" | "dart"
        | "fish" | "ex" | "exs" | "erl" | "hs" | "clj" | "zig" | "pl" => FileCategory::Code,

        // Docs
        "txt" => FileCategory::Docs,

        // Config
        "yaml" | "yml" | "toml" | "ini" | "env" | "cfg" | "conf" | "config" | "properties"
        | "lock" | "csproj" | "fsproj" | "vbproj" => FileCategory::Config,

        // JSON/XML can be datasets, exports or documents. Without a recognized
        // configuration name above, leave their purpose unclassified.
        _ => FileCategory::Other,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeEventKind {
    PatchEdit,
    FullWrite,
}

#[derive(Debug, Clone)]
pub struct ParsedChangeEvent {
    pub timestamp: DateTime<Local>,
    pub model: String,
    #[allow(dead_code)]
    pub provider: String,
    pub path: String,
    pub kind: ChangeEventKind,
    pub added_lines: u64,
    pub removed_lines: u64,
    pub category: FileCategory,
    pub dedupe_key: Option<String>,
    pub agent_scope: crate::stats::subagent::AgentScope,
    /// Same key as the `ParsedEntry` rows of the session that made the edit,
    /// so edits can be tied back to what that session cost.
    pub session_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChangeStats {
    pub added_lines: u64,
    pub removed_lines: u64,
    pub net_lines: i64,
    pub files_touched: u32,
    pub change_events: u32,
    pub write_events: u32,
    pub code_lines_changed: u64,
    pub docs_lines_changed: u64,
    pub config_lines_changed: u64,
    pub other_lines_changed: u64,
    pub avg_lines_per_event: Option<f64>,
    pub cost_per_100_net_lines: Option<f64>,
    pub tokens_per_net_line: Option<f64>,
    pub rewrite_ratio: Option<f64>,
    pub churn_ratio: Option<f64>,
    pub dominant_extension: Option<String>,
    /// Conversations in the period that edited files, out of all that had
    /// usage. Subagents count toward the conversation that spawned them.
    #[serde(default)]
    pub edit_sessions: u32,
    #[serde(default)]
    pub total_sessions: u32,
    /// Share (0..=1) of the period's cost spent in conversations that edited
    /// files. `None` when the edits can't be tied to sessions; efficiency
    /// figures then fall back to the whole period.
    #[serde(default)]
    pub edit_cost_share: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelChangeSummary {
    pub added_lines: u64,
    pub removed_lines: u64,
    pub net_lines: i64,
    pub files_touched: u32,
    pub change_events: u32,
}

/// The conversation a session belongs to: subagent keys
/// (`<conversation>:subagent:<id>`) and Claude's `:main` suffix collapse onto
/// their parent, so work a main session delegates stays attributed to it.
pub fn conversation_key(session_key: &str) -> &str {
    let key = session_key
        .split_once(":subagent:")
        .map_or(session_key, |(conversation, _)| conversation);
    key.strip_suffix(":main").unwrap_or(key)
}

/// Cost, token and count totals for the conversations that edited files,
/// against the same totals for every conversation in `entries`.
struct EditScope {
    edit_sessions: u32,
    total_sessions: u32,
    cost_share: f64,
    token_share: f64,
}

fn edit_scope(events: &[ParsedChangeEvent], entries: &[ParsedEntry]) -> Option<EditScope> {
    let editing: HashSet<&str> = events
        .iter()
        .filter(|ev| !ev.session_key.is_empty() && (ev.added_lines > 0 || ev.removed_lines > 0))
        .map(|ev| conversation_key(&ev.session_key))
        .collect();
    if editing.is_empty() {
        return None;
    }

    let mut sessions = HashSet::new();
    let mut edit_sessions = HashSet::new();
    let (mut all_cost, mut edit_cost) = (0.0, 0.0);
    let (mut all_tokens, mut edit_tokens) = (0_u64, 0_u64);
    for entry in entries {
        let conversation = conversation_key(&entry.session_key);
        let cost = entry.cost_usd();
        let tokens = entry.total_tokens();
        sessions.insert(conversation);
        all_cost += cost;
        all_tokens += tokens;
        if editing.contains(conversation) {
            edit_sessions.insert(conversation);
            edit_cost += cost;
            edit_tokens += tokens;
        }
    }
    if edit_cost <= 0.0 || all_cost <= 0.0 {
        return None;
    }

    Some(EditScope {
        edit_sessions: edit_sessions.len() as u32,
        total_sessions: sessions.len() as u32,
        cost_share: edit_cost / all_cost,
        token_share: if all_tokens > 0 {
            edit_tokens as f64 / all_tokens as f64
        } else {
            1.0
        },
    })
}

/// `entries` are the period's usage rows; they scope the efficiency figures to
/// the conversations that actually edited files, so a day of research that
/// happens to include one small script isn't billed entirely to that script.
pub fn aggregate_change_stats(
    events: &[ParsedChangeEvent],
    entries: &[ParsedEntry],
    total_cost: f64,
    total_tokens: u64,
) -> Option<ChangeStats> {
    if events.is_empty() {
        return None;
    }

    let mut added: u64 = 0;
    let mut removed: u64 = 0;
    let mut code: u64 = 0;
    let mut docs: u64 = 0;
    let mut config: u64 = 0;
    let mut other: u64 = 0;
    let mut write_events: u32 = 0;
    let mut files = HashSet::new();

    for ev in events {
        added += ev.added_lines;
        removed += ev.removed_lines;
        let changed = ev.added_lines + ev.removed_lines;
        match ev.category {
            FileCategory::Code => code += changed,
            FileCategory::Docs => docs += changed,
            FileCategory::Config => config += changed,
            FileCategory::Other => other += changed,
        }
        if ev.kind == ChangeEventKind::FullWrite {
            write_events += 1;
        }
        files.insert(ev.path.clone());
    }

    let net = added as i64 - removed as i64;
    let change_events = events.len() as u32;
    let total_changed = added + removed;

    let avg_lines_per_event = if change_events > 0 {
        Some(total_changed as f64 / change_events as f64)
    } else {
        None
    };

    let scope = edit_scope(events, entries);
    let edit_cost = total_cost * scope.as_ref().map_or(1.0, |s| s.cost_share);
    let edit_tokens = total_tokens as f64 * scope.as_ref().map_or(1.0, |s| s.token_share);

    let cost_per_100 = if net > 0 {
        Some((edit_cost / net as f64) * 100.0)
    } else {
        None
    };

    let tokens_per = if net > 0 {
        Some(edit_tokens / net as f64)
    } else {
        None
    };

    let churn = if added > 0 {
        Some(removed as f64 / added as f64)
    } else {
        None
    };

    Some(ChangeStats {
        added_lines: added,
        removed_lines: removed,
        net_lines: net,
        files_touched: files.len() as u32,
        change_events,
        write_events,
        code_lines_changed: code,
        docs_lines_changed: docs,
        config_lines_changed: config,
        other_lines_changed: other,
        avg_lines_per_event,
        cost_per_100_net_lines: cost_per_100,
        tokens_per_net_line: tokens_per,
        rewrite_ratio: None,
        churn_ratio: churn,
        dominant_extension: None,
        edit_sessions: scope.as_ref().map_or(0, |s| s.edit_sessions),
        total_sessions: scope.as_ref().map_or(0, |s| s.total_sessions),
        edit_cost_share: scope.map(|s| s.cost_share),
    })
}

pub fn aggregate_model_change_summary(
    events: &[ParsedChangeEvent],
    model_key: &str,
) -> Option<ModelChangeSummary> {
    let model_events: Vec<&ParsedChangeEvent> =
        events.iter().filter(|e| e.model == model_key).collect();

    if model_events.is_empty() {
        return None;
    }

    let mut added: u64 = 0;
    let mut removed: u64 = 0;
    let mut files = HashSet::new();

    for ev in &model_events {
        added += ev.added_lines;
        removed += ev.removed_lines;
        files.insert(ev.path.clone());
    }

    Some(ModelChangeSummary {
        added_lines: added,
        removed_lines: removed,
        net_lines: added as i64 - removed as i64,
        files_touched: files.len() as u32,
        change_events: model_events.len() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_rust_file() {
        assert_eq!(classify_file("src/main.rs"), FileCategory::Code);
    }

    #[test]
    fn classify_typescript_file() {
        assert_eq!(classify_file("src/lib/types/index.ts"), FileCategory::Code);
    }

    #[test]
    fn classify_svelte_file() {
        assert_eq!(classify_file("src/App.svelte"), FileCategory::Code);
    }

    #[test]
    fn classify_markdown_file() {
        assert_eq!(classify_file("docs/README.md"), FileCategory::Docs);
    }

    #[test]
    fn classify_latex_as_docs() {
        assert_eq!(classify_file("HW1/main.tex"), FileCategory::Docs);
        assert_eq!(classify_file("paper/refs.bib"), FileCategory::Docs);
    }

    #[test]
    fn classify_json_file() {
        assert_eq!(classify_file("package.json"), FileCategory::Config);
    }

    #[test]
    fn classify_yaml_file() {
        assert_eq!(
            classify_file(".github/workflows/ci.yml"),
            FileCategory::Config
        );
    }

    #[test]
    fn classify_unknown_extension() {
        assert_eq!(classify_file("image.png"), FileCategory::Other);
    }

    #[test]
    fn classify_no_extension() {
        assert_eq!(classify_file("unknown"), FileCategory::Other);
    }

    #[test]
    fn classify_configuration_filenames_before_extensions() {
        for path in [
            ".gitignore",
            ".env.local",
            ".env.production.example",
            "Dockerfile",
            "docker/Dockerfile.dev",
            "Makefile",
            "CMakeLists.txt",
            "requirements-dev.txt",
            "Cargo.lock",
            "pnpm-lock.yaml",
            "vite.config.ts",
            ".eslintrc.cjs",
            r"C:\project\.ENV.LOCAL",
        ] {
            assert_eq!(classify_file(path), FileCategory::Config, "{path}");
        }
    }

    #[test]
    fn classify_tool_configuration_patterns() {
        for path in [
            "src-tauri/tauri.conf.json",
            ".vscode/extensions.json",
            "config/app.json",
        ] {
            assert_eq!(classify_file(path), FileCategory::Config, "{path}");
        }
    }

    #[test]
    fn classify_docs_about_configuration_as_docs() {
        for path in ["Dockerfile.md", ".env.local.md", ".eslintrc.md"] {
            assert_eq!(classify_file(path), FileCategory::Docs, "{path}");
        }
    }

    #[test]
    fn classify_extensionless_documents() {
        for path in [
            "README",
            "docs/LICENSE",
            "CHANGELOG",
            "CONTRIBUTING",
            "NOTICE",
        ] {
            assert_eq!(classify_file(path), FileCategory::Docs, "{path}");
        }
    }

    #[test]
    fn classify_notebooks_and_additional_source_languages() {
        for path in [
            "analysis.ipynb",
            "analysis.R",
            "model.jl",
            "install.ps1",
            "main.lua",
            "app.dart",
            "script.fish",
        ] {
            assert_eq!(classify_file(path), FileCategory::Code, "{path}");
        }
    }

    #[test]
    fn classify_uses_basename_on_both_platforms() {
        for path in [
            "/tmp/project.rs/unknown",
            r"C:\project.py\unknown",
            "notes.md/",
            "",
            ".rs",
        ] {
            assert_eq!(classify_file(path), FileCategory::Other, "{path}");
        }
        assert_eq!(classify_file("docs/example.py"), FileCategory::Code);
        assert_eq!(classify_file("src/example.py.md"), FileCategory::Docs);
        assert_eq!(classify_file("README.py"), FileCategory::Code);
    }

    #[test]
    fn classify_json_and_xml_data_without_assuming_configuration() {
        for path in [
            "data.json",
            "results.jsonl",
            "report.xml",
            "fixtures/package.json",
        ] {
            assert_eq!(classify_file(path), FileCategory::Other, "{path}");
        }
        for path in [
            "package.json",
            "tsconfig.app.json",
            ".vscode/settings.json",
            "app.config.json",
            "pom.xml",
        ] {
            assert_eq!(classify_file(path), FileCategory::Config, "{path}");
        }
    }

    #[test]
    fn classify_case_insensitive() {
        assert_eq!(classify_file("README.MD"), FileCategory::Docs);
    }

    // ── Aggregation tests ──

    use chrono::TimeZone;

    fn make_event(path: &str, added: u64, removed: u64, model: &str) -> ParsedChangeEvent {
        ParsedChangeEvent {
            timestamp: Local.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap(),
            model: model.to_string(),
            provider: "claude".to_string(),
            path: path.to_string(),
            kind: ChangeEventKind::PatchEdit,
            added_lines: added,
            removed_lines: removed,
            category: classify_file(path),
            dedupe_key: None,
            agent_scope: crate::stats::subagent::AgentScope::Main,
            session_key: String::new(),
        }
    }

    #[test]
    fn aggregate_empty_returns_none() {
        assert!(aggregate_change_stats(&[], &[], 0.0, 0).is_none());
    }

    #[test]
    fn aggregate_single_event() {
        let events = vec![make_event("src/main.rs", 10, 3, "opus-4-6")];
        let stats = aggregate_change_stats(&events, &[], 1.0, 1000).unwrap();
        assert_eq!(stats.added_lines, 10);
        assert_eq!(stats.removed_lines, 3);
        assert_eq!(stats.net_lines, 7);
        assert_eq!(stats.files_touched, 1);
        assert_eq!(stats.change_events, 1);
        assert_eq!(stats.code_lines_changed, 13);
        assert_eq!(stats.docs_lines_changed, 0);
    }

    #[test]
    fn aggregate_composition_partitions_all_lines() {
        let events = vec![
            make_event("src/main.rs", 50, 10, "opus-4-6"),
            make_event("README.md", 20, 5, "opus-4-6"),
            make_event("config.yaml", 8, 2, "opus-4-6"),
        ];
        let stats = aggregate_change_stats(&events, &[], 5.0, 10000).unwrap();
        let total = stats.code_lines_changed
            + stats.docs_lines_changed
            + stats.config_lines_changed
            + stats.other_lines_changed;
        assert_eq!(total, stats.added_lines + stats.removed_lines);
    }

    #[test]
    fn aggregate_dedupes_files() {
        let events = vec![
            make_event("src/main.rs", 10, 0, "opus-4-6"),
            make_event("src/main.rs", 5, 2, "opus-4-6"),
        ];
        let stats = aggregate_change_stats(&events, &[], 1.0, 1000).unwrap();
        assert_eq!(stats.files_touched, 1);
        assert_eq!(stats.change_events, 2);
    }

    #[test]
    fn aggregate_negative_net() {
        let events = vec![make_event("src/main.rs", 5, 20, "opus-4-6")];
        let stats = aggregate_change_stats(&events, &[], 1.0, 1000).unwrap();
        assert_eq!(stats.net_lines, -15);
        assert!(stats.cost_per_100_net_lines.is_none());
        assert!(stats.tokens_per_net_line.is_none());
    }

    #[test]
    fn aggregate_efficiency_when_positive_net() {
        let events = vec![make_event("src/main.rs", 100, 0, "opus-4-6")];
        let stats = aggregate_change_stats(&events, &[], 5.0, 50000).unwrap();
        assert!((stats.cost_per_100_net_lines.unwrap() - 5.0).abs() < 0.01);
        assert!((stats.tokens_per_net_line.unwrap() - 500.0).abs() < 0.01);
    }

    fn make_entry(session_key: &str, input_tokens: u64) -> ParsedEntry {
        ParsedEntry {
            timestamp: Local.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap(),
            model: "claude-opus-4-6".to_string(),
            input_tokens,
            output_tokens: 0,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: 0,
            web_search_requests: 0,
            unique_hash: None,
            session_key: session_key.to_string(),
            agent_scope: crate::stats::subagent::AgentScope::Main,
        }
    }

    #[test]
    fn conversation_key_folds_subagents_and_main_suffix() {
        assert_eq!(conversation_key("claude:s1:main"), "claude:s1");
        assert_eq!(conversation_key("claude:s1:subagent:a1"), "claude:s1");
        assert_eq!(conversation_key("codex:p1:subagent:c1"), "codex:p1");
        assert_eq!(conversation_key("codex:p1"), "codex:p1");
    }

    #[test]
    fn efficiency_counts_only_conversations_that_edited() {
        // s1 edits through a subagent; s2 is research with no edits and costs
        // as much as all of s1.
        let mut edit = make_event("src/main.rs", 100, 0, "opus-4-6");
        edit.session_key = "claude:s1:subagent:a1".to_string();
        let entries = vec![
            make_entry("claude:s1:main", 1_000),
            make_entry("claude:s1:subagent:a1", 1_000),
            make_entry("claude:s2:main", 2_000),
        ];

        let stats = aggregate_change_stats(&[edit], &entries, 10.0, 4_000).unwrap();
        assert_eq!((stats.edit_sessions, stats.total_sessions), (1, 2));
        assert!((stats.edit_cost_share.unwrap() - 0.5).abs() < 1e-9);
        assert!((stats.cost_per_100_net_lines.unwrap() - 5.0).abs() < 1e-9);
        assert!((stats.tokens_per_net_line.unwrap() - 20.0).abs() < 1e-9);
    }

    #[test]
    fn efficiency_falls_back_to_period_totals_without_session_data() {
        let mut edit = make_event("src/main.rs", 100, 0, "opus-4-6");
        edit.session_key = "claude:s1:main".to_string();

        let stats = aggregate_change_stats(&[edit], &[], 5.0, 50_000).unwrap();
        assert!(stats.edit_cost_share.is_none());
        assert_eq!((stats.edit_sessions, stats.total_sessions), (0, 0));
        assert!((stats.cost_per_100_net_lines.unwrap() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn zero_line_events_do_not_classify_a_conversation_as_editing() {
        let mut edit = make_event("src/main.rs", 100, 0, "opus-4-6");
        edit.session_key = "claude:s1:main".to_string();
        let mut empty_write = make_event("notes.md", 0, 0, "opus-4-6");
        empty_write.session_key = "claude:s2:main".to_string();
        let entries = vec![
            make_entry("claude:s1:main", 1_000),
            make_entry("claude:s2:main", 1_000),
        ];
        let stats = aggregate_change_stats(&[edit, empty_write], &entries, 10.0, 2_000).unwrap();
        assert_eq!((stats.edit_sessions, stats.total_sessions), (1, 2));
        assert_eq!(stats.edit_cost_share, Some(0.5));
        assert_eq!(stats.cost_per_100_net_lines, Some(5.0));
    }

    #[test]
    fn model_summary_filters_by_model() {
        let events = vec![
            make_event("src/a.rs", 30, 5, "opus-4-6"),
            make_event("src/b.rs", 10, 2, "sonnet-4-6"),
        ];
        let summary = aggregate_model_change_summary(&events, "opus-4-6").unwrap();
        assert_eq!(summary.added_lines, 30);
        assert_eq!(summary.removed_lines, 5);
        assert_eq!(summary.change_events, 1);
    }
}
