use crate::models::{
    ActiveBlock, ChartBucket, ChartSegment, ModelSummary, UsagePayload, UsageSource,
};
use crate::stats::change::ParsedChangeEvent;
#[cfg(test)]
use crate::stats::change::{ChangeEventKind, FileCategory};
use crate::usage::integrations::{
    provider_matches_model, UsageIntegrationId, UsageIntegrationSelection,
};
use chrono::{DateTime, Duration, Local, NaiveDate, Timelike};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

#[cfg(test)]
use super::claude_parser::read_claude_entries;
use super::claude_parser::{
    parse_claude_session_file, upsert_claude_change_event, upsert_claude_entry, ClaudeDedupeAction,
};
use super::codex_parser::parse_codex_session_file;
use super::cursor_parser::{cursor_last_warning, parse_cursor_session_file, set_cursor_warning};
use super::kimi_parser::parse_kimi_session_file;

// ─────────────────────────────────────────────────────────────────────────────
// Parsed entry (shared between Claude and Codex)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ParsedEntry {
    pub timestamp: DateTime<Local>,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_5m_tokens: u64,
    pub cache_creation_1h_tokens: u64,
    pub cache_read_tokens: u64,
    pub web_search_requests: u64,
    pub unique_hash: Option<String>,
    pub session_key: String,
    pub agent_scope: crate::stats::subagent::AgentScope,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderReadDebug {
    pub provider: String,
    pub root_dir: String,
    pub root_exists: bool,
    pub since: Option<String>,
    pub strategy: String,
    pub listing_cache_hit: bool,
    pub discovered_paths: usize,
    pub attempted_paths: usize,
    pub opened_paths: usize,
    pub skipped_paths: usize,
    pub skipped_by_mtime: usize,
    pub failed_paths: usize,
    pub lines_read: usize,
    pub emitted_entries: usize,
    pub visited_day_dirs: usize,
    pub existing_day_dirs: usize,
    pub sample_paths: Vec<String>,
    pub sample_skipped_paths: Vec<String>,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageQueryDebugReport {
    pub provider: String,
    pub aggregation: String,
    pub since: String,
    pub cache_key: String,
    pub from_cache: bool,
    pub entry_count: usize,
    pub sources: Vec<ProviderReadDebug>,
}

#[derive(Clone, PartialEq, Eq)]
struct FileStamp {
    modified: SystemTime,
    len: u64,
}

#[derive(Clone)]
struct DirectoryStamp {
    path: PathBuf,
    modified: SystemTime,
}

#[derive(Clone)]
struct FileListStamp {
    path: PathBuf,
    stamp: FileStamp,
}

#[derive(Clone)]
struct CachedRootFileList {
    files: Arc<[PathBuf]>,
    directories: Arc<[DirectoryStamp]>,
    file_stamps: Arc<[FileListStamp]>,
    last_accessed_at: Instant,
}

/// One root's files by path: the stamp each was last read at, and the
/// earliest entry date it held.
type FileEarliestDates = HashMap<PathBuf, (FileStamp, Option<NaiveDate>)>;

/// One root's listing as a sweep found it, when a directory's or a file's
/// stamp in it moved.
#[derive(Default)]
struct RootSweep {
    cache_key: String,
    directories: Vec<DirectoryStamp>,
    file_stamps: Vec<FileListStamp>,
    /// Files came or went.
    set_changed: bool,
    /// The files whose stamp moved, by cache key.
    changed_keys: Vec<String>,
    /// When the earliest written of them had been written before.
    changed_since: Option<SystemTime>,
    /// One of them shrank: rewritten, not appended to.
    shrunk: bool,
}

/// Outcome of one sweep over the cached listings.
#[derive(Default)]
struct SourceChangeScan {
    /// Nothing listed yet (or a poisoned lock): every root is re-listed.
    listing_changed: bool,
    /// Roots that are gone: their listings are dropped.
    dropped: Vec<String>,
    roots: Vec<RootSweep>,
}

/// What a sweep found in the logs.
#[derive(Debug, PartialEq)]
pub(crate) enum LogChanges {
    None,
    /// Only lines appended to logs already listed.
    Appended(LogAppends),
    /// Anything else: files came, went or shrank, a Cursor chat (a JSON
    /// document, written whole) changed, or nothing was listed yet.
    Any,
}

/// Lines appended to the logs of `integrations`, none dated before `since`.
#[derive(Debug, PartialEq)]
pub(crate) struct LogAppends {
    pub integrations: Vec<UsageIntegrationId>,
    pub since: NaiveDate,
}

/// A line is dated when its request was made, which may come before the
/// file's previous write, though not by this much.
const APPEND_DATE_SLACK: Duration = Duration::hours(1);

/// A file written within this long is stat'ed at every sweep; an older one
/// only at a full sweep.
const HOT_FILE_AGE: std::time::Duration = std::time::Duration::from_secs(7 * 86_400);

#[derive(Clone)]
struct CachedFileEntries {
    stamp: FileStamp,
    entries: Arc<[ParsedEntry]>,
    change_events: Arc<[ParsedChangeEvent]>,
    earliest_date: Option<NaiveDate>,
    last_accessed_at: Instant,
}

#[derive(Clone, Copy)]
enum ProviderFileKind {
    Claude,
    Codex,
    Cursor,
    Kimi,
}

impl ProviderFileKind {
    fn parse(self, path: &Path) -> SessionParseResult {
        match self {
            Self::Claude => parse_claude_session_file(path),
            Self::Codex => parse_codex_session_file(path),
            Self::Cursor => parse_cursor_session_file(path),
            Self::Kimi => parse_kimi_session_file(path),
        }
    }

    /// Whether the tree walk lists `path`: a `.jsonl` log, or for Cursor a
    /// chat JSON in a workspace's `chatSessions` directory.
    fn is_session_file(self, path: &Path) -> bool {
        match self {
            Self::Cursor => {
                path.extension().is_some_and(|e| e == "json")
                    && path
                        .parent()
                        .and_then(Path::file_name)
                        .is_some_and(|name| name == "chatSessions")
            }
            _ => path.extension().is_some_and(|e| e == "jsonl"),
        }
    }
}

#[derive(Clone)]
struct UsageIntegrationConfig {
    id: UsageIntegrationId,
    roots: Vec<PathBuf>,
}

impl UsageIntegrationConfig {
    fn new(id: UsageIntegrationId, roots: Vec<PathBuf>) -> Self {
        Self { id, roots }
    }

    fn file_kind(&self) -> ProviderFileKind {
        match self.id {
            UsageIntegrationId::Claude => ProviderFileKind::Claude,
            UsageIntegrationId::Codex => ProviderFileKind::Codex,
            UsageIntegrationId::Cursor => ProviderFileKind::Cursor,
            UsageIntegrationId::Kimi => ProviderFileKind::Kimi,
        }
    }

    fn scan_strategy(&self) -> &'static str {
        match self.id {
            UsageIntegrationId::Claude => {
                "recursive-jsonl-glob+root-file-list-cache+parsed-file-cache+dedupe"
            }
            UsageIntegrationId::Codex => {
                "recursive-jsonl-glob+root-file-list-cache+parsed-file-cache+token-delta"
            }
            UsageIntegrationId::Cursor => "workspace-chat-json+token-field-probe+cursor-remote-api",
            UsageIntegrationId::Kimi => {
                "recursive-jsonl-glob+root-file-list-cache+parsed-file-cache+turn-scoped"
            }
        }
    }

    fn dedupe_entry_hashes(&self) -> bool {
        matches!(
            self.id,
            UsageIntegrationId::Claude | UsageIntegrationId::Cursor
        )
    }

    fn dedupe_change_events(&self) -> bool {
        matches!(self.id, UsageIntegrationId::Claude)
    }
}

struct CachedFileLoad {
    entries: Arc<[ParsedEntry]>,
    change_events: Arc<[ParsedChangeEvent]>,
    earliest_date: Option<NaiveDate>,
    lines_read: usize,
    opened: bool,
    from_cache: bool,
}

/// Shared result of a single `load_entries` call, cached for reuse within
/// the same IPC request scope.
#[derive(Clone, Default)]
pub(crate) struct LoadedEntries {
    pub entries: Vec<ParsedEntry>,
    pub change_events: Vec<ParsedChangeEvent>,
    #[allow(dead_code)]
    pub reports: Vec<ProviderReadDebug>,
}

struct PayloadCacheEntry {
    payload: UsagePayload,
    stored_at: Instant,
    last_accessed_at: Instant,
}

// ─────────────────────────────────────────────────────────────────────────────
// File scanning helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Recursively find all `.jsonl` files under `dir`.
///
/// Symlinks are not followed: traversing a symlink may cross onto a network
/// volume, external disk, or other TCC-guarded location and cause macOS to
/// prompt the user for access they never asked for. Regular files reached via
/// symlink are still accepted (reading a symlinked JSONL doesn't recurse), but
/// symlinked directories are skipped.
pub(crate) fn glob_jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if !dir.exists() {
        return results;
    }
    tracing::debug!(path = %dir.display(), "read_dir (glob_jsonl_files)");
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::debug!(path = %dir.display(), error = %e, "read_dir failed");
            return results;
        }
    };
    for entry in rd.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_symlink() {
            tracing::debug!(path = %path.display(), "skipping symlink");
            continue;
        }
        if file_type.is_dir() {
            let mut sub = glob_jsonl_files(&path);
            results.append(&mut sub);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            results.push(path);
        }
    }
    results.sort();
    results
}

/// Parse a `since` string in `YYYYMMDD` format into a `NaiveDate`.
pub(crate) fn parse_since_date(since: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(since, "%Y%m%d").ok()
}

pub(crate) fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

pub(crate) fn push_sample_path(sample_paths: &mut Vec<String>, path: &Path) {
    if sample_paths.len() < 5 {
        sample_paths.push(path_to_string(path));
    }
}

fn scan_jsonl_tree_into(
    dir: &Path,
    kind: ProviderFileKind,
    files: &mut Vec<PathBuf>,
    directories: &mut Vec<DirectoryStamp>,
) {
    // symlink_metadata doesn't follow symlinks; we refuse to recurse through
    // them so the walker stays on the volume the user originally opted into.
    let metadata = match fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(e) => {
            tracing::debug!(path = %dir.display(), error = %e, "symlink_metadata failed");
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        tracing::debug!(path = %dir.display(), "skipping symlink dir");
        return;
    }
    let modified = match metadata.modified() {
        Ok(modified) => modified,
        Err(_) => return,
    };
    directories.push(DirectoryStamp {
        path: dir.to_path_buf(),
        modified,
    });

    tracing::debug!(path = %dir.display(), "read_dir (scan_jsonl_tree)");
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::debug!(path = %dir.display(), error = %e, "read_dir failed");
            return;
        }
    };
    for entry in rd.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_symlink() {
            tracing::debug!(path = %path.display(), "skipping symlink");
            continue;
        }
        if file_type.is_dir() {
            scan_jsonl_tree_into(&path, kind, files, directories);
        } else if kind.is_session_file(&path) {
            files.push(path);
        }
    }
}

fn scan_jsonl_tree(dir: &Path, kind: ProviderFileKind) -> (Vec<PathBuf>, Vec<DirectoryStamp>) {
    let mut files = Vec::new();
    let mut directories = Vec::new();
    if !dir.exists() {
        return (files, directories);
    }
    scan_jsonl_tree_into(dir, kind, &mut files, &mut directories);
    files.sort();
    (files, directories)
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileStamp {
        modified: metadata.modified().ok()?,
        len: metadata.len(),
    })
}

/// Sweep one root's listing: stat its files, every one when `full`, else
/// those written within [`HOT_FILE_AGE`], each by its own stat (on NTFS a
/// directory entry lags behind a file a writer holds open); stat its
/// directories, and read again those whose stamp moved ([`relist_dir`]), so
/// a file that came or went costs one directory read, not a walk of the
/// tree. `None` when `root` is gone, `Some(None)` when nothing moved.
fn sweep_root(
    root: &Path,
    entry: &CachedRootFileList,
    kind: ProviderFileKind,
    full: bool,
) -> Option<Option<RootSweep>> {
    let now = SystemTime::now();
    // The idle path allocates nothing: only what moved is collected.
    let mut restat = Vec::new();
    for (i, file) in entry.file_stamps.iter().enumerate() {
        let cold = now
            .duration_since(file.stamp.modified)
            .is_ok_and(|age| age >= HOT_FILE_AGE);
        if cold && !full {
            continue;
        }
        let stamp = file_stamp(&file.path);
        if stamp.is_none() && !is_gone(&file.path) {
            continue;
        }
        if stamp.as_ref() != Some(&file.stamp) {
            restat.push((i, stamp));
        }
    }
    let mut moved = Vec::new();
    for dir in entry.directories.iter() {
        let modified = fs::metadata(&dir.path).and_then(|m| m.modified()).ok();
        if modified.is_none() && !is_gone(&dir.path) {
            continue;
        }
        if modified.is_none() && dir.path == root {
            return None;
        }
        if modified != Some(dir.modified) {
            moved.push((dir.path.clone(), modified));
        }
    }
    if restat.is_empty() && moved.is_empty() {
        return Some(None);
    }

    let mut sweep = RootSweep {
        directories: entry.directories.to_vec(),
        ..RootSweep::default()
    };
    let mut restat = restat.into_iter().peekable();
    for (i, file) in entry.file_stamps.iter().enumerate() {
        match restat.next_if(|(at, _)| *at == i) {
            None => sweep.file_stamps.push(file.clone()),
            // Gone without its directory's stamp moving (a race, or a coarse
            // clock): it leaves the listing.
            Some((_, None)) => sweep.set_changed = true,
            Some((_, Some(stamp))) => {
                sweep.shrunk |= stamp.len < file.stamp.len;
                let before = file.stamp.modified;
                sweep.changed_since = Some(sweep.changed_since.map_or(before, |at| at.min(before)));
                sweep.changed_keys.push(path_to_string(&file.path));
                sweep.file_stamps.push(FileListStamp {
                    path: file.path.clone(),
                    stamp,
                });
            }
        }
    }
    // Parents first: a subdirectory that left with its parent is not read.
    moved.sort_by_key(|(path, _)| path.components().count());
    for (dir, modified) in &moved {
        sweep.set_changed |= relist_dir(
            dir,
            *modified,
            kind,
            &mut sweep.directories,
            &mut sweep.file_stamps,
        );
    }
    if sweep.set_changed {
        sweep.file_stamps.sort_by(|a, b| a.path.cmp(&b.path));
    }
    Some(Some(sweep))
}

/// Whether `path` is known to be gone. One that could not be read (a flaky
/// network share, a lock) keeps its stamp and is tried again at the next sweep.
fn is_gone(path: &Path) -> bool {
    matches!(path.try_exists(), Ok(false))
}

/// Read `dir` again, its stamp now `modified` (`None`: gone): the session
/// files and subdirectories it holds replace those listed under it. A new
/// subdirectory is walked whole, and all under one that left leaves with
/// it. Returns whether the listed files changed.
fn relist_dir(
    dir: &Path,
    modified: Option<SystemTime>,
    kind: ProviderFileKind,
    directories: &mut Vec<DirectoryStamp>,
    files: &mut Vec<FileListStamp>,
) -> bool {
    if !directories.iter().any(|d| d.path == dir) {
        return false;
    }
    let mut held_files = HashSet::new();
    let mut held_dirs = HashSet::new();
    if modified.is_some() {
        // Unread, it keeps its stamp and is tried again at the next sweep.
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                held_dirs.insert(path);
            } else if !file_type.is_symlink() && kind.is_session_file(&path) {
                held_files.insert(path);
            }
        }
    }
    let gone: Vec<PathBuf> = match modified {
        None => vec![dir.to_path_buf()],
        Some(_) => directories
            .iter()
            .filter(|d| d.path.parent() == Some(dir) && !held_dirs.contains(&d.path))
            .map(|d| d.path.clone())
            .collect(),
    };
    let left = |path: &Path| gone.iter().any(|gone| path.starts_with(gone));
    directories.retain(|d| !left(&d.path));
    let before = files.len();
    files.retain(|f| {
        !left(&f.path) && (f.path.parent() != Some(dir) || held_files.contains(&f.path))
    });
    let removed = files.len() != before;
    let Some(modified) = modified else {
        return removed;
    };

    for listed in directories.iter_mut().filter(|d| d.path == dir) {
        listed.modified = modified;
    }
    let known: HashSet<&Path> = directories
        .iter()
        .map(|d| d.path.as_path())
        .chain(files.iter().map(|f| f.path.as_path()))
        .filter(|path| path.parent() == Some(dir))
        .collect();
    let mut came: Vec<PathBuf> = held_files
        .into_iter()
        .filter(|path| !known.contains(path.as_path()))
        .collect();
    let new_dirs: Vec<PathBuf> = held_dirs
        .into_iter()
        .filter(|path| !known.contains(path.as_path()))
        .collect();
    for sub in &new_dirs {
        scan_jsonl_tree_into(sub, kind, &mut came, directories);
    }
    let before = files.len();
    files.extend(
        came.into_iter()
            .filter_map(|path| file_stamp(&path).map(|stamp| FileListStamp { path, stamp })),
    );
    removed || files.len() != before
}

fn earliest_entry_date(entries: &[ParsedEntry]) -> Option<NaiveDate> {
    entries
        .iter()
        .map(|entry| entry.timestamp.date_naive())
        .min()
}

// ─────────────────────────────────────────────────────────────────────────────
// Model normalisation helper
// ─────────────────────────────────────────────────────────────────────────────

fn normalize_model(raw: &str) -> (String, String) {
    let known = crate::models::known_model_from_raw(raw);
    (known.display_name, known.model_key)
}

// ─────────────────────────────────────────────────────────────────────────────
// Provider-specific readers
// ─────────────────────────────────────────────────────────────────────────────

/// Check if a file was modified on or after the given date.
pub(crate) fn modified_since(path: &Path, since: NaiveDate) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| {
            let dt: chrono::DateTime<Local> = t.into();
            dt.date_naive() >= since
        })
        .unwrap_or(true) // if we can't read metadata, include the file
}

/// Count added and removed lines in a unified diff.
/// Lines starting with `+` (but not `+++`) are additions.
/// Lines starting with `-` (but not `---`) are removals.
pub(crate) fn count_diff_lines(patch: &str) -> (u64, u64) {
    let mut added: u64 = 0;
    let mut removed: u64 = 0;
    for line in patch.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            removed += 1;
        }
    }
    (added, removed)
}

pub(crate) type SessionParseResult = (Vec<ParsedEntry>, Vec<ParsedChangeEvent>, usize, bool);

// ─────────────────────────────────────────────────────────────────────────────
// Hour label helper
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) fn format_hour(h: u32) -> String {
    match h {
        0 => "12AM".into(),
        1..=11 => format!("{}AM", h),
        12 => "12PM".into(),
        _ => format!("{}PM", h - 12),
    }
}

fn truncate_hour(ts: DateTime<Local>) -> DateTime<Local> {
    ts.with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(ts)
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared aggregation utility — build segments map for a bucket
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct SegmentAgg {
    display_name: String,
    cost: f64,
    tokens: u64,
    pricing_available: bool,
}

/// Aggregate (display_name, cost, tokens, pricing_available) keyed by model_key
/// for a slice of entries.
/// (display_name, model_key, USD cost) of one entry.
fn price_entry(e: &ParsedEntry) -> (String, String, f64) {
    let (name, key) = normalize_model(&e.model);
    let cost = crate::usage::pricing::calculate_cost_for_key(
        &key,
        e.input_tokens,
        e.output_tokens,
        e.cache_creation_5m_tokens,
        e.cache_creation_1h_tokens,
        e.cache_read_tokens,
        e.web_search_requests,
    ) * crate::usage::pricing::provider_multiplier(&e.model);
    (name, key, cost)
}

fn build_segment_map(entries: &[&ParsedEntry]) -> HashMap<String, SegmentAgg> {
    let mut map: HashMap<String, SegmentAgg> = HashMap::new();
    for e in entries {
        let (name, key, cost) = price_entry(e);
        let pricing_available = crate::usage::pricing::pricing_available_for_key(&key);
        let entry = map.entry(key).or_insert(SegmentAgg {
            display_name: name,
            cost: 0.0,
            tokens: 0,
            pricing_available: true,
        });
        entry.cost += cost;
        entry.tokens += entry_total_tokens(e);
        entry.pricing_available &= pricing_available;
    }
    map
}

fn entry_total_tokens(entry: &ParsedEntry) -> u64 {
    entry.input_tokens
        + entry.output_tokens
        + entry.cache_creation_5m_tokens
        + entry.cache_creation_1h_tokens
        + entry.cache_read_tokens
}

fn entry_archive_hour(entry: &ParsedEntry) -> (NaiveDate, u8) {
    (entry.timestamp.date_naive(), entry.timestamp.hour() as u8)
}

fn merge_archived_and_live_entries(
    out: &mut Vec<ParsedEntry>,
    archived: Vec<ParsedEntry>,
    live: Vec<ParsedEntry>,
    frontier: Option<super::archive::ArchiveFrontier>,
) {
    let Some(frontier) = frontier else {
        out.extend(live);
        return;
    };

    let mut archived_keys_by_hour: HashMap<(NaiveDate, u8), HashSet<String>> = HashMap::new();
    for entry in &archived {
        archived_keys_by_hour
            .entry(entry_archive_hour(entry))
            .or_default()
            .insert(crate::models::normalized_model_key(&entry.model));
    }

    let mut replacement_hours: HashSet<(NaiveDate, u8)> = HashSet::new();
    let mut live_to_add = Vec::new();
    for entry in live {
        let hour = entry_archive_hour(&entry);
        if frontier.covers(hour.0, hour.1) {
            let model_key = crate::models::normalized_model_key(&entry.model);
            match archived_keys_by_hour.get(&hour) {
                Some(archived_keys) => {
                    if archived_keys.contains("unknown")
                        && model_key != "unknown"
                        && !archived_keys.contains(&model_key)
                    {
                        replacement_hours.insert(hour);
                        live_to_add.push(entry);
                    }
                }
                // Frontier can span empty hours between archived rows; keep
                // later-arriving live data for hours that have no archive rows.
                None => live_to_add.push(entry),
            }
        } else {
            live_to_add.push(entry);
        }
    }

    out.extend(archived.into_iter().filter(|entry| {
        let hour = entry_archive_hour(entry);
        !(replacement_hours.contains(&hour)
            && crate::models::normalized_model_key(&entry.model) == "unknown")
    }));
    out.extend(live_to_add);
}

fn segment_map_to_vec(map: HashMap<String, SegmentAgg>) -> Vec<ChartSegment> {
    map.into_iter()
        .map(|(key, agg)| ChartSegment {
            model: agg.display_name,
            model_key: key,
            cost: agg.cost,
            tokens: agg.tokens,
            pricing_available: agg.pricing_available,
        })
        .collect()
}

fn segment_map_to_model_summaries(map: &HashMap<String, SegmentAgg>) -> Vec<ModelSummary> {
    map.iter()
        .map(|(key, agg)| ModelSummary {
            display_name: agg.display_name.clone(),
            model_key: key.clone(),
            cost: agg.cost,
            tokens: agg.tokens,
            pricing_available: agg.pricing_available,
            change_stats: None,
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// UsageParser
// ─────────────────────────────────────────────────────────────────────────────

const CACHE_TTL_SECS: u64 = 120;
/// How long a failed Cursor remote fetch suppresses further fetch attempts.
///
/// A failed fetch stores no entries, so without this marker
/// [`UsageParser::needs_cursor_remote_fetch`] stays `true` forever and every
/// UI/tray refresh respawns the same failing fetch — a tight retry loop. Held
/// at the cache TTL so a persistently failing endpoint is retried at most once
/// per refresh interval, exactly like an expired cache.
const CURSOR_REMOTE_FAILURE_COOLDOWN_SECS: u64 = CACHE_TTL_SECS;
/// Longest a Cursor fetch that keeps finding nothing new is spaced out to.
/// Usage from other machines can show up this late while the IDE here idles.
const CURSOR_IDLE_MAX_SECS: u64 = 1800;

/// The next wait between Cursor fetches: back to `base` when the last fetch
/// changed something, else doubled, up to [`CURSOR_IDLE_MAX_SECS`].
pub(crate) fn cursor_idle_backoff(current: u64, base: u64, changed: bool) -> u64 {
    if changed {
        base
    } else {
        current.saturating_mul(2).clamp(base, CURSOR_IDLE_MAX_SECS)
    }
}
const MAX_PAYLOAD_CACHE_ENTRIES: usize = 256;
const MAX_FILE_CACHE_ENTRIES: usize = 4096;

pub struct UsageParser {
    integrations: Vec<UsageIntegrationConfig>,
    cache: Mutex<HashMap<String, PayloadCacheEntry>>,
    file_cache: Mutex<HashMap<String, CachedFileEntries>>,
    root_file_lists: Mutex<HashMap<String, CachedRootFileList>>,
    last_query_debug: Mutex<Option<UsageQueryDebugReport>>,
    archive: Mutex<Option<super::archive::ArchiveManager>>,
    entries_cache: Mutex<HashMap<String, (Instant, Arc<LoadedEntries>)>>,
    cursor_remote_cache: Mutex<Option<CachedCursorRemote>>,
    /// When the last background Cursor remote fetch failed. Gates
    /// `needs_cursor_remote_fetch` for `CURSOR_REMOTE_FAILURE_COOLDOWN_SECS`
    /// so a failing fetch cannot be respawned on every refresh.
    cursor_remote_failure_at: Mutex<Option<Instant>>,
    /// How long the Cursor remote data stays fresh for the periodic refresh:
    /// [`CACHE_TTL_SECS`], doubled by each refresh that found nothing new (see
    /// [`cursor_idle_backoff`]), and back to the base on Cursor activity.
    cursor_remote_ttl_secs: AtomicU64,
    /// Bumped whenever the Cursor remote data changes. A compute that saw it
    /// move built from a superseded snapshot and must not be cached; see
    /// [`UsageParser::store_cache_at_cursor_generation`].
    cursor_remote_generation: AtomicU64,
    /// Earliest entry date per provider string, cached so `has_entries_before`
    /// answers in O(1) instead of re-scanning every session file per query.
    /// Invalidated on a listing change ([`UsageParser::sweep`]) and `clear_cache`.
    earliest_date_cache: Mutex<HashMap<String, Option<NaiveDate>>>,
    /// Each listed file's earliest entry date by root, so recomputing a
    /// provider's earliest date parses only the files new since, or shrunk.
    file_earliest_dates: Mutex<HashMap<String, FileEarliestDates>>,
    /// How long a payload cache entry lives; see [`payload_ttl_for`].
    payload_ttl_secs: AtomicU64,
    /// When set, only the sweep ([`UsageParser::sweep`]) revalidates the
    /// session-file listings; queries reuse them as built.
    listings_frozen: AtomicBool,
    /// Bumped per integration at each sweep that finds its logs changed; the
    /// first listing or a cache clear bumps them all.
    log_changes: Mutex<HashMap<UsageIntegrationId, u64>>,
}

/// Payload cache TTL for a refresh interval: it outlives the interval, so a
/// view computed from one sample is reused until the next. "Off" (0) keeps
/// it for a day; the sample that finds a change clears it anyway.
pub(crate) fn payload_ttl_for(interval_secs: u64) -> u64 {
    if interval_secs == 0 {
        86_400
    } else {
        CACHE_TTL_SECS.max(interval_secs.saturating_mul(2))
    }
}

/// The `entries_cache` key of a load; the loads from the logs alone apart.
fn entries_cache_key(provider: &str, since: Option<NaiveDate>, with_archive: bool) -> String {
    format!(
        "{}{}:{}",
        if with_archive { "" } else { "live:" },
        provider,
        since.map(|d| d.to_string()).unwrap_or_default()
    )
}

/// Cached result of a background Cursor remote API fetch.
///
/// Non-consuming and range-tagged: one fetch of the widest opened range serves
/// every period view by filtering on the request's `since`. `covered_since` is
/// the `since` the fetch used (`None` = all time); the cache satisfies any
/// request whose `since >= covered_since`.
#[derive(Clone)]
pub(crate) struct CachedCursorRemote {
    pub entries: Vec<ParsedEntry>,
    pub stored_at: Instant,
    pub covered_since: Option<NaiveDate>,
}

/// True when a cache covering `[covered_since, now]` satisfies a request for
/// `[req_since, now]` (the request is a subset). `None` = all time (widest).
fn cursor_range_covers(covered_since: Option<NaiveDate>, req_since: Option<NaiveDate>) -> bool {
    match (covered_since, req_since) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(covered), Some(req)) => req >= covered,
    }
}

/// True when `entry` is dated before `since` (`None` = all time, so never).
fn cursor_entry_before(entry: &ParsedEntry, since: Option<NaiveDate>) -> bool {
    since.is_some_and(|since| entry.timestamp.date_naive() < since)
}

/// Cheap change check for a slice of Cursor remote entries:
/// (count, token sums per type, latest timestamp). Per type because moving
/// tokens between types changes the cost without changing the total.
fn cursor_entries_fingerprint(
    entries: &[ParsedEntry],
) -> (usize, [u64; 5], Option<DateTime<Local>>) {
    // Wrapping: the sums are only compared for equality, and a panic here
    // would poison the Cursor cache mutex held by `store_cursor_remote`.
    let mut tokens = [0u64; 5];
    for e in entries {
        tokens[0] = tokens[0].wrapping_add(e.input_tokens);
        tokens[1] = tokens[1].wrapping_add(e.output_tokens);
        tokens[2] = tokens[2].wrapping_add(e.cache_creation_5m_tokens);
        tokens[3] = tokens[3].wrapping_add(e.cache_creation_1h_tokens);
        tokens[4] = tokens[4].wrapping_add(e.cache_read_tokens);
    }
    (
        entries.len(),
        tokens,
        entries.iter().map(|e| e.timestamp).max(),
    )
}

fn insert_payload_cache_entry(
    cache: &mut HashMap<String, PayloadCacheEntry>,
    key: &str,
    payload: UsagePayload,
    ttl_secs: u64,
) {
    let now = Instant::now();
    cache.insert(
        key.to_string(),
        PayloadCacheEntry {
            payload,
            stored_at: now,
            last_accessed_at: now,
        },
    );
    prune_payload_cache(cache, ttl_secs);
}

fn prune_payload_cache(cache: &mut HashMap<String, PayloadCacheEntry>, ttl_secs: u64) {
    let now = Instant::now();
    cache.retain(|_, entry| now.duration_since(entry.stored_at).as_secs() < ttl_secs);

    if cache.len() <= MAX_PAYLOAD_CACHE_ENTRIES {
        return;
    }

    let mut oldest_keys: Vec<(String, Instant)> = cache
        .iter()
        .map(|(key, entry)| (key.clone(), entry.last_accessed_at))
        .collect();
    oldest_keys.sort_by_key(|(_, last_accessed_at)| *last_accessed_at);

    for (key, _) in oldest_keys
        .into_iter()
        .take(cache.len().saturating_sub(MAX_PAYLOAD_CACHE_ENTRIES))
    {
        cache.remove(&key);
    }
}

/// A file last written this long ago serves only views far back (Year,
/// earlier months). Once no load has read it for `FILE_CACHE_IDLE` it leaves
/// the cache and is re-parsed on demand, which keeps the cache well under
/// its cap and its memory to the recent logs.
const OLD_FILE_AGE: std::time::Duration = std::time::Duration::from_secs(32 * 86_400);
const FILE_CACHE_IDLE: std::time::Duration = std::time::Duration::from_secs(3_600);

fn prune_file_cache(
    cache: &mut HashMap<String, CachedFileEntries>,
    now: Instant,
    wall_now: SystemTime,
) {
    cache.retain(|_, entry| {
        now.duration_since(entry.last_accessed_at) < FILE_CACHE_IDLE
            || !wall_now
                .duration_since(entry.stamp.modified)
                .is_ok_and(|age| age >= OLD_FILE_AGE)
    });
    if cache.len() <= MAX_FILE_CACHE_ENTRIES {
        return;
    }

    let mut oldest_keys: Vec<(String, Instant)> = cache
        .iter()
        .map(|(key, entry)| (key.clone(), entry.last_accessed_at))
        .collect();
    oldest_keys.sort_by_key(|(_, last_accessed_at)| *last_accessed_at);

    for (key, _) in oldest_keys
        .into_iter()
        .take(cache.len().saturating_sub(MAX_FILE_CACHE_ENTRIES))
    {
        cache.remove(&key);
    }
}

const MAX_ROOT_FILE_LIST_CACHE_ENTRIES: usize = 32;

fn prune_root_file_list_cache(cache: &mut HashMap<String, CachedRootFileList>) {
    if cache.len() <= MAX_ROOT_FILE_LIST_CACHE_ENTRIES {
        return;
    }

    let mut oldest_keys: Vec<(String, Instant)> = cache
        .iter()
        .map(|(key, entry)| (key.clone(), entry.last_accessed_at))
        .collect();
    oldest_keys.sort_by_key(|(_, last_accessed_at)| *last_accessed_at);

    for (key, _) in oldest_keys
        .into_iter()
        .take(cache.len().saturating_sub(MAX_ROOT_FILE_LIST_CACHE_ENTRIES))
    {
        cache.remove(&key);
    }
}

fn default_usage_integration_configs() -> Vec<UsageIntegrationConfig> {
    vec![
        UsageIntegrationConfig::new(
            UsageIntegrationId::Claude,
            UsageIntegrationId::Claude.detect_roots(),
        ),
        UsageIntegrationConfig::new(
            UsageIntegrationId::Codex,
            UsageIntegrationId::Codex.detect_roots(),
        ),
        UsageIntegrationConfig::new(
            UsageIntegrationId::Cursor,
            UsageIntegrationId::Cursor.detect_roots(),
        ),
        UsageIntegrationConfig::new(
            UsageIntegrationId::Kimi,
            UsageIntegrationId::Kimi.detect_roots(),
        ),
    ]
}

fn usage_integration_configs_with_overrides(
    claude_roots: Option<Vec<PathBuf>>,
    codex_roots: Option<Vec<PathBuf>>,
    cursor_roots: Option<Vec<PathBuf>>,
) -> Vec<UsageIntegrationConfig> {
    vec![
        UsageIntegrationConfig::new(
            UsageIntegrationId::Claude,
            claude_roots.unwrap_or_else(|| UsageIntegrationId::Claude.detect_roots()),
        ),
        UsageIntegrationConfig::new(
            UsageIntegrationId::Codex,
            codex_roots.unwrap_or_else(|| UsageIntegrationId::Codex.detect_roots()),
        ),
        UsageIntegrationConfig::new(
            UsageIntegrationId::Cursor,
            cursor_roots.unwrap_or_else(|| UsageIntegrationId::Cursor.detect_roots()),
        ),
        UsageIntegrationConfig::new(
            UsageIntegrationId::Kimi,
            UsageIntegrationId::Kimi.detect_roots(),
        ),
    ]
}

impl UsageParser {
    fn from_integrations(integrations: Vec<UsageIntegrationConfig>) -> Self {
        Self {
            integrations,
            cache: Mutex::new(HashMap::new()),
            file_cache: Mutex::new(HashMap::new()),
            root_file_lists: Mutex::new(HashMap::new()),
            last_query_debug: Mutex::new(None),
            archive: Mutex::new(None),
            entries_cache: Mutex::new(HashMap::new()),
            cursor_remote_cache: Mutex::new(None),
            cursor_remote_failure_at: Mutex::new(None),
            cursor_remote_ttl_secs: AtomicU64::new(CACHE_TTL_SECS),
            cursor_remote_generation: AtomicU64::new(0),
            earliest_date_cache: Mutex::new(HashMap::new()),
            file_earliest_dates: Mutex::new(HashMap::new()),
            payload_ttl_secs: AtomicU64::new(CACHE_TTL_SECS),
            listings_frozen: AtomicBool::new(false),
            log_changes: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_payload_ttl_secs(&self, secs: u64) {
        self.payload_ttl_secs.store(secs, Ordering::SeqCst);
    }

    pub fn set_listings_frozen(&self, frozen: bool) {
        self.listings_frozen.store(frozen, Ordering::SeqCst);
    }

    /// Set the archive manager for persistent hourly data storage.
    /// Once set, `load_entries()` merges archived data with live source data.
    pub fn set_archive(&self, archive: super::archive::ArchiveManager) {
        *self.archive.lock().unwrap() = Some(archive);
    }

    /// Access the archive manager (if set).
    pub fn archive(&self) -> Option<super::archive::ArchiveManager> {
        self.archive.lock().unwrap().clone()
    }

    /// [`Self::store_cursor_remote_if_current`] with no generation check.
    #[cfg(test)]
    pub(crate) fn store_cursor_remote(
        &self,
        entries: Vec<ParsedEntry>,
        covered_since: Option<NaiveDate>,
    ) -> bool {
        self.store_cursor_remote_checked(entries, covered_since, None)
            .unwrap_or(false)
    }

    /// Store Cursor remote entries fetched in the background, tagged with the
    /// `covered_since` range the fetch used. The cache merges and never
    /// narrows:
    /// - a refresh the cache already covers replaces only the entries from
    ///   `covered_since` on, keeps the older ones and restarts the TTL;
    /// - a wider fetch adds only the days before the old coverage and keeps
    ///   `stored_at`, so the recent part (today) never moves between refreshes.
    ///
    /// Nothing is stored when the Cursor data changed or was cleared since the
    /// fetch began at `generation`: a clear means the account may have
    /// changed, so a fetch begun before it used the old credentials. The check
    /// runs under the cache lock, where every change bumps the generation.
    ///
    /// Returns `None` for such a superseded fetch, otherwise whether the
    /// cached data changed.
    pub(crate) fn store_cursor_remote_if_current(
        &self,
        entries: Vec<ParsedEntry>,
        covered_since: Option<NaiveDate>,
        generation: u64,
    ) -> Option<bool> {
        self.store_cursor_remote_checked(entries, covered_since, Some(generation))
    }

    fn store_cursor_remote_checked(
        &self,
        entries: Vec<ParsedEntry>,
        covered_since: Option<NaiveDate>,
        generation: Option<u64>,
    ) -> Option<bool> {
        let mut guard = self.cursor_remote_cache.lock().unwrap();
        if generation.is_some_and(|generation| generation != self.cursor_remote_generation()) {
            return None;
        }
        let (cache, changed) = match guard.take() {
            None => (
                CachedCursorRemote {
                    entries,
                    stored_at: Instant::now(),
                    covered_since,
                },
                true,
            ),
            Some(old) if cursor_range_covers(old.covered_since, covered_since) => {
                let (mut merged, replaced): (Vec<_>, Vec<_>) = old
                    .entries
                    .into_iter()
                    .partition(|e| cursor_entry_before(e, covered_since));
                let changed =
                    cursor_entries_fingerprint(&replaced) != cursor_entries_fingerprint(&entries);
                // A refresh that found nothing new waits longer for the next.
                let ttl = self.cursor_remote_ttl_secs.load(Ordering::SeqCst);
                self.cursor_remote_ttl_secs.store(
                    cursor_idle_backoff(ttl, CACHE_TTL_SECS, changed),
                    Ordering::SeqCst,
                );
                merged.extend(entries);
                (
                    CachedCursorRemote {
                        entries: merged,
                        stored_at: Instant::now(),
                        covered_since: old.covered_since,
                    },
                    changed,
                )
            }
            Some(old) => {
                let mut merged: Vec<_> = entries
                    .into_iter()
                    .filter(|e| cursor_entry_before(e, old.covered_since))
                    .collect();
                merged.extend(old.entries);
                // Always a change: views computed while this range was
                // uncovered carry no Cursor data at all, even when the fetch
                // found no older days.
                (
                    CachedCursorRemote {
                        entries: merged,
                        stored_at: old.stored_at,
                        covered_since,
                    },
                    true,
                )
            }
        };
        *guard = Some(cache);
        if changed {
            self.cursor_remote_generation.fetch_add(1, Ordering::SeqCst);
        }
        drop(guard);
        // The endpoint answered, so any earlier failure cooldown is obsolete.
        self.clear_cursor_remote_failure();
        Some(changed)
    }

    /// Drop the Cursor remote cache and any failure cooldown (the Cursor
    /// account may have changed), so the next query fetches afresh.
    pub(crate) fn clear_cursor_remote(&self) {
        let mut guard = self.cursor_remote_cache.lock().unwrap();
        *guard = None;
        self.cursor_remote_generation.fetch_add(1, Ordering::SeqCst);
        drop(guard);
        self.clear_cursor_remote_failure();
        self.reset_cursor_remote_ttl();
    }

    /// Cursor may be in use (the IDE wrote its state, the popover was shown,
    /// or the user asked for a refresh): refresh its remote data on the base
    /// TTL again.
    pub(crate) fn reset_cursor_remote_ttl(&self) {
        self.cursor_remote_ttl_secs
            .store(CACHE_TTL_SECS, Ordering::SeqCst);
    }

    /// Current Cursor remote data generation. Read it before building
    /// anything from the Cursor cache, then pass it to
    /// [`Self::store_cache_at_cursor_generation`].
    pub(crate) fn cursor_remote_generation(&self) -> u64 {
        self.cursor_remote_generation.load(Ordering::SeqCst)
    }

    /// Record that a background Cursor remote fetch failed (API error, bad
    /// payload, or a panicked task).
    ///
    /// A failure stores no entries, so `needs_cursor_remote_fetch` would stay
    /// `true` and the next refresh would respawn the same doomed fetch. This
    /// marker suppresses retries for `CURSOR_REMOTE_FAILURE_COOLDOWN_SECS`,
    /// cleared as soon as a fetch succeeds (or the cache is cleared).
    pub(crate) fn note_cursor_remote_failure(&self) {
        if let Ok(mut guard) = self.cursor_remote_failure_at.lock() {
            *guard = Some(Instant::now());
        }
    }

    /// [`Self::note_cursor_remote_failure`] for a fetch that began at Cursor
    /// `generation`, unless the data was cleared since: the failure was the
    /// old account's, and must not hold back a fetch with the new one.
    /// Returns whether it was noted.
    pub(crate) fn note_cursor_remote_failure_if_current(&self, generation: u64) -> bool {
        // Under the cache lock, where a clear bumps the generation; the clear
        // drops the cooldown after it, so either order leaves none behind.
        let _cache = self.cursor_remote_cache.lock().unwrap();
        if generation != self.cursor_remote_generation() {
            return false;
        }
        self.note_cursor_remote_failure();
        true
    }

    fn clear_cursor_remote_failure(&self) {
        if let Ok(mut guard) = self.cursor_remote_failure_at.lock() {
            *guard = None;
        }
    }

    /// True while a recent fetch failure still suppresses retries.
    pub(crate) fn cursor_remote_failure_cooldown_active(&self) -> bool {
        self.cursor_remote_failure_at
            .lock()
            .ok()
            .and_then(|guard| *guard)
            .is_some_and(|at| at.elapsed().as_secs() < CURSOR_REMOTE_FAILURE_COOLDOWN_SECS)
    }

    /// Non-consuming read of the cursor remote cache for a requested `since`.
    ///
    /// Returns cached entries even when the TTL has expired (stale-while-revalidate)
    /// so tray/UI cost does not drop to `$0` during the refresh window. Freshness
    /// is owned by [`Self::needs_cursor_remote_fetch`], which still triggers a
    /// background refetch after `CACHE_TTL_SECS`. Returns `None` only when there
    /// is no cache or the cache does not cover `req_since`.
    pub(crate) fn cursor_remote_for(
        &self,
        req_since: Option<NaiveDate>,
    ) -> Option<Vec<ParsedEntry>> {
        let guard = self.cursor_remote_cache.lock().unwrap();
        let cache = guard.as_ref()?;
        if !cursor_range_covers(cache.covered_since, req_since) {
            return None;
        }
        let entries = match req_since {
            Some(since) => cache
                .entries
                .iter()
                .filter(|e| e.timestamp.date_naive() >= since)
                .cloned()
                .collect(),
            None => cache.entries.clone(),
        };
        Some(entries)
    }

    #[cfg(test)]
    pub(crate) fn age_cursor_remote_cache_for_test(&self, age: std::time::Duration) {
        let mut guard = self.cursor_remote_cache.lock().unwrap();
        if let Some(cache) = guard.as_mut() {
            if let Some(aged) = Instant::now().checked_sub(age) {
                cache.stored_at = aged;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn age_cursor_remote_failure_for_test(&self, age: std::time::Duration) {
        let mut guard = self.cursor_remote_failure_at.lock().unwrap();
        if let Some(at) = guard.as_mut() {
            if let Some(aged) = Instant::now().checked_sub(age) {
                *at = aged;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn cursor_remote_ttl_expired_for_test(&self) -> bool {
        let ttl = self.cursor_remote_ttl_secs.load(Ordering::SeqCst);
        let guard = self.cursor_remote_cache.lock().unwrap();
        guard
            .as_ref()
            .is_some_and(|cache| cache.stored_at.elapsed().as_secs() >= ttl)
    }

    /// Create with default home-directory paths.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::from_integrations(default_usage_integration_configs())
    }

    /// Create with an explicit Claude projects directory (for testing).
    #[allow(dead_code)]
    pub fn with_claude_dir(claude_dir: PathBuf) -> Self {
        Self::with_claude_dirs(vec![claude_dir])
    }

    /// Create with explicit Claude projects directories (for testing).
    #[allow(dead_code)]
    pub fn with_claude_dirs(claude_dirs: Vec<PathBuf>) -> Self {
        Self::from_integrations(usage_integration_configs_with_overrides(
            Some(claude_dirs),
            None,
            None,
        ))
    }

    /// Create with an explicit Codex sessions directory (for testing).
    #[allow(dead_code)]
    pub fn with_codex_dir(codex_dir: PathBuf) -> Self {
        Self::from_integrations(usage_integration_configs_with_overrides(
            None,
            Some(vec![codex_dir]),
            None,
        ))
    }

    fn integration_config(&self, id: UsageIntegrationId) -> Option<&UsageIntegrationConfig> {
        self.integrations.iter().find(|config| config.id == id)
    }

    /// Return the Codex sessions directory path.
    pub fn codex_dir(&self) -> &Path {
        self.integration_config(UsageIntegrationId::Codex)
            .and_then(|config| config.roots.first())
            .map(PathBuf::as_path)
            .expect("codex integration should always have a primary root")
    }

    /// Create with explicit directories for both providers (for testing).
    #[allow(dead_code)]
    pub fn with_dirs(claude_dir: PathBuf, codex_dir: PathBuf) -> Self {
        Self::from_integrations(usage_integration_configs_with_overrides(
            Some(vec![claude_dir]),
            Some(vec![codex_dir]),
            None,
        ))
    }

    // ── Cache helpers ──

    #[allow(dead_code)]
    pub fn clear_cache(&self) {
        self.clear_payload_cache();
        set_cursor_warning(None);
        // An explicit cache clear is the user's escape hatch (e.g. after fixing
        // Cursor auth) — let the next query retry immediately.
        self.clear_cursor_remote_failure();
        if let Ok(mut c) = self.file_cache.lock() {
            c.clear();
        }
        if let Ok(mut c) = self.earliest_date_cache.lock() {
            c.clear();
        }
        if let Ok(mut c) = self.file_earliest_dates.lock() {
            c.clear();
        }
        if let Ok(mut c) = self.root_file_lists.lock() {
            c.clear();
        }
        if let Ok(mut current) = self.last_query_debug.lock() {
            *current = None;
        }
        if let Ok(guard) = self.archive.lock() {
            if let Some(archive) = guard.as_ref() {
                archive.reset();
            }
        }
        if let Ok(mut c) = self.entries_cache.lock() {
            c.clear();
        }
        self.note_log_change(None);
    }

    pub fn clear_payload_cache(&self) {
        if let Ok(mut c) = self.cache.lock() {
            c.clear();
        }
        self.clear_entries_cache();
    }

    /// [`Self::clear_payload_cache`] of only the payloads `stale` picks by key.
    pub(crate) fn clear_payload_cache_where(&self, stale: impl Fn(&str) -> bool) {
        if let Ok(mut c) = self.cache.lock() {
            c.retain(|key, _| !stale(key));
        }
        self.clear_entries_cache();
    }

    pub(crate) fn load_entries_cached(
        &self,
        provider: &str,
        since: Option<NaiveDate>,
    ) -> Arc<LoadedEntries> {
        self.entries_cached(provider, since, true)
    }

    /// [`Self::load_entries_cached`] from the logs alone, without the
    /// archive: its hourly aggregates are stamped at the top of their hour,
    /// which loses when in the hour each request was made. For callers that
    /// need that and look back no further than the logs are kept.
    pub(crate) fn load_live_entries_cached(
        &self,
        provider: &str,
        since: Option<NaiveDate>,
    ) -> Arc<LoadedEntries> {
        self.entries_cached(provider, since, false)
    }

    fn entries_cached(
        &self,
        provider: &str,
        since: Option<NaiveDate>,
        with_archive: bool,
    ) -> Arc<LoadedEntries> {
        // Note: do NOT call have_sources_changed() here — it stats all files
        // and defeats the warm-path optimization. The refresh's sweep clears
        // entries_cache when sources change.
        let Some(selection) = UsageIntegrationSelection::parse(provider) else {
            return Arc::default();
        };
        self.cached_load(entries_cache_key(provider, since, with_archive), || {
            // Each integration's load is cached apart, before the tab's model
            // filter, so an `all` view reuses its per-provider charts' loads.
            let parts: Vec<Arc<LoadedEntries>> = selection
                .integration_ids()
                .into_iter()
                .map(|id| {
                    let key =
                        entries_cache_key(&format!("raw:{}", id.as_str()), since, with_archive);
                    self.cached_load(key, || {
                        Arc::new(self.load_integration(id, since, with_archive))
                    })
                })
                .collect();
            let keep = |entry: &ParsedEntry| provider_matches_model(provider, &entry.model);
            match parts.as_slice() {
                [part] if part.entries.iter().all(keep) => part.clone(),
                _ => Arc::new(LoadedEntries {
                    entries: parts
                        .iter()
                        .flat_map(|part| part.entries.iter().filter(|e| keep(e)).cloned())
                        .collect(),
                    change_events: parts
                        .iter()
                        .flat_map(|part| part.change_events.iter().cloned())
                        .collect(),
                    reports: parts
                        .iter()
                        .flat_map(|part| part.reports.iter().cloned())
                        .collect(),
                }),
            }
        })
    }

    /// The `entries_cache` entry at `key`, from `load` on a miss. Not stored
    /// when the Cursor remote data changed mid-load: the snapshot is
    /// superseded, and a later compute that trusts the new generation must
    /// not pick it up.
    fn cached_load(
        &self,
        key: String,
        load: impl FnOnce() -> Arc<LoadedEntries>,
    ) -> Arc<LoadedEntries> {
        if let Some((_stored_at, cached)) = self.entries_cache.lock().unwrap().get(&key) {
            return cached.clone();
        }
        let cursor_generation = self.cursor_remote_generation();
        let loaded = load();
        let mut cache = self.entries_cache.lock().unwrap();
        if self.cursor_remote_generation() == cursor_generation {
            cache.insert(key, (Instant::now(), loaded.clone()));
        }
        loaded
    }

    pub(crate) fn clear_entries_cache(&self) {
        if let Ok(mut c) = self.entries_cache.lock() {
            c.clear();
        }
    }
    pub fn clear_payload_cache_prefix(&self, prefix: &str) {
        if let Ok(mut c) = self.cache.lock() {
            c.retain(|key, _| !key.starts_with(prefix));
        }
    }

    /// One sweep over the cached listings, root by root (see [`sweep_root`]).
    /// Roots are swept off the lock; only the sample sweeps, under the
    /// compute gate, so no query lists a root meanwhile.
    fn scan_source_changes(&self, full: bool) -> SourceChangeScan {
        let listed: Vec<(String, CachedRootFileList)> = match self.root_file_lists.lock() {
            Ok(cache) => cache.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            // Poisoned lock: be conservative and force a full rescan.
            Err(_) => Vec::new(),
        };
        if listed.is_empty() {
            return SourceChangeScan {
                listing_changed: true,
                ..SourceChangeScan::default()
            };
        }

        let mut scan = SourceChangeScan::default();
        for (cache_key, entry) in listed {
            let swept = self
                .listed_root(&cache_key)
                .and_then(|(config, root)| sweep_root(root, &entry, config.file_kind(), full));
            match swept {
                None => scan.dropped.push(cache_key),
                Some(None) => {}
                Some(Some(root)) => scan.roots.push(RootSweep { cache_key, ..root }),
            }
        }
        scan
    }

    /// A full [`Self::sweep`] that drops every payload when anything changed.
    /// Returns whether anything did.
    pub fn invalidate_if_changed(&self) -> bool {
        if self.sweep(true) == LogChanges::None {
            return false;
        }
        self.clear_payload_cache();
        true
    }

    /// Sweep the logs (every file when `full`, see [`sweep_root`]) and take
    /// in what moved: the listings as found, and the changed files dropped
    /// from the file cache, so only they re-parse. The payloads are the
    /// caller's to drop, by what this returns. A file that came clears the
    /// earliest dates too (it may hold older entries); their recompute parses
    /// only the files the per-file memo lacks. An append cannot lower them.
    pub(crate) fn sweep(&self, full: bool) -> LogChanges {
        let scan = self.scan_source_changes(full);
        if scan.listing_changed {
            // The next query lists every root.
            if let Ok(mut c) = self.root_file_lists.lock() {
                c.clear();
            }
            if let Ok(mut c) = self.earliest_date_cache.lock() {
                c.clear();
            }
            self.note_log_change(None);
            self.clear_entries_cache();
            return LogChanges::Any;
        }

        let mut set_changed = !scan.dropped.is_empty();
        let mut any = set_changed;
        let mut appended = Vec::new();
        let mut since: Option<SystemTime> = None;
        let mut stale_files = Vec::new();
        let mut listings = Vec::new();
        for key in &scan.dropped {
            self.note_log_change(Some(key));
        }
        for root in scan.roots {
            if root.set_changed || !root.changed_keys.is_empty() {
                self.note_log_change(Some(&root.cache_key));
            }
            if !root.changed_keys.is_empty() {
                match self.listed_root(&root.cache_key) {
                    Some((config, _))
                        if config.id != UsageIntegrationId::Cursor && !root.shrunk =>
                    {
                        if !appended.contains(&config.id) {
                            appended.push(config.id);
                        }
                        since = since.into_iter().chain(root.changed_since).min();
                    }
                    _ => any = true,
                }
            }
            set_changed |= root.set_changed;
            any |= root.set_changed;
            stale_files.extend(root.changed_keys);
            listings.push((root.cache_key, root.directories, root.file_stamps));
        }

        if let Ok(mut lists) = self.root_file_lists.lock() {
            for key in &scan.dropped {
                lists.remove(key);
            }
            for (key, directories, file_stamps) in listings {
                if let Some(entry) = lists.get_mut(&key) {
                    entry.files = file_stamps.iter().map(|f| f.path.clone()).collect();
                    entry.directories = directories.into();
                    entry.file_stamps = file_stamps.into();
                }
            }
        }
        if let Ok(mut fc) = self.file_cache.lock() {
            for key in &stale_files {
                fc.remove(key);
            }
        }
        if set_changed {
            if let Ok(mut c) = self.earliest_date_cache.lock() {
                c.clear();
            }
        }
        let changes = match since {
            _ if any => LogChanges::Any,
            // No later than now: a clock that went back left a later stamp.
            Some(since) => LogChanges::Appended(LogAppends {
                integrations: appended,
                since: (DateTime::<Local>::from(since.min(SystemTime::now())) - APPEND_DATE_SLACK)
                    .date_naive(),
            }),
            None => return LogChanges::None,
        };
        self.clear_entries_cache();
        changes
    }

    /// A count that moves whenever `provider`'s entries may have: at each
    /// sweep that found its logs changed, and for Cursor at each change of
    /// its remote data. The same count means the same entries.
    pub(crate) fn data_version(&self, provider: &str) -> u64 {
        let Some(selection) = UsageIntegrationSelection::parse(provider) else {
            return 0;
        };
        let changes = self.log_changes.lock().unwrap_or_else(|p| p.into_inner());
        selection
            .integration_ids()
            .into_iter()
            .map(|id| {
                let remote = if id == UsageIntegrationId::Cursor {
                    self.cursor_remote_generation()
                } else {
                    0
                };
                changes.get(&id).copied().unwrap_or(0) + remote
            })
            .sum()
    }

    /// Count a change to the logs under the root listed as `root_key`, or
    /// under every root for `None`.
    fn note_log_change(&self, root_key: Option<&str>) {
        let owner = root_key
            .and_then(|key| self.listed_root(key))
            .map(|(config, _)| config.id);
        let mut changes = self.log_changes.lock().unwrap_or_else(|p| p.into_inner());
        for config in &self.integrations {
            if owner.is_none_or(|id| id == config.id) {
                *changes.entry(config.id).or_default() += 1;
            }
        }
    }

    /// The integration whose root is listed as `root_key`, and that root.
    fn listed_root(&self, root_key: &str) -> Option<(&UsageIntegrationConfig, &Path)> {
        self.integrations.iter().find_map(|config| {
            config
                .roots
                .iter()
                .find(|root| path_to_string(root) == root_key)
                .map(|root| (config, root.as_path()))
        })
    }

    pub fn check_cache(&self, key: &str) -> Option<UsagePayload> {
        let mut payload = self.check_cache_as_stored(key)?;
        payload.from_cache = true;
        Some(payload)
    }

    /// [`Self::check_cache`] that returns the stored `from_cache` unchanged,
    /// so only a copy restored from disk reads as cached.
    pub fn check_cache_as_stored(&self, key: &str) -> Option<UsagePayload> {
        let mut c = self.cache.lock().ok()?;
        prune_payload_cache(&mut c, self.payload_ttl_secs.load(Ordering::SeqCst));

        let entry = c.get_mut(key)?;
        entry.last_accessed_at = Instant::now();
        Some(entry.payload.clone())
    }

    pub fn store_cache(&self, key: &str, payload: UsagePayload) {
        if let Ok(mut c) = self.cache.lock() {
            let ttl_secs = self.payload_ttl_secs.load(Ordering::SeqCst);
            insert_payload_cache_entry(&mut c, key, payload, ttl_secs);
        }
    }

    /// [`Self::store_cache`] unless the Cursor remote data changed since
    /// `cursor_generation` was read, in which case the payload was built from
    /// a superseded snapshot and is dropped (returns `false`). The check runs
    /// under the cache lock, and a change bumps the generation before its
    /// completion clears this cache, so no stale payload can outlive it.
    pub(crate) fn store_cache_at_cursor_generation(
        &self,
        key: &str,
        payload: UsagePayload,
        cursor_generation: u64,
    ) -> bool {
        let Ok(mut c) = self.cache.lock() else {
            return false;
        };
        if self.cursor_remote_generation() != cursor_generation {
            return false;
        }
        let ttl_secs = self.payload_ttl_secs.load(Ordering::SeqCst);
        insert_payload_cache_entry(&mut c, key, payload, ttl_secs);
        true
    }

    fn set_last_query_debug(&self, report: UsageQueryDebugReport) {
        if let Ok(mut current) = self.last_query_debug.lock() {
            *current = Some(report);
        }
    }

    pub fn last_query_debug(&self) -> Option<UsageQueryDebugReport> {
        self.last_query_debug.lock().ok()?.clone()
    }

    fn root_listing_is_fresh(entry: &CachedRootFileList) -> bool {
        if entry.directories.is_empty() {
            return false;
        }

        let directories_unchanged = entry.directories.iter().all(|directory| {
            fs::metadata(&directory.path)
                .and_then(|metadata| metadata.modified())
                .map(|modified| modified == directory.modified)
                .unwrap_or(false)
        });
        if !directories_unchanged {
            return false;
        }

        // Directory mtime changes whenever files are added/removed/renamed inside it.
        // If all directory mtimes match, the file listing is still valid — no need
        // to re-stat every individual file (which costs ~0.4ms × 14K files on Windows).
        true
    }

    #[allow(clippy::type_complexity)]
    fn cached_jsonl_files(
        &self,
        dir: &Path,
        kind: ProviderFileKind,
    ) -> (Arc<[PathBuf]>, Option<Arc<[FileListStamp]>>, bool) {
        if !dir.exists() {
            return (Arc::from(Vec::<PathBuf>::new()), None, false);
        }

        let cache_key = path_to_string(dir);
        if let Ok(mut cache) = self.root_file_lists.lock() {
            if let Some(entry) = cache.get_mut(&cache_key) {
                // Frozen, a listing is revalidated only by the sweep: a re-walk
                // here would absorb a new file, and the sweep would then find
                // no change to report.
                if self.listings_frozen.load(Ordering::SeqCst) || Self::root_listing_is_fresh(entry)
                {
                    entry.last_accessed_at = Instant::now();
                    return (entry.files.clone(), Some(entry.file_stamps.clone()), true);
                }
                cache.remove(&cache_key);
            }
        }

        let (files, directories) = scan_jsonl_tree(dir, kind);
        let file_stamps: Vec<FileListStamp> = files
            .iter()
            .filter_map(|path| {
                file_stamp(path).map(|stamp| FileListStamp {
                    path: path.clone(),
                    stamp,
                })
            })
            .collect();
        let files: Arc<[PathBuf]> = files.into();
        let directories: Arc<[DirectoryStamp]> = directories.into();
        let file_stamps: Arc<[FileListStamp]> = file_stamps.into();

        if !directories.is_empty() {
            if let Ok(mut cache) = self.root_file_lists.lock() {
                let now = Instant::now();
                cache.insert(
                    cache_key,
                    CachedRootFileList {
                        files: files.clone(),
                        directories,
                        file_stamps: file_stamps.clone(),
                        last_accessed_at: now,
                    },
                );
                prune_root_file_list_cache(&mut cache);
            }
        }

        (files, Some(file_stamps), false)
    }

    fn load_integration_entries_with_debug(
        &self,
        config: &UsageIntegrationConfig,
        since: Option<NaiveDate>,
    ) -> (
        Vec<ParsedEntry>,
        Vec<ParsedChangeEvent>,
        Vec<ProviderReadDebug>,
    ) {
        let mut entries = Vec::new();
        let mut change_events = Vec::new();
        let mut reports = Vec::new();
        let mut entry_report_indices = Vec::new();
        let mut processed_hashes = HashMap::new();
        let mut processed_change_keys = HashMap::new();
        let kind = config.file_kind();
        let _prof_t0 = std::time::Instant::now();

        for root_dir in &config.roots {
            let _t_scan = std::time::Instant::now();
            let (files, cached_stamps, listing_cache_hit) = self.cached_jsonl_files(root_dir, kind);
            reports.push(ProviderReadDebug {
                provider: String::from(config.id.as_str()),
                root_dir: path_to_string(root_dir),
                root_exists: root_dir.exists(),
                since: since.map(|date| date.format("%Y-%m-%d").to_string()),
                strategy: String::from(config.scan_strategy()),
                listing_cache_hit,
                discovered_paths: files.len(),
                ..ProviderReadDebug::default()
            });
            let report_idx = reports.len() - 1;

            // Phase 1: Build mtime-filter stamps from cache (fast, no stat) and
            // prepare to get fresh stamps only for files that pass the filter.
            let cached_stamp_map: Option<HashMap<&Path, &FileStamp>> = cached_stamps
                .as_ref()
                .map(|cs| cs.iter().map(|fs| (fs.path.as_path(), &fs.stamp)).collect());

            // Phase 2a: Mtime filter using cached stamps (zero stat cost).
            let mut candidate_indices: Vec<usize> = Vec::new();
            for (i, path) in files.iter().enumerate() {
                if let Some(since_date) = since {
                    let cached_stamp = cached_stamp_map
                        .as_ref()
                        .and_then(|m| m.get(path.as_path()).copied());
                    let dominated = cached_stamp.is_some_and(|s| {
                        let dt: DateTime<Local> = s.modified.into();
                        dt.date_naive() < since_date
                    });
                    if dominated {
                        let report = &mut reports[report_idx];
                        report.skipped_paths += 1;
                        report.skipped_by_mtime += 1;
                        push_sample_path(&mut report.sample_skipped_paths, path);
                        continue;
                    }
                }
                candidate_indices.push(i);
            }
            tracing::debug!(
                "[PROFILE] {}: Phase1+2a scan+mtime_filter={:?} files={} candidates={} listing_cache={}",
                config.id.as_str(),
                _t_scan.elapsed(),
                files.len(),
                candidate_indices.len(),
                listing_cache_hit,
            );

            // Phase 2b: Classify into cache-hit vs needs-parse (single lock).
            // When listing_cache_hit is true AND file_cache has the entry at the
            // listed stamp, trust it without a fresh stat: the sweep stats files
            // and drops the entries of those that changed. One at another stamp
            // (its path left the listing and came back) is stat'ed below.
            let mut cache_hits: Vec<CachedFileLoad> = Vec::new();
            let mut to_parse: Vec<(usize, PathBuf, Option<FileStamp>)> = Vec::new();
            let mut needs_stat_indices: Vec<usize> = Vec::new();
            let now = Instant::now();
            {
                let mut cache = self.file_cache.lock().unwrap();
                for &i in &candidate_indices {
                    let path = &files[i];
                    reports[report_idx].attempted_paths += 1;
                    push_sample_path(&mut reports[report_idx].sample_paths, path);

                    let cache_key = path_to_string(path);
                    if listing_cache_hit {
                        let listed = cached_stamp_map
                            .as_ref()
                            .and_then(|m| m.get(path.as_path()).copied());
                        let cached = cache.get_mut(&cache_key);
                        if let Some(cached) = cached.filter(|c| Some(&c.stamp) == listed) {
                            cached.last_accessed_at = now;
                            reports[report_idx].cache_hits += 1;
                            cache_hits.push(CachedFileLoad {
                                entries: cached.entries.clone(),
                                change_events: cached.change_events.clone(),
                                earliest_date: cached.earliest_date,
                                lines_read: 0,
                                opened: false,
                                from_cache: true,
                            });
                            continue;
                        }
                    }
                    needs_stat_indices.push(i);
                }
            }
            tracing::debug!(
                "[PROFILE] {}: Phase2b classify elapsed={:?} cache_hits={} needs_stat={}",
                config.id.as_str(),
                _t_scan.elapsed(),
                cache_hits.len(),
                needs_stat_indices.len(),
            );

            // Phase 2c: Parallel stat only files not found in file_cache.
            let fresh_stamps: Vec<(usize, Option<FileStamp>)> = needs_stat_indices
                .par_iter()
                .map(|&i| (i, file_stamp(&files[i])))
                .collect();
            {
                let mut cache = self.file_cache.lock().unwrap();
                for (i, stamp) in &fresh_stamps {
                    let path = &files[*i];
                    let cache_key = path_to_string(path);
                    let hit = stamp.as_ref().and_then(|s| {
                        cache.get_mut(&cache_key).and_then(|cached| {
                            if &cached.stamp == s {
                                cached.last_accessed_at = now;
                                Some(CachedFileLoad {
                                    entries: cached.entries.clone(),
                                    change_events: cached.change_events.clone(),
                                    earliest_date: cached.earliest_date,
                                    lines_read: 0,
                                    opened: false,
                                    from_cache: true,
                                })
                            } else {
                                None
                            }
                        })
                    });

                    match hit {
                        Some(loaded) => {
                            reports[report_idx].cache_hits += 1;
                            cache_hits.push(loaded);
                        }
                        None => {
                            to_parse.push((*i, path.clone(), stamp.clone()));
                        }
                    }
                }
            }
            tracing::debug!(
                "[PROFILE] {}: Phase2c parallel_stat elapsed={:?} to_parse={}",
                config.id.as_str(),
                _t_scan.elapsed(),
                to_parse.len(),
            );

            // Phase 3: Parallel parse of cache-miss files.
            let parsed: Vec<(PathBuf, Option<FileStamp>, CachedFileLoad)> = to_parse
                .par_iter()
                .map(|(_i, path, stamp)| {
                    let (raw_entries, raw_change_events, lines_read, opened) = kind.parse(path);
                    let earliest_date = earliest_entry_date(&raw_entries);
                    let loaded = CachedFileLoad {
                        entries: raw_entries.into(),
                        change_events: raw_change_events.into(),
                        earliest_date,
                        lines_read,
                        opened,
                        from_cache: false,
                    };
                    (path.clone(), stamp.clone(), loaded)
                })
                .collect();
            tracing::debug!(
                "[PROFILE] {}: Phase3 parallel_parse elapsed={:?} parsed_files={}",
                config.id.as_str(),
                _t_scan.elapsed(),
                parsed.len(),
            );

            // Phase 4: Batch update file_cache (single lock).
            {
                let mut cache = self.file_cache.lock().unwrap();
                for (path, stamp, loaded) in &parsed {
                    let cache_key = path_to_string(path);
                    if loaded.opened {
                        if let Some(stamp) = stamp {
                            cache.insert(
                                cache_key,
                                CachedFileEntries {
                                    stamp: stamp.clone(),
                                    entries: loaded.entries.clone(),
                                    change_events: loaded.change_events.clone(),
                                    earliest_date: loaded.earliest_date,
                                    last_accessed_at: now,
                                },
                            );
                        } else {
                            cache.remove(&cache_key);
                        }
                    } else {
                        cache.remove(&cache_key);
                    }
                }
                prune_file_cache(&mut cache, now, SystemTime::now());
            }

            // If files were re-parsed, entries_cache is stale.
            if !parsed.is_empty() {
                self.clear_entries_cache();
            }

            // Phase 5: Update parse reports.
            for (_path, _stamp, loaded) in &parsed {
                let report = &mut reports[report_idx];
                report.lines_read += loaded.lines_read;
                if loaded.from_cache {
                    report.cache_hits += 1;
                } else {
                    report.cache_misses += 1;
                    if loaded.opened {
                        report.opened_paths += 1;
                    } else {
                        report.failed_paths += 1;
                    }
                }
            }

            // Phase 6: Merge entries + dedup (sequential, CPU-bound).
            let all_loaded = cache_hits
                .iter()
                .chain(parsed.iter().map(|(_, _, loaded)| loaded));

            for loaded in all_loaded {
                if !loaded.opened && !loaded.from_cache {
                    continue;
                }

                for cev in loaded.change_events.iter() {
                    if since.is_some_and(|since_date| cev.timestamp.date_naive() < since_date) {
                        continue;
                    }
                    if config.dedupe_change_events() {
                        let _ = upsert_claude_change_event(
                            &mut change_events,
                            &mut processed_change_keys,
                            cev.clone(),
                        );
                        continue;
                    }
                    change_events.push(cev.clone());
                }

                for entry in loaded.entries.iter() {
                    if since.is_some_and(|since_date| entry.timestamp.date_naive() < since_date) {
                        continue;
                    }
                    if config.dedupe_entry_hashes() {
                        match upsert_claude_entry(
                            &mut entries,
                            &mut processed_hashes,
                            entry.clone(),
                        ) {
                            ClaudeDedupeAction::Inserted => {
                                entry_report_indices.push(report_idx);
                                reports[report_idx].emitted_entries += 1;
                            }
                            ClaudeDedupeAction::Replaced(existing_idx) => {
                                let old_report_idx = entry_report_indices
                                    .get(existing_idx)
                                    .copied()
                                    .expect("existing deduped entry should track its origin");
                                if old_report_idx != report_idx {
                                    let previous_count =
                                        reports[old_report_idx].emitted_entries.saturating_sub(1);
                                    reports[old_report_idx].emitted_entries = previous_count;
                                    reports[report_idx].emitted_entries += 1;
                                }
                                entry_report_indices[existing_idx] = report_idx;
                            }
                            ClaudeDedupeAction::Skipped => {}
                        }
                        continue;
                    }
                    reports[report_idx].emitted_entries += 1;
                    entries.push(entry.clone());
                }
            }
        }
        tracing::debug!(
            "[PROFILE] {}: TOTAL={:?} entries={} change_events={}",
            config.id.as_str(),
            _prof_t0.elapsed(),
            entries.len(),
            change_events.len(),
        );

        (entries, change_events, reports)
    }

    fn load_claude_entries_with_debug(
        &self,
        since: Option<NaiveDate>,
    ) -> (
        Vec<ParsedEntry>,
        Vec<ParsedChangeEvent>,
        Vec<ProviderReadDebug>,
    ) {
        let config = self
            .integration_config(UsageIntegrationId::Claude)
            .expect("claude integration should be configured");
        self.load_integration_entries_with_debug(config, since)
    }

    fn load_codex_entries_with_debug(
        &self,
        since: Option<NaiveDate>,
    ) -> (Vec<ParsedEntry>, Vec<ParsedChangeEvent>, ProviderReadDebug) {
        let config = self
            .integration_config(UsageIntegrationId::Codex)
            .expect("codex integration should be configured");
        let (entries, change_events, mut reports) =
            self.load_integration_entries_with_debug(config, since);
        let report = reports.pop().unwrap_or_default();
        (entries, change_events, report)
    }

    fn load_kimi_entries_with_debug(
        &self,
        since: Option<NaiveDate>,
    ) -> (Vec<ParsedEntry>, Vec<ParsedChangeEvent>, ProviderReadDebug) {
        let config = self
            .integration_config(UsageIntegrationId::Kimi)
            .expect("kimi integration should be configured");
        let (entries, change_events, mut reports) =
            self.load_integration_entries_with_debug(config, since);
        let report = reports.pop().unwrap_or_default();
        (entries, change_events, report)
    }

    fn load_cursor_local_entries_with_debug(
        &self,
        since: Option<NaiveDate>,
    ) -> (Vec<ParsedEntry>, ProviderReadDebug) {
        let config = self
            .integration_config(UsageIntegrationId::Cursor)
            .expect("cursor integration should be configured");
        // The chat listing and each file's parse (even one with no usage)
        // are cached like the other providers' logs, and revalidated only by
        // the sweep.
        let (mut entries, _change_events, reports) =
            self.load_integration_entries_with_debug(config, since);
        entries.sort_by_key(|entry| entry.timestamp);
        (entries, reports.into_iter().next().unwrap_or_default())
    }

    fn load_cursor_entries_with_debug(
        &self,
        since: Option<NaiveDate>,
    ) -> (Vec<ParsedEntry>, Vec<ParsedChangeEvent>, ProviderReadDebug) {
        let (local_entries, mut report) = self.load_cursor_local_entries_with_debug(since);
        if !local_entries.is_empty() {
            set_cursor_warning(None);
            return (local_entries, Vec::new(), report);
        }

        // Serve from the non-consuming, range-tagged remote cache when it covers
        // the requested range (entries are filtered to `since`).
        if let Some(entries) = self.cursor_remote_for(since) {
            report.strategy = format!("{}+cursor-remote-cache", report.strategy);
            report.emitted_entries = entries.len();
            set_cursor_warning(None);
            return (entries, Vec::new(), report);
        }

        // No local entries and no cache — signal that async fetch is needed.
        // The caller (usage_query) will spawn a background task.
        report.strategy = format!("{}+cursor-remote-pending", report.strategy);
        (Vec::new(), Vec::new(), report)
    }

    /// Returns `true` when Cursor remote auth is configured and the cache is
    /// missing, past its TTL, or does not cover the requested `since` range —
    /// the periodic (tray) refresh check. Usage views use
    /// [`Self::cursor_remote_uncovered`], which ignores the TTL.
    ///
    /// Returns `false` during the cooldown that follows a failed fetch: a
    /// failure caches nothing, so without the cooldown every refresh triggered
    /// by the previous failure would spawn the next one.
    pub(crate) fn needs_cursor_remote_fetch(&self, req_since: Option<NaiveDate>) -> bool {
        use super::cursor_parser::resolve_cursor_auth;
        if resolve_cursor_auth().is_none() {
            return false;
        }
        if self.cursor_remote_failure_cooldown_active() {
            return false;
        }
        let ttl = self.cursor_remote_ttl_secs.load(Ordering::SeqCst);
        let guard = self.cursor_remote_cache.lock().unwrap();
        match guard.as_ref() {
            None => true,
            Some(cache) => {
                cache.stored_at.elapsed().as_secs() >= ttl
                    || !cursor_range_covers(cache.covered_since, req_since)
            }
        }
    }

    /// Returns `true` when Cursor remote auth is configured, no failure
    /// cooldown is active, and the cache does not cover `req_since` — a view
    /// needs a widening fetch (only the part we lack). There is no TTL check:
    /// an expired cache that still covers the range is served as-is, since
    /// keeping it fresh is the periodic refresh's job.
    pub(crate) fn cursor_remote_uncovered(&self, req_since: Option<NaiveDate>) -> bool {
        use super::cursor_parser::resolve_cursor_auth;
        resolve_cursor_auth().is_some()
            && !self.cursor_remote_failure_cooldown_active()
            && self.cursor_remote_cache_uncovered(req_since)
    }

    /// The first day the Cursor remote cache covers, when a fetch since
    /// `req_since` would widen it: add only the days before that one. `None`
    /// when it holds nothing, or covers `req_since` already.
    pub(crate) fn cursor_widening_from(&self, req_since: Option<NaiveDate>) -> Option<NaiveDate> {
        let guard = self.cursor_remote_cache.lock().unwrap();
        let covered = guard.as_ref()?.covered_since?;
        (!cursor_range_covers(Some(covered), req_since)).then_some(covered)
    }

    /// True when there is no cache or it does not cover `req_since`, however
    /// old it is.
    fn cursor_remote_cache_uncovered(&self, req_since: Option<NaiveDate>) -> bool {
        let guard = self.cursor_remote_cache.lock().unwrap();
        guard
            .as_ref()
            .is_none_or(|cache| !cursor_range_covers(cache.covered_since, req_since))
    }

    // ── Internal: load entries for a provider/since combination ──

    pub(crate) fn load_entries(
        &self,
        provider: &str,
        since: Option<NaiveDate>,
    ) -> (
        Vec<ParsedEntry>,
        Vec<ParsedChangeEvent>,
        Vec<ProviderReadDebug>,
    ) {
        self.load_entries_from(provider, since, true)
    }

    /// [`Self::load_entries`], with the archive's completed hours in place
    /// of the logs' when `with_archive`, else from the logs alone.
    fn load_entries_from(
        &self,
        provider: &str,
        since: Option<NaiveDate>,
        with_archive: bool,
    ) -> (
        Vec<ParsedEntry>,
        Vec<ParsedChangeEvent>,
        Vec<ProviderReadDebug>,
    ) {
        let Some(selection) = UsageIntegrationSelection::parse(provider) else {
            return (Vec::new(), Vec::new(), Vec::new());
        };

        let mut entries = Vec::new();
        let mut change_events = Vec::new();
        let mut reports = Vec::new();
        for integration_id in selection.integration_ids() {
            let part = self.load_integration(integration_id, since, with_archive);
            entries.extend(part.entries);
            change_events.extend(part.change_events);
            reports.extend(part.reports);
        }

        // Drop rows whose model doesn't belong to the selected provider tab.
        // A third-party model logged through any CLI (e.g. GLM-5 via a Claude
        // Code proxy) should not show up in the Claude tab; otherwise the
        // main dashboard total diverges from the Per-Device breakdown, which
        // applies the same predicate to remote SSH rows.
        entries.retain(|e| provider_matches_model(provider, &e.model));

        (entries, change_events, reports)
    }

    /// One integration's entries, before any provider tab's model filter.
    fn load_integration(
        &self,
        integration_id: UsageIntegrationId,
        since: Option<NaiveDate>,
        with_archive: bool,
    ) -> LoadedEntries {
        let archive_guard = self.archive.lock().unwrap();
        let archive = archive_guard.as_ref().filter(|_| with_archive);
        let source_key = format!("local:{}", integration_id.as_str());
        let frontier = archive.and_then(|a| a.frontier(&source_key));

        // Load archived entries for completed hours (up to frontier).
        let archived = if let (Some(a), Some(_frontier)) = (archive, frontier) {
            a.load_archived(&source_key, since)
        } else {
            Vec::new()
        };

        // Load live entries from source JSONL files.
        let (live, change_events, reports) = match integration_id {
            UsageIntegrationId::Claude => self.load_claude_entries_with_debug(since),
            UsageIntegrationId::Codex => {
                let (entries, change_events, report) = self.load_codex_entries_with_debug(since);
                (entries, change_events, vec![report])
            }
            UsageIntegrationId::Cursor => {
                let (entries, change_events, report) = self.load_cursor_entries_with_debug(since);
                (entries, change_events, vec![report])
            }
            UsageIntegrationId::Kimi => {
                let (entries, change_events, report) = self.load_kimi_entries_with_debug(since);
                (entries, change_events, vec![report])
            }
        };

        let mut entries = Vec::new();
        merge_archived_and_live_entries(&mut entries, archived, live, frontier);
        LoadedEntries {
            entries,
            change_events,
            reports,
        }
    }

    // ── has_entries_before: check if data exists before a given date ──

    pub fn has_entries_before(&self, provider: &str, before_date: NaiveDate) -> bool {
        self.provider_earliest_date(provider)
            .is_some_and(|earliest| earliest < before_date)
    }

    /// Earliest entry date across all of a provider's data, cached per epoch.
    ///
    /// The first call scans the provider's session files once (parsing
    /// uncached ones in parallel); every later call — including each period
    /// switch and the background warmup's per-offset probing — is O(1). The
    /// cache is cleared by `clear_cache` and a sweep that finds files came or
    /// went.
    fn provider_earliest_date(&self, provider: &str) -> Option<NaiveDate> {
        if let Ok(cache) = self.earliest_date_cache.lock() {
            if let Some(cached) = cache.get(provider) {
                return *cached;
            }
        }
        let computed = self.compute_provider_earliest_date(provider);
        if let Ok(mut cache) = self.earliest_date_cache.lock() {
            cache.insert(provider.to_string(), computed);
        }
        computed
    }

    fn compute_provider_earliest_date(&self, provider: &str) -> Option<NaiveDate> {
        let selection = UsageIntegrationSelection::parse(provider)?;
        selection
            .integration_ids()
            .iter()
            .copied()
            .filter_map(|integration_id| {
                self.integration_config(integration_id)
                    .and_then(|config| self.integration_earliest_date(config))
            })
            .min()
    }

    /// Minimum earliest-entry-date across a provider's session files.
    ///
    /// Parses files directly in parallel and keeps only each file's earliest
    /// date — it deliberately does NOT go through `load_cached_file`. With many
    /// thousands of session files, inserting each into the `MAX_FILE_CACHE_ENTRIES`
    /// (4096) capped `file_cache` would call `prune_file_cache` on every insert
    /// past the cap (clone-all-keys + O(n log n) sort, under one Mutex shared by
    /// the rayon workers) — that eviction churn, not parsing, was the multi-second
    /// cost here.
    ///
    /// Each file's date is memoized with the stamp it was read at, and only a
    /// file that is new, shrank, or held no entry is parsed again: an append
    /// cannot lower a file's earliest date. A file that left the listing
    /// leaves the memo, and with it the minimum.
    fn integration_earliest_date(&self, config: &UsageIntegrationConfig) -> Option<NaiveDate> {
        let kind = config.file_kind();
        config
            .roots
            .iter()
            .filter_map(|root_dir| {
                let (_, stamps, _) = self.cached_jsonl_files(root_dir, kind);
                let root_key = path_to_string(root_dir);
                let known = self
                    .file_earliest_dates
                    .lock()
                    .ok()
                    .and_then(|mut memo| memo.remove(&root_key))
                    .unwrap_or_default();
                let dates: FileEarliestDates = stamps?
                    .par_iter()
                    .map(|file| {
                        let date = match known.get(&file.path) {
                            Some((read_at, Some(date))) if read_at.len <= file.stamp.len => {
                                Some(*date)
                            }
                            Some((read_at, None)) if *read_at == file.stamp => None,
                            _ => earliest_entry_date(&kind.parse(&file.path).0),
                        };
                        (file.path.clone(), (file.stamp.clone(), date))
                    })
                    .collect();
                let earliest = dates.values().filter_map(|(_, date)| *date).min();
                if let Ok(mut memo) = self.file_earliest_dates.lock() {
                    memo.insert(root_key, dates);
                }
                earliest
            })
            .min()
    }

    // ── Internal: build model_breakdown across all entries ──

    #[allow(dead_code)]
    fn build_model_breakdown(entries: &[ParsedEntry]) -> Vec<ModelSummary> {
        let refs: Vec<&ParsedEntry> = entries.iter().collect();
        let map = build_segment_map(&refs);
        segment_map_to_model_summaries(&map)
    }

    fn provider_usage_warning(provider: &str) -> Option<String> {
        if provider == UsageIntegrationId::Cursor.as_str() {
            cursor_last_warning()
        } else {
            None
        }
    }

    // ── Aggregation: daily ──

    pub fn get_daily(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("daily:{}:{}", provider, since);
        let since_date = parse_since_date(since);
        let loaded = self.load_entries_cached(provider, since_date);
        let entries = &loaded.entries;
        self.set_last_query_debug(UsageQueryDebugReport {
            provider: provider.to_string(),
            aggregation: String::from("daily"),
            since: since.to_string(),
            cache_key: cache_key.clone(),
            from_cache: false,
            entry_count: entries.len(),
            sources: loaded.reports.clone(),
        });

        // Group by NaiveDate using a BTreeMap so dates are ordered
        let mut day_map: std::collections::BTreeMap<NaiveDate, Vec<&ParsedEntry>> =
            std::collections::BTreeMap::new();
        for e in entries {
            day_map.entry(e.timestamp.date_naive()).or_default().push(e);
        }

        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, SegmentAgg> = HashMap::new();

        for (date, day_entries) in &day_map {
            let label = date.format("%b %-d").to_string();
            let seg_map = build_segment_map(day_entries);
            let bucket_cost: f64 = seg_map.values().map(|agg| agg.cost).sum();
            let bucket_tokens: u64 = seg_map.values().map(|agg| agg.tokens).sum();

            total_cost += bucket_cost;
            total_tokens += bucket_tokens;

            for e in day_entries.iter() {
                total_input += e.input_tokens;
                total_output += e.output_tokens;
            }

            // Merge into global model map
            for (key, agg) in &seg_map {
                let gm = global_model_map.entry(key.clone()).or_insert(SegmentAgg {
                    display_name: agg.display_name.clone(),
                    cost: 0.0,
                    tokens: 0,
                    pricing_available: true,
                });
                gm.cost += agg.cost;
                gm.tokens += agg.tokens;
                gm.pricing_available &= agg.pricing_available;
            }

            chart_buckets.push(ChartBucket {
                label,
                sort_key: date.format("%Y-%m-%d").to_string(),
                total: bucket_cost,
                segments: segment_map_to_vec(seg_map),
            });
        }

        let model_breakdown = segment_map_to_model_summaries(&global_model_map);
        let session_count = day_map.len() as u32;

        UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            cache_read_tokens: 0,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            web_search_requests: 0,
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            usage_source: UsageSource::Parser,
            usage_warning: Self::provider_usage_warning(provider),
            period_label: String::new(),
            has_earlier_data: false,
            change_stats: None,
            subagent_stats: None,
            device_breakdown: None,
            device_chart_buckets: None,
            provider_detected: None,
            cursor_loading: false,
        }
    }

    // ── Aggregation: monthly ──

    pub fn get_monthly(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("monthly:{}:{}", provider, since);
        let since_date = parse_since_date(since);
        let loaded = self.load_entries_cached(provider, since_date);
        let entries = &loaded.entries;
        self.set_last_query_debug(UsageQueryDebugReport {
            provider: provider.to_string(),
            aggregation: String::from("monthly"),
            since: since.to_string(),
            cache_key: cache_key.clone(),
            from_cache: false,
            entry_count: entries.len(),
            sources: loaded.reports.clone(),
        });

        // Group by YYYY-MM string using a BTreeMap for order
        let mut month_map: std::collections::BTreeMap<String, Vec<&ParsedEntry>> =
            std::collections::BTreeMap::new();
        for e in entries {
            let key = e.timestamp.format("%Y-%m").to_string();
            month_map.entry(key).or_default().push(e);
        }

        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, SegmentAgg> = HashMap::new();

        for (ym, month_entries) in &month_map {
            // Label: parse "YYYY-MM" -> "Jan", "Feb", etc.
            let label = NaiveDate::parse_from_str(&format!("{}-01", ym), "%Y-%m-%d")
                .map(|d| d.format("%b").to_string())
                .unwrap_or_else(|_| ym.clone());

            let seg_map = build_segment_map(month_entries);
            let bucket_cost: f64 = seg_map.values().map(|agg| agg.cost).sum();
            let bucket_tokens: u64 = seg_map.values().map(|agg| agg.tokens).sum();

            total_cost += bucket_cost;
            total_tokens += bucket_tokens;

            for e in month_entries.iter() {
                total_input += e.input_tokens;
                total_output += e.output_tokens;
            }

            for (key, agg) in &seg_map {
                let gm = global_model_map.entry(key.clone()).or_insert(SegmentAgg {
                    display_name: agg.display_name.clone(),
                    cost: 0.0,
                    tokens: 0,
                    pricing_available: true,
                });
                gm.cost += agg.cost;
                gm.tokens += agg.tokens;
                gm.pricing_available &= agg.pricing_available;
            }

            chart_buckets.push(ChartBucket {
                label,
                sort_key: ym.clone(),
                total: bucket_cost,
                segments: segment_map_to_vec(seg_map),
            });
        }

        let model_breakdown = segment_map_to_model_summaries(&global_model_map);
        let session_count = month_map.len() as u32;

        UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            cache_read_tokens: 0,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            web_search_requests: 0,
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            usage_source: UsageSource::Parser,
            usage_warning: Self::provider_usage_warning(provider),
            period_label: String::new(),
            has_earlier_data: false,
            change_stats: None,
            subagent_stats: None,
            device_breakdown: None,
            device_chart_buckets: None,
            provider_detected: None,
            cursor_loading: false,
        }
    }

    // ── Aggregation: hourly ──

    pub fn get_hourly(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("hourly:{}:{}", provider, since);
        let since_date = parse_since_date(since);
        let end_date = since_date.map(|date| date + chrono::Duration::days(1));
        let loaded = self.load_entries_cached(provider, since_date);
        let entries: Vec<&ParsedEntry> = loaded
            .entries
            .iter()
            .filter(|entry| end_date.is_none_or(|end| entry.timestamp.date_naive() < end))
            .collect();
        self.set_last_query_debug(UsageQueryDebugReport {
            provider: provider.to_string(),
            aggregation: String::from("hourly"),
            since: since.to_string(),
            cache_key: cache_key.clone(),
            from_cache: false,
            entry_count: entries.len(),
            sources: loaded.reports.clone(),
        });

        // Group by hour (0-23)
        let mut hour_map: HashMap<u32, Vec<&ParsedEntry>> = HashMap::new();
        for e in &entries {
            hour_map.entry(e.timestamp.hour()).or_default().push(*e);
        }

        let now = Local::now();
        let today = now.date_naive();
        let since_naive = parse_since_date(since);
        let is_past_day = since_naive.is_some_and(|d| d < today);
        let (start_hour, end_hour) = if is_past_day {
            (0u32, 23u32)
        } else {
            let current_hour = now.hour();
            let min_hour = hour_map.keys().copied().min().unwrap_or(current_hour);
            (min_hour, current_hour)
        };

        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, SegmentAgg> = HashMap::new();

        for h in start_hour..=end_hour {
            let label = format_hour(h);
            let hour_entries = hour_map.get(&h).map(|v| v.as_slice()).unwrap_or(&[]);

            let seg_map = build_segment_map(hour_entries);
            let bucket_cost: f64 = seg_map.values().map(|agg| agg.cost).sum();
            let bucket_tokens: u64 = seg_map.values().map(|agg| agg.tokens).sum();

            total_cost += bucket_cost;
            total_tokens += bucket_tokens;

            for e in hour_entries.iter() {
                total_input += e.input_tokens;
                total_output += e.output_tokens;
            }

            for (key, agg) in &seg_map {
                let gm = global_model_map.entry(key.clone()).or_insert(SegmentAgg {
                    display_name: agg.display_name.clone(),
                    cost: 0.0,
                    tokens: 0,
                    pricing_available: true,
                });
                gm.cost += agg.cost;
                gm.tokens += agg.tokens;
                gm.pricing_available &= agg.pricing_available;
            }

            chart_buckets.push(ChartBucket {
                label,
                sort_key: format!("{:02}", h),
                total: bucket_cost,
                segments: segment_map_to_vec(seg_map),
            });
        }

        let model_breakdown = segment_map_to_model_summaries(&global_model_map);
        let session_count = chart_buckets.iter().filter(|b| b.total > 0.0).count() as u32;

        UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            cache_read_tokens: 0,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            web_search_requests: 0,
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            usage_source: UsageSource::Parser,
            usage_warning: Self::provider_usage_warning(provider),
            period_label: String::new(),
            has_earlier_data: false,
            change_stats: None,
            subagent_stats: None,
            device_breakdown: None,
            device_chart_buckets: None,
            provider_detected: None,
            cursor_loading: false,
        }
    }

    /// Every request from `start` on as (time, display_name, model_key, USD),
    /// sorted by time. From the logs alone, so each keeps its own time: an
    /// archived hour would come back as one row at the top of the hour.
    pub fn priced_entries_since(
        &self,
        provider: &str,
        start: DateTime<Local>,
    ) -> Vec<(DateTime<Local>, String, String, f64)> {
        let loaded = self.load_live_entries_cached(provider, Some(start.date_naive()));
        let mut out: Vec<_> = loaded
            .entries
            .iter()
            .filter(|e| e.timestamp >= start)
            .map(|e| {
                let (name, key, cost) = price_entry(e);
                (e.timestamp, name, key, cost)
            })
            .collect();
        out.sort_by_key(|row| row.0);
        out
    }

    // ── Aggregation: official 5h window (or any [start, end) instant range) ──

    pub fn get_time_range(
        &self,
        provider: &str,
        start: DateTime<Local>,
        end: DateTime<Local>,
    ) -> UsagePayload {
        let loaded = self.load_entries_cached(provider, Some(start.date_naive()));
        let entries: Vec<&ParsedEntry> = loaded
            .entries
            .iter()
            .filter(|entry| entry.timestamp >= start && entry.timestamp < end)
            .collect();
        self.set_last_query_debug(UsageQueryDebugReport {
            provider: provider.to_string(),
            aggregation: String::from("time_range"),
            since: start.to_rfc3339(),
            cache_key: format!(
                "range:{}:{}:{}",
                provider,
                start.to_rfc3339(),
                end.to_rfc3339()
            ),
            from_cache: false,
            entry_count: entries.len(),
            sources: loaded.reports.clone(),
        });

        let mut hour_map: HashMap<DateTime<Local>, Vec<&ParsedEntry>> = HashMap::new();
        for entry in &entries {
            hour_map
                .entry(truncate_hour(entry.timestamp))
                .or_default()
                .push(*entry);
        }

        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, SegmentAgg> = HashMap::new();

        let mut hour = truncate_hour(start);
        while hour < end {
            let hour_entries = hour_map.get(&hour).map(|v| v.as_slice()).unwrap_or(&[]);
            let seg_map = build_segment_map(hour_entries);
            let bucket_cost: f64 = seg_map.values().map(|agg| agg.cost).sum();
            let bucket_tokens: u64 = seg_map.values().map(|agg| agg.tokens).sum();

            total_cost += bucket_cost;
            total_tokens += bucket_tokens;
            for e in hour_entries.iter() {
                total_input += e.input_tokens;
                total_output += e.output_tokens;
            }
            for (key, agg) in &seg_map {
                let gm = global_model_map.entry(key.clone()).or_insert(SegmentAgg {
                    display_name: agg.display_name.clone(),
                    cost: 0.0,
                    tokens: 0,
                    pricing_available: true,
                });
                gm.cost += agg.cost;
                gm.tokens += agg.tokens;
                gm.pricing_available &= agg.pricing_available;
            }

            chart_buckets.push(ChartBucket {
                label: format_hour(hour.hour()),
                sort_key: hour.format("%Y-%m-%dT%H:00:00%z").to_string(),
                total: bucket_cost,
                segments: segment_map_to_vec(seg_map),
            });
            hour += Duration::hours(1);
        }

        let now = Local::now();
        let elapsed_hours = (now - start).num_milliseconds().max(1) as f64 / 3_600_000.0;
        // Rolling fallback ends at resolve-time `now` (exclusive), so the live
        // check needs a small grace for the aggregation that follows.
        let active_block = if now >= start && now < end + Duration::seconds(2) {
            let burn_rate_per_hour = total_cost / elapsed_hours;
            Some(ActiveBlock {
                cost: total_cost,
                burn_rate_per_hour,
                projected_cost: burn_rate_per_hour * 5.0,
                is_active: true,
            })
        } else {
            None
        };

        UsagePayload {
            total_cost,
            total_tokens,
            session_count: chart_buckets.iter().filter(|b| b.total > 0.0).count() as u32,
            input_tokens: total_input,
            output_tokens: total_output,
            cache_read_tokens: 0,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            web_search_requests: 0,
            chart_buckets,
            model_breakdown: segment_map_to_model_summaries(&global_model_map),
            active_block,
            five_hour_cost: total_cost,
            last_updated: now.to_rfc3339(),
            from_cache: false,
            usage_source: UsageSource::Parser,
            usage_warning: Self::provider_usage_warning(provider),
            period_label: String::new(),
            has_earlier_data: false,
            change_stats: None,
            subagent_stats: None,
            device_breakdown: None,
            device_chart_buckets: None,
            provider_detected: None,
            cursor_loading: false,
        }
    }

    // ── Aggregation: blocks ──

    #[cfg(test)]
    pub fn get_blocks(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("blocks:{}:{}", provider, since);
        let since_date = parse_since_date(since);
        let loaded = self.load_entries_cached(provider, since_date);
        let mut entries: Vec<&ParsedEntry> = loaded.entries.iter().collect();
        self.set_last_query_debug(UsageQueryDebugReport {
            provider: provider.to_string(),
            aggregation: String::from("blocks"),
            since: since.to_string(),
            cache_key: cache_key.clone(),
            from_cache: false,
            entry_count: entries.len(),
            sources: loaded.reports.clone(),
        });

        // Sort by timestamp ascending
        entries.sort_by_key(|a| a.timestamp);

        // NOT a const — chrono::Duration::minutes() is not const fn
        let gap_threshold = chrono::Duration::minutes(30);

        // Split into blocks separated by gaps > 30 minutes
        let mut blocks: Vec<Vec<&ParsedEntry>> = Vec::new();
        {
            let mut current_block: Vec<&ParsedEntry> = Vec::new();
            let mut prev_ts: Option<DateTime<Local>> = None;

            for &e in &entries {
                if let Some(prev) = prev_ts {
                    if e.timestamp - prev > gap_threshold && !current_block.is_empty() {
                        blocks.push(std::mem::take(&mut current_block));
                    }
                }
                current_block.push(e);
                prev_ts = Some(e.timestamp);
            }
            if !current_block.is_empty() {
                blocks.push(current_block);
            }
        }

        let now = Local::now();
        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut global_model_map: HashMap<String, SegmentAgg> = HashMap::new();
        let mut active_block: Option<ActiveBlock> = None;
        let mut five_hour_cost = 0.0f64;

        for (idx, block) in blocks.iter().enumerate() {
            let seg_map = build_segment_map(block);
            let block_cost: f64 = seg_map.values().map(|agg| agg.cost).sum();
            let block_tokens: u64 = seg_map.values().map(|agg| agg.tokens).sum();

            total_cost += block_cost;
            total_tokens += block_tokens;

            for (key, agg) in &seg_map {
                let gm = global_model_map.entry(key.clone()).or_insert(SegmentAgg {
                    display_name: agg.display_name.clone(),
                    cost: 0.0,
                    tokens: 0,
                    pricing_available: true,
                });
                gm.cost += agg.cost;
                gm.tokens += agg.tokens;
                gm.pricing_available &= agg.pricing_available;
            }

            // Label: start time of block formatted as "9am", "10am", etc.
            let start_ts = block[0].timestamp;
            let label = start_ts.format("%-I%P").to_string();

            chart_buckets.push(ChartBucket {
                label,
                sort_key: start_ts.to_rfc3339(),
                total: block_cost,
                segments: segment_map_to_vec(seg_map),
            });

            // Last block gets ActiveBlock data
            if idx == blocks.len() - 1 {
                let last_entry_ts = block.last().unwrap().timestamp;
                // Use a 2-minute grace period beyond the gap threshold to prevent
                // five_hour_cost from oscillating at the exact 30-minute boundary.
                // Block splitting still uses the original gap_threshold.
                let active_grace = gap_threshold + chrono::Duration::minutes(2);
                let is_active = (now - last_entry_ts) <= active_grace;

                let duration_secs = {
                    let d = last_entry_ts - start_ts;
                    d.num_seconds().max(1) as f64
                };
                let burn_rate_per_hour = block_cost / (duration_secs / 3600.0);

                // Project to 5-hour block
                let projected_cost = burn_rate_per_hour * 5.0;

                if is_active {
                    active_block = Some(ActiveBlock {
                        cost: block_cost,
                        burn_rate_per_hour,
                        projected_cost,
                        is_active,
                    });
                    five_hour_cost = block_cost;
                }
            }
        }

        if active_block.is_none() {
            five_hour_cost = total_cost;
        }

        let model_breakdown = segment_map_to_model_summaries(&global_model_map);
        let session_count = blocks.len() as u32;

        UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            web_search_requests: 0,
            chart_buckets,
            model_breakdown,
            active_block,
            five_hour_cost,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            usage_source: UsageSource::Parser,
            usage_warning: Self::provider_usage_warning(provider),
            period_label: String::new(),
            has_earlier_data: false,
            change_stats: None,
            subagent_stats: None,
            device_breakdown: None,
            device_chart_buckets: None,
            provider_detected: None,
            cursor_loading: false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::codex_parser::*;
    use crate::usage::cursor_parser::*;
    use std::fs;
    use tempfile::TempDir;

    // ── Helpers ──

    fn write_file(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
    }

    fn test_entry(model: &str, hour: u32) -> ParsedEntry {
        use chrono::TimeZone;

        ParsedEntry {
            timestamp: Local
                .with_ymd_and_hms(2026, 6, 12, hour, 5, 0)
                .single()
                .unwrap(),
            model: model.to_string(),
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: 0,
            web_search_requests: 0,
            unique_hash: None,
            session_key: String::from("test"),
            agent_scope: crate::stats::subagent::AgentScope::Main,
        }
    }

    #[test]
    fn archived_unknown_hour_is_replaced_by_live_named_model() {
        let archived = vec![test_entry("unknown", 10)];
        let live = vec![test_entry("claude-fable-5", 10)];
        let frontier = crate::usage::archive::ArchiveFrontier {
            date: archived[0].timestamp.date_naive(),
            hour: 10,
        };

        let mut merged = Vec::new();
        merge_archived_and_live_entries(&mut merged, archived, live, Some(frontier));

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].model, "claude-fable-5");
    }

    #[test]
    fn live_data_kept_for_frontier_hours_with_no_archive_rows() {
        let archived = vec![test_entry("claude-sonnet-4-6", 10)];
        let live = vec![
            test_entry("claude-sonnet-4-6", 10),
            test_entry("claude-sonnet-4-6", 11),
            test_entry("claude-sonnet-4-6", 13),
        ];
        let frontier = crate::usage::archive::ArchiveFrontier {
            date: archived[0].timestamp.date_naive(),
            hour: 12,
        };

        let mut merged = Vec::new();
        merge_archived_and_live_entries(&mut merged, archived, live, Some(frontier));

        let hours: Vec<u32> = merged.iter().map(|e| e.timestamp.hour()).collect();
        assert!(hours.contains(&10), "archived hour 10 should remain");
        assert!(
            hours.contains(&11),
            "empty hour 11 under the frontier must keep later-arriving live data"
        );
        assert!(hours.contains(&13), "hours past the frontier stay live");
        assert_eq!(
            merged.iter().filter(|e| e.timestamp.hour() == 10).count(),
            1,
            "hour 10 live must be dropped because archive has rows"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Symlink-skip guards (macOS TCC safety — see glob_jsonl_files doc comment)
    // ─────────────────────────────────────────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn glob_jsonl_files_skips_symlinked_subdirectories() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().unwrap();
        let elsewhere = TempDir::new().unwrap();

        // Regular file inside the root — should be found.
        write_file(&root.path().join("session.jsonl"), "{}");

        // JSONL outside the root, reached only via a symlinked directory.
        // Following the symlink would cross onto whatever volume `elsewhere`
        // lives on — exactly the case that triggers macOS TCC prompts.
        write_file(&elsewhere.path().join("offsite.jsonl"), "{}");
        symlink(elsewhere.path(), root.path().join("link")).unwrap();

        let found = glob_jsonl_files(root.path());
        assert_eq!(found.len(), 1, "symlinked subdir must not be traversed");
        assert!(found[0].ends_with("session.jsonl"));
    }

    #[cfg(unix)]
    #[test]
    fn scan_jsonl_tree_skips_symlinked_subdirectories() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().unwrap();
        let elsewhere = TempDir::new().unwrap();

        write_file(&root.path().join("session.jsonl"), "{}");
        write_file(&elsewhere.path().join("offsite.jsonl"), "{}");
        symlink(elsewhere.path(), root.path().join("link")).unwrap();

        let mut files = Vec::new();
        let mut dirs = Vec::new();
        scan_jsonl_tree_into(root.path(), ProviderFileKind::Claude, &mut files, &mut dirs);
        assert_eq!(files.len(), 1, "symlinked subdir must not be traversed");
        assert!(files[0].ends_with("session.jsonl"));
        // The symlinked dir also must not appear in the directory-stamp list.
        assert!(!dirs.iter().any(|d| d.path.ends_with("link")));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Cursor parsing
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn classify_cursor_secret_recognizes_admin_prefix() {
        assert_eq!(
            classify_cursor_secret("key_abc123"),
            Some(CursorAuth::Admin(String::from("key_abc123")))
        );
        // Whitespace stripped.
        assert_eq!(
            classify_cursor_secret("  key_xyz  "),
            Some(CursorAuth::Admin(String::from("key_xyz")))
        );
    }

    #[test]
    fn classify_cursor_secret_falls_back_to_dashboard() {
        let workos = "user_01ABCD::eyJhbGciOiJIUzI1NiJ9.payload.sig";
        assert_eq!(
            classify_cursor_secret(workos),
            Some(CursorAuth::Dashboard(workos.to_string()))
        );
    }

    #[test]
    fn classify_cursor_secret_rejects_blank() {
        assert_eq!(classify_cursor_secret(""), None);
        assert_eq!(classify_cursor_secret("   "), None);
        assert_eq!(classify_cursor_secret("\n\t"), None);
    }

    #[test]
    fn choose_cursor_auth_prefers_secret_override() {
        // Override beats every other source.
        let override_token = "user_01ABCD::session-token";
        let auth = choose_cursor_auth(
            Some("key_legacy_admin"),
            Some("user_99XYZ::other-session"),
            Some(override_token),
            Some("ide_token_should_lose"),
        )
        .expect("override should produce a credential");
        assert_eq!(auth, CursorAuth::Dashboard(override_token.to_string()));
    }

    #[test]
    fn choose_cursor_auth_session_token_env_beats_api_key_env() {
        // No override: CURSOR_SESSION_TOKEN wins over CURSOR_API_KEY because
        // users typically only set the session-token var explicitly when
        // they've deliberately switched to the dashboard path.
        let auth = choose_cursor_auth(
            Some("key_legacy_admin"),
            Some("user_01ABCD::dashboard-session"),
            None,
            None,
        )
        .expect("env-supplied session token should produce a credential");
        assert_eq!(
            auth,
            CursorAuth::Dashboard(String::from("user_01ABCD::dashboard-session"))
        );
    }

    #[test]
    fn choose_cursor_auth_falls_back_to_api_key_env() {
        let auth = choose_cursor_auth(Some("key_admin_only"), None, None, None)
            .expect("api-key env should produce a credential");
        assert_eq!(auth, CursorAuth::Admin(String::from("key_admin_only")));
    }

    #[test]
    fn choose_cursor_auth_falls_back_to_ide_token_when_nothing_else_set() {
        let ide_token = "eyJhbGciOiJIUzI1NiJ9.payload.sig";
        let auth = choose_cursor_auth(None, None, None, Some(ide_token))
            .expect("ide token should produce a credential at the lowest tier");
        assert_eq!(auth, CursorAuth::IdeBearer(ide_token.to_string()));
    }

    #[test]
    fn choose_cursor_auth_user_secret_beats_ide_token() {
        // Even an explicit but "weak" secret (no `key_` prefix → Dashboard)
        // should win over the auto-detected IDE token. Users may have
        // deliberately pasted a different account's session.
        let pasted = "user_99ZZZ::pasted-by-hand";
        let ide_token = "eyJhbGciOiJIUzI1NiJ9.different.user";
        let auth = choose_cursor_auth(None, None, Some(pasted), Some(ide_token))
            .expect("user paste should beat IDE auto-detect");
        assert_eq!(auth, CursorAuth::Dashboard(pasted.to_string()));
    }

    #[test]
    fn choose_cursor_auth_returns_none_when_all_blank() {
        assert!(choose_cursor_auth(None, None, None, None).is_none());
        assert!(choose_cursor_auth(Some(""), Some("   "), Some("\n"), Some("\t")).is_none());
    }

    #[test]
    fn cursor_request_url_branches_by_auth_kind() {
        assert!(
            cursor_request_url(&CursorAuth::Admin(String::from("key_x")))
                .contains("api.cursor.com/teams/filtered-usage-events")
        );
        assert!(
            cursor_request_url(&CursorAuth::Dashboard(String::from("session")))
                .contains("cursor.com/api/dashboard/get-filtered-usage-events")
        );
        assert!(
            cursor_request_url(&CursorAuth::IdeBearer(String::from("eyJ.bearer.jwt")))
                .contains("api2.cursor.sh/aiserver.v1.DashboardService/GetFilteredUsageEvents")
        );
    }

    #[test]
    fn cursor_session_key_for_uses_distinct_prefixes_per_auth_kind() {
        assert_eq!(
            cursor_session_key_for(CursorAuthKind::Admin),
            "cursor-admin"
        );
        assert_eq!(
            cursor_session_key_for(CursorAuthKind::Dashboard),
            "cursor-dashboard"
        );
        assert_eq!(
            cursor_session_key_for(CursorAuthKind::IdeBearer),
            "cursor-ide"
        );
    }

    #[test]
    fn parse_cursor_official_usage_events_extracts_token_usage_from_admin_payload() {
        let data = serde_json::json!({
            "usageEvents": [
                {
                    "timestamp": "1750979225854",
                    "userEmail": "developer@example.com",
                    "model": "claude-4.5-sonnet",
                    "tokenUsage": {
                        "inputTokens": 126,
                        "outputTokens": 450,
                        "cacheWriteTokens": 6112,
                        "cacheReadTokens": 11964,
                        "totalCents": 20.18232
                    }
                },
                {
                    "timestamp": "1750979173824",
                    "model": "request-based",
                    "isTokenBasedCall": false
                }
            ],
            "pagination": { "hasNextPage": false }
        });

        let entries = parse_cursor_official_usage_events(&data, None, "cursor-admin").unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].model, "claude-4.5-sonnet");
        assert_eq!(entries[0].input_tokens, 126);
        assert_eq!(entries[0].output_tokens, 450);
        assert_eq!(entries[0].cache_creation_1h_tokens, 6112);
        assert_eq!(entries[0].cache_read_tokens, 11964);
        assert_eq!(entries[0].session_key, "cursor-admin");
    }

    #[test]
    fn parse_cursor_official_usage_events_tags_dashboard_session_key() {
        // Dashboard schema sample — same shape as admin, just tagged with a
        // different session_key so downstream aggregation can disambiguate.
        let data = serde_json::json!({
            "usageEvents": [
                {
                    "timestamp": "1750979225854",
                    "model": "gpt-5.4",
                    "tokenUsage": {
                        "inputTokens": 200,
                        "outputTokens": 80,
                        "cacheWriteTokens": 0,
                        "cacheReadTokens": 50
                    },
                    "kind": "USAGE_EVENT_KIND_USAGE_BASED",
                    "maxMode": false
                }
            ],
            "pagination": { "hasNextPage": false }
        });

        let entries = parse_cursor_official_usage_events(&data, None, "cursor-dashboard").unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].session_key, "cursor-dashboard");
        assert_eq!(entries[0].input_tokens, 200);
        assert_eq!(entries[0].output_tokens, 80);
        assert_eq!(entries[0].cache_read_tokens, 50);
    }

    #[test]
    fn parse_cursor_official_usage_events_handles_ide_bearer_display_array() {
        // The IDE-bearer Connect-Web endpoint uses `usageEventsDisplay`
        // instead of `usageEvents`. Same per-row shape, with extra fields
        // we ignore (kind, requestsCosts, chargedCents, owningUser, …).
        // Pagination is communicated via `totalUsageEventsCount` (string-
        // encoded int64 under Connect-Web's JSON convention).
        let data = serde_json::json!({
            "totalUsageEventsCount": "114",
            "usageEventsDisplay": [
                {
                    "timestamp": "1777165184690",
                    "model": "claude-opus-4-7-thinking-max",
                    "kind": "USAGE_EVENT_KIND_INCLUDED_IN_PRO_PLUS",
                    "maxMode": true,
                    "requestsCosts": 133.7,
                    "isTokenBasedCall": true,
                    "tokenUsage": {
                        "inputTokens": 22,
                        "outputTokens": 20245,
                        "cacheWriteTokens": 350245,
                        "cacheReadTokens": 5301898,
                        "totalCents": 534.6215249999999
                    },
                    "owningUser": "346002640",
                    "chargedCents": 534.621525
                }
            ]
        });

        let entries = parse_cursor_official_usage_events(&data, None, "cursor-ide").unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].session_key, "cursor-ide");
        assert_eq!(entries[0].model, "claude-opus-4-7-thinking-max");
        assert_eq!(entries[0].input_tokens, 22);
        assert_eq!(entries[0].output_tokens, 20245);
        assert_eq!(entries[0].cache_creation_1h_tokens, 350245);
        assert_eq!(entries[0].cache_read_tokens, 5301898);
    }

    #[test]
    fn parse_cursor_official_usage_events_treats_missing_array_as_empty_page() {
        // api2 (protobuf-JSON) answers an empty page with `{}`.
        for data in [
            serde_json::json!({}),
            serde_json::json!({"someOtherField": []}),
        ] {
            let entries = parse_cursor_official_usage_events(&data, None, "cursor-ide").unwrap();
            assert!(entries.is_empty());
        }
    }

    #[test]
    fn parse_cursor_official_usage_events_errors_on_non_object_payload() {
        let data = serde_json::json!([1, 2, 3]);
        match parse_cursor_official_usage_events(&data, None, "cursor-ide") {
            Ok(_) => panic!("expected error for a non-object payload"),
            Err(err) => assert!(err.contains("not a JSON object"), "got: {err}"),
        }
    }

    #[test]
    fn cursor_response_has_next_page_uses_pagination_object_when_present() {
        let with_more = serde_json::json!({"pagination": {"hasNextPage": true}});
        let without_more = serde_json::json!({"pagination": {"hasNextPage": false}});
        assert!(cursor_response_has_next_page(&with_more, 1, 100));
        assert!(!cursor_response_has_next_page(&without_more, 1, 100));
    }

    #[test]
    fn cursor_response_has_next_page_uses_total_count_for_ide_bearer_payloads() {
        // 114 total, page 1 of 100 → still 14 more on page 2.
        let p1 = serde_json::json!({"totalUsageEventsCount": "114"});
        assert!(cursor_response_has_next_page(&p1, 1, 100));
        // After page 2 we've covered 200 events, more than the total.
        assert!(!cursor_response_has_next_page(&p1, 2, 100));
        // Numeric encoding works too, in case a deployment stops string-
        // encoding int64 fields.
        let numeric = serde_json::json!({"totalUsageEventsCount": 250});
        assert!(cursor_response_has_next_page(&numeric, 2, 100));
        assert!(!cursor_response_has_next_page(&numeric, 3, 100));
    }

    #[test]
    fn cursor_response_has_next_page_returns_false_with_no_pagination_info() {
        let neither = serde_json::json!({"usageEvents": []});
        assert!(!cursor_response_has_next_page(&neither, 1, 100));
    }

    #[test]
    fn parse_cursor_session_file_extracts_token_usage() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("session.json");
        write_file(
            &path,
            r#"{"messages":[{"id":"event-1","timestamp":"2026-03-15T12:00:00+00:00","model":"cursor-model","tokenUsage":{"inputTokens":100,"outputTokens":50,"cacheReadTokens":25,"cacheWriteTokens":10}}]}"#,
        );

        let (entries, _change_events, lines_read, opened) = parse_cursor_session_file(&path);

        assert!(opened);
        assert_eq!(lines_read, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].input_tokens, 100);
        assert_eq!(entries[0].output_tokens, 50);
        assert_eq!(entries[0].cache_read_tokens, 25);
        assert_eq!(entries[0].cache_creation_1h_tokens, 10);
    }

    #[test]
    fn cursor_local_debug_reports_readable_files_without_usage_entries() {
        let root = TempDir::new().unwrap();
        let chat_dir = root.path().join("workspace-a").join("chatSessions");
        fs::create_dir_all(&chat_dir).unwrap();
        write_file(
            &chat_dir.join("session.json"),
            r#"{"messages":[{"id":"event-1","text":"hello"}]}"#,
        );
        let parser = UsageParser::from_integrations(usage_integration_configs_with_overrides(
            None,
            None,
            Some(vec![root.path().to_path_buf()]),
        ));

        let (entries, report) = parser.load_cursor_local_entries_with_debug(None);

        assert!(entries.is_empty());
        assert_eq!(report.discovered_paths, 1);
        assert_eq!(report.opened_paths, 1);
        assert_eq!(report.emitted_entries, 0);
    }

    #[test]
    fn cursor_chat_listing_and_parses_are_reused_until_the_sweep() {
        let root = TempDir::new().unwrap();
        let chat = |workspace: &str| root.path().join(workspace).join("chatSessions");
        fs::create_dir_all(chat("workspace-a")).unwrap();
        write_file(
            &chat("workspace-a").join("session.json"),
            r#"{"messages":[{"id":"event-1","text":"hello"}]}"#,
        );
        let parser = UsageParser::from_integrations(usage_integration_configs_with_overrides(
            None,
            None,
            Some(vec![root.path().to_path_buf()]),
        ));
        parser.set_listings_frozen(true);

        let (_, first) = parser.load_cursor_local_entries_with_debug(None);
        assert_eq!((first.listing_cache_hit, first.opened_paths), (false, 1));
        // No walk and no parse, although the file held no usage.
        let (_, again) = parser.load_cursor_local_entries_with_debug(None);
        assert_eq!((again.listing_cache_hit, again.opened_paths), (true, 0));
        assert_eq!(again.cache_hits, 1);

        fs::create_dir_all(chat("workspace-b")).unwrap();
        write_file(
            &chat("workspace-b").join("other.json"),
            r#"{"messages":[{"id":"event-2","timestamp":"2026-03-15T12:00:00+00:00","model":"cursor-model","tokenUsage":{"inputTokens":100,"outputTokens":50}}]}"#,
        );
        let (entries, _) = parser.load_cursor_local_entries_with_debug(None);
        assert!(entries.is_empty(), "listings move only at the sweep");
        assert!(parser.invalidate_if_changed());
        let (entries, _) = parser.load_cursor_local_entries_with_debug(None);
        assert_eq!(entries.len(), 1);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Claude parsing
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn parse_claude_entries_from_jsonl() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"user","timestamp":"2026-03-15T12:01:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5}}}
{"type":"assistant","timestamp":"2026-03-15T12:02:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":80}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let entries = read_claude_entries(dir.path(), None);
        assert_eq!(entries.len(), 2, "should parse only assistant entries");
        assert_eq!(entries[0].input_tokens, 100);
        assert_eq!(entries[1].input_tokens, 200);
    }

    #[test]
    fn parse_claude_filters_by_date() {
        let dir = TempDir::new().unwrap();
        // Use noon UTC to avoid local-timezone edge cases near midnight
        let content = r#"{"type":"assistant","timestamp":"2026-01-01T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":80}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let since = parse_since_date("20260301");
        let entries = read_claude_entries(dir.path(), since);
        assert_eq!(entries.len(), 1, "should only return the March entry");
        assert_eq!(entries[0].input_tokens, 200);
    }

    #[test]
    fn parse_claude_recursive_glob() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("project-abc").join("session-1");
        fs::create_dir_all(&sub).unwrap();

        let entry_line = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":50,"output_tokens":20}}}"#;
        write_file(&dir.path().join("root.jsonl"), entry_line);
        write_file(&sub.join("nested.jsonl"), entry_line);

        let entries = read_claude_entries(dir.path(), None);
        assert_eq!(
            entries.len(),
            2,
            "should find files in nested subdirectories"
        );
    }

    #[test]
    fn parse_claude_dedupes_null_stop_reason_entries_by_message_and_request() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":20,"cache_read_input_tokens":30}}}
{"type":"assistant","timestamp":"2026-03-15T12:00:01+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":20,"cache_read_input_tokens":30}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let (entries, _change_events, reports) =
            parser.load_entries("claude", parse_since_date("20260301"));

        assert_eq!(
            entries.len(),
            1,
            "duplicate assistant transcript entries should count once"
        );
        assert_eq!(entries[0].input_tokens, 10);
        assert_eq!(entries[0].output_tokens, 5);
        assert_eq!(entries[0].cache_creation_1h_tokens, 20);
        assert_eq!(entries[0].cache_read_tokens, 30);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].emitted_entries, 1);
    }

    #[test]
    fn load_entries_drops_third_party_models_from_claude_tab() {
        // Claude Code CLI logs can contain third-party models when proxied
        // (e.g. GLM-5 via an Anthropic-compatible proxy). Those rows must
        // NOT be counted in the "claude" tab, otherwise the main dashboard
        // total diverges from the Per-Device breakdown (which filters by
        // model family for remote SSH rows).
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"assistant","timestamp":"2026-03-15T12:01:00+00:00","message":{"model":"glm-5","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":80}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());

        let (claude_entries, _, _) = parser.load_entries("claude", parse_since_date("20260301"));
        assert_eq!(
            claude_entries.len(),
            1,
            "GLM-5 row logged via Claude Code CLI should not count in the Claude tab"
        );
        assert_eq!(claude_entries[0].model, "claude-sonnet-4-6");

        let (all_entries, _, _) = parser.load_entries("all", parse_since_date("20260301"));
        assert!(
            all_entries.iter().any(|e| e.model == "glm-5"),
            "the 'all' tab should still include the GLM-5 row"
        );

        // The cached loads share one Claude load and filter it the same way.
        let since = parse_since_date("20260301");
        assert_eq!(parser.load_entries_cached("claude", since).entries.len(), 1);
        let all = parser.load_entries_cached("all", since);
        assert!(all.entries.iter().any(|e| e.model == "glm-5"));
    }

    #[test]
    fn parse_claude_dedupe_keeps_latest_output_tokens() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":35,"cache_creation_input_tokens":20,"cache_read_input_tokens":30}}}
{"type":"assistant","timestamp":"2026-03-15T12:00:02+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6","stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":954,"cache_creation_input_tokens":20,"cache_read_input_tokens":30}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let (entries, _change_events, reports) =
            parser.load_entries("claude", parse_since_date("20260301"));

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].output_tokens, 954);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].emitted_entries, 1);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Codex parsing
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn parse_codex_emits_last_usage_for_each_token_event() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("workspace").join("subdir");
        fs::create_dir_all(&session_dir).unwrap();

        let ts = Local::now().format("%Y-%m-%dT12:00:00+00:00").to_string();
        let content = format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp/demo","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50,"reasoning_output_tokens":5,"cached_input_tokens":10}}}}}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":200,"output_tokens":100,"reasoning_output_tokens":15,"cached_input_tokens":20}}}}}}}}"#,
            ts = ts
        );
        write_file(&session_dir.join("session.jsonl"), &content);

        let today_str = Local::now().format("%Y%m%d").to_string();
        let entries = read_codex_entries(dir.path(), parse_since_date(&today_str));
        assert_eq!(
            entries.len(),
            2,
            "should produce one entry per token_count event"
        );
        assert_eq!(entries[0].model, "gpt-5.4");
        assert_eq!(entries[0].input_tokens, 90);
        assert_eq!(entries[0].output_tokens, 50);
        assert_eq!(entries[0].cache_read_tokens, 10);
        assert_eq!(
            entries[1].input_tokens, 180,
            "should preserve per-event usage rather than collapsing to the final event"
        );
        assert_eq!(entries[1].output_tokens, 100);
        assert_eq!(entries[1].cache_read_tokens, 20);
    }

    #[test]
    fn parse_codex_reasoning_follows_total_tokens() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("workspace");
        fs::create_dir_all(&session_dir).unwrap();
        let content = r#"{"type":"turn_context","payload":{"model":"gpt-5.4"}}
{"type":"event_msg","timestamp":"2026-03-15T12:00:00+00:00","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"reasoning_output_tokens":5,"total_tokens":110}}}}
{"type":"event_msg","timestamp":"2026-03-15T12:00:01+00:00","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"reasoning_output_tokens":5,"total_tokens":115}}}}"#;
        write_file(&session_dir.join("session.jsonl"), content);

        let entries = read_codex_entries(dir.path(), parse_since_date("20260301"));
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].output_tokens, 10,
            "A: total=input+output, do not add reasoning"
        );
        assert_eq!(
            entries[1].output_tokens, 15,
            "B: total=input+output+reasoning, add"
        );
    }

    #[test]
    fn parse_codex_total_token_usage_is_converted_to_deltas() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("nested");
        fs::create_dir_all(&session_dir).unwrap();

        let ts1 = "2026-03-15T12:00:00+00:00";
        let ts2 = "2026-03-15T12:05:00+00:00";
        let content = format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp/demo","model":"gpt-5"}}}}
{{"type":"event_msg","timestamp":"{ts1}","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":300,"output_tokens":100,"reasoning_output_tokens":25,"cached_input_tokens":50,"total_tokens":400}}}}}}}}
{{"type":"event_msg","timestamp":"{ts2}","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":450,"output_tokens":160,"reasoning_output_tokens":40,"cached_input_tokens":70,"total_tokens":610}}}}}}}}"#
        );
        write_file(&session_dir.join("session.jsonl"), &content);

        let entries = read_codex_entries(dir.path(), parse_since_date("20260301"));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].input_tokens, 250);
        assert_eq!(entries[0].output_tokens, 100);
        assert_eq!(entries[0].cache_read_tokens, 50);
        assert_eq!(entries[1].input_tokens, 130);
        assert_eq!(entries[1].output_tokens, 60);
        assert_eq!(entries[1].cache_read_tokens, 20);
    }

    #[test]
    fn parse_codex_total_token_usage_skips_duplicate_replays() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("nested");
        fs::create_dir_all(&session_dir).unwrap();

        let ts1 = "2026-03-15T12:00:00+00:00";
        let ts2 = "2026-03-15T12:00:01+00:00";
        let ts3 = "2026-03-15T12:00:02+00:00";
        let content = format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp/demo","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts1}","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":120,"cached_input_tokens":20,"output_tokens":30,"total_tokens":150}},"last_token_usage":{{"input_tokens":120,"cached_input_tokens":20,"output_tokens":30,"total_tokens":150}}}}}}}}
{{"type":"event_msg","timestamp":"{ts2}","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":120,"cached_input_tokens":20,"output_tokens":30,"total_tokens":150}},"last_token_usage":{{"input_tokens":120,"cached_input_tokens":20,"output_tokens":30,"total_tokens":150}}}}}}}}
{{"type":"event_msg","timestamp":"{ts3}","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":170,"cached_input_tokens":30,"output_tokens":50,"total_tokens":220}},"last_token_usage":{{"input_tokens":50,"cached_input_tokens":10,"output_tokens":20,"total_tokens":70}}}}}}}}"#,
        );
        write_file(&session_dir.join("session.jsonl"), &content);

        let entries = read_codex_entries(dir.path(), parse_since_date("20260301"));
        assert_eq!(
            entries.len(),
            2,
            "duplicate replay should not emit a second entry"
        );
        assert_eq!(entries[0].input_tokens, 100);
        assert_eq!(entries[0].output_tokens, 30);
        assert_eq!(entries[0].cache_read_tokens, 20);
        assert_eq!(entries[1].input_tokens, 40);
        assert_eq!(entries[1].output_tokens, 20);
        assert_eq!(entries[1].cache_read_tokens, 10);
    }

    #[test]
    fn parse_codex_assigns_pre_context_usage_to_first_known_model() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("workspace");
        fs::create_dir_all(&session_dir).unwrap();

        let ts1 = "2026-03-15T12:00:00+00:00";
        let ts2 = "2026-03-15T12:05:00+00:00";
        let content = format!(
            r#"{{"type":"event_msg","timestamp":"{ts1}","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":120,"cached_input_tokens":20,"output_tokens":30,"total_tokens":150}}}}}}}}
{{"type":"turn_context","payload":{{"cwd":"/tmp/demo","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts2}","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":150,"cached_input_tokens":25,"output_tokens":45,"total_tokens":195}}}}}}}}"#,
        );
        write_file(&session_dir.join("session.jsonl"), &content);

        let entries = read_codex_entries(dir.path(), parse_since_date("20260301"));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].model, "gpt-5.4");
        assert_eq!(entries[1].model, "gpt-5.4");
        assert_eq!(entries[0].input_tokens, 100);
        assert_eq!(entries[1].input_tokens, 25);
    }

    #[test]
    fn parse_codex_filters_by_timestamp_date() {
        let dir = TempDir::new().unwrap();

        let session_dir = dir.path().join("workspace").join("history");
        fs::create_dir_all(&session_dir).unwrap();
        let old_ts = "2025-01-01T12:00:00+00:00";
        let old_content = format!(
            r#"{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":999,"output_tokens":1}}}}}}}}"#,
            ts = old_ts
        );
        write_file(&session_dir.join("old.jsonl"), &old_content);

        let today = Local::now().date_naive();
        let today_str = today.format("%Y%m%d").to_string();
        let entries = read_codex_entries(dir.path(), parse_since_date(&today_str));
        assert!(entries.is_empty(), "old timestamp should be excluded");
    }

    #[test]
    fn parse_codex_empty_dir_returns_empty() {
        let dir = TempDir::new().unwrap();
        let entries = read_codex_entries(dir.path(), None);
        assert!(entries.is_empty());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Daily aggregation
    // ─────────────────────────────────────────────────────────────────────────

    fn make_parser_with_claude_data(content: &str) -> (TempDir, UsageParser) {
        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("session.jsonl"), content);
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        (dir, parser)
    }

    #[test]
    fn daily_aggregation_groups_by_date() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-14T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500}}}
{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":2000,"output_tokens":1000}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);
        let payload = parser.get_daily("claude", "20260101");

        assert_eq!(payload.chart_buckets.len(), 2, "should have 2 day buckets");
        let labels: Vec<&str> = payload
            .chart_buckets
            .iter()
            .map(|b| b.label.as_str())
            .collect();
        assert!(labels.contains(&"Mar 14"), "should have Mar 14 bucket");
        assert!(labels.contains(&"Mar 15"), "should have Mar 15 bucket");
    }

    #[test]
    fn daily_aggregation_model_breakdown() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500}}}
{"type":"assistant","timestamp":"2026-03-15T12:30:00+00:00","message":{"model":"claude-opus-4-6","stop_reason":"end_turn","usage":{"input_tokens":500,"output_tokens":200}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);
        let payload = parser.get_daily("claude", "20260315");

        assert_eq!(
            payload.model_breakdown.len(),
            2,
            "should have 2 distinct model summaries"
        );
        let keys: Vec<&str> = payload
            .model_breakdown
            .iter()
            .map(|m| m.model_key.as_str())
            .collect();
        assert!(keys.contains(&"sonnet-4-6"), "should include Sonnet 4.6");
        assert!(keys.contains(&"opus-4-6"), "should include Opus 4.6");
    }

    #[test]
    fn daily_aggregation_keeps_distinct_claude_versions_separate() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-opus-4-5","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500}}}
{"type":"assistant","timestamp":"2026-03-15T12:30:00+00:00","message":{"model":"claude-opus-4-6","stop_reason":"end_turn","usage":{"input_tokens":500,"output_tokens":200}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);
        let payload = parser.get_daily("claude", "20260315");

        assert_eq!(
            payload.model_breakdown.len(),
            2,
            "distinct Claude versions should not collapse into one family bucket"
        );
        let keys: Vec<&str> = payload
            .model_breakdown
            .iter()
            .map(|m| m.model_key.as_str())
            .collect();
        assert!(keys.contains(&"opus-4-5"), "should include Opus 4.5");
        assert!(keys.contains(&"opus-4-6"), "should include Opus 4.6");
    }

    #[test]
    fn daily_aggregation_keeps_distinct_codex_models_separate() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("2026").join("03").join("15");
        fs::create_dir_all(&session_dir).unwrap();

        let content = r#"{"type":"turn_context","payload":{"cwd":"/tmp/demo","model":"gpt-5.1-codex-max"}}
{"type":"event_msg","timestamp":"2026-03-15T12:00:00+00:00","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":50}}}}
{"type":"turn_context","payload":{"cwd":"/tmp/demo","model":"gpt-5.4"}}
{"type":"event_msg","timestamp":"2026-03-15T12:10:00+00:00","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":200,"output_tokens":75}}}}"#;
        write_file(&session_dir.join("session.jsonl"), content);

        let parser = UsageParser::with_codex_dir(dir.path().to_path_buf());
        let payload = parser.get_daily("codex", "20260315");

        assert_eq!(
            payload.model_breakdown.len(),
            2,
            "distinct Codex models should not collapse into one generic bucket"
        );
        let keys: Vec<&str> = payload
            .model_breakdown
            .iter()
            .map(|m| m.model_key.as_str())
            .collect();
        assert!(
            keys.contains(&"gpt-5.1-codex-max"),
            "should include gpt-5.1-codex-max"
        );
        assert!(keys.contains(&"gpt-5.4"), "should include gpt-5.4");
    }

    #[test]
    fn daily_aggregation_includes_cache_tokens_in_totals_and_models() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":50,"cache_read_input_tokens":10,"cache_creation":{"ephemeral_5m_input_tokens":20,"ephemeral_1h_input_tokens":30}}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);
        let payload = parser.get_daily("claude", "20260315");

        assert_eq!(payload.total_tokens, 210);
        assert_eq!(payload.input_tokens, 100);
        assert_eq!(payload.output_tokens, 50);
        assert_eq!(payload.model_breakdown.len(), 1);
        assert_eq!(payload.model_breakdown[0].tokens, 210);
        assert_eq!(payload.chart_buckets[0].segments[0].tokens, 210);
    }

    #[test]
    fn codex_cached_input_is_not_double_counted_in_input_or_cost() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("2026").join("03").join("15");
        fs::create_dir_all(&session_dir).unwrap();

        let content = r#"{"type":"turn_context","payload":{"cwd":"/tmp/demo","model":"gpt-5.4"}}
{"type":"event_msg","timestamp":"2026-03-15T12:00:00+00:00","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10,"reasoning_output_tokens":0,"total_tokens":110}}}}"#;
        write_file(&session_dir.join("session.jsonl"), content);

        let parser = UsageParser::with_codex_dir(dir.path().to_path_buf());
        let payload = parser.get_daily("codex", "20260315");

        assert_eq!(payload.input_tokens, 20);
        assert_eq!(payload.output_tokens, 10);
        assert_eq!(payload.total_tokens, 110);
        assert_eq!(payload.model_breakdown.len(), 1);
        assert_eq!(payload.model_breakdown[0].tokens, 110);
        assert!((payload.total_cost - 0.00022).abs() < 1e-9);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Caching
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn parser_aggregations_use_file_cache_without_payload_cache() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);

        let first = parser.get_daily("claude", "20260315");
        assert!(!first.from_cache, "first call should NOT be from cache");
        let first_debug = parser.last_query_debug().unwrap();
        assert_eq!(first_debug.sources[0].cache_hits, 0);
        assert_eq!(first_debug.sources[0].cache_misses, 1);

        // Production clears entries_cache after every top-level query
        // (usage_query.rs:635); replicate that so this second aggregation
        // re-runs and exercises the per-file parse cache instead of being
        // short-circuited by the (provider:since) entries_cache.
        parser.clear_entries_cache();
        let second = parser.get_daily("claude", "20260315");
        assert!(
            !second.from_cache,
            "parser aggregations should not use the payload cache"
        );
        let second_debug = parser.last_query_debug().unwrap();
        assert_eq!(second_debug.sources[0].cache_hits, 1);
        assert_eq!(second_debug.sources[0].cache_misses, 0);
    }

    #[test]
    fn parsed_file_cache_reuses_claude_file_across_aggregations() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);

        parser.get_daily("claude", "20260101");
        let first_debug = parser.last_query_debug().unwrap();
        let first_source = &first_debug.sources[0];
        assert_eq!(first_source.cache_hits, 0);
        assert_eq!(first_source.cache_misses, 1);
        assert_eq!(first_source.opened_paths, 1);

        // Cross-query reuse: production clears entries_cache per query, so the
        // second aggregation must re-run and hit the parsed-file cache.
        parser.clear_entries_cache();
        parser.get_monthly("claude", "20260101");
        let second_debug = parser.last_query_debug().unwrap();
        let second_source = &second_debug.sources[0];
        assert_eq!(second_source.cache_hits, 1);
        assert_eq!(second_source.cache_misses, 0);
        assert_eq!(second_source.opened_paths, 0);
        assert_eq!(second_source.lines_read, 0);
    }

    #[test]
    fn parsed_file_cache_invalidates_when_claude_file_changes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("session.jsonl");
        write_file(
            &path,
            r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#,
        );
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());

        let first = parser.get_daily("claude", "20260101");
        assert_eq!(first.input_tokens, 100);
        let first_debug = parser.last_query_debug().unwrap();
        assert_eq!(first_debug.sources[0].cache_misses, 1);

        write_file(
            &path,
            concat!(
                r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2026-03-16T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":75}}}"#,
            ),
        );

        // Simulate the background invalidation loop detecting file changes
        parser.invalidate_if_changed();

        let second = parser.get_monthly("claude", "20260101");
        assert_eq!(second.input_tokens, 300);
        assert_eq!(second.output_tokens, 125);
        let second_debug = parser.last_query_debug().unwrap();
        assert_eq!(second_debug.sources[0].cache_hits, 0);
        assert_eq!(second_debug.sources[0].cache_misses, 1);
        assert_eq!(second_debug.sources[0].opened_paths, 1);
    }

    #[test]
    fn clearing_payload_cache_preserves_parsed_file_cache() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);

        parser.get_daily("claude", "20260101");
        let first_debug = parser.last_query_debug().unwrap();
        assert_eq!(first_debug.sources[0].cache_hits, 0);
        assert_eq!(first_debug.sources[0].cache_misses, 1);

        parser.clear_payload_cache();
        parser.get_monthly("claude", "20260101");
        let second_debug = parser.last_query_debug().unwrap();
        assert_eq!(second_debug.sources[0].cache_hits, 1);
        assert_eq!(second_debug.sources[0].cache_misses, 0);
        assert_eq!(second_debug.sources[0].opened_paths, 0);
        assert_eq!(second_debug.sources[0].lines_read, 0);
    }

    #[test]
    fn root_file_list_cache_reuses_scan_when_tree_is_unchanged() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);

        parser.get_daily("claude", "20260101");
        let first_debug = parser.last_query_debug().unwrap();
        assert!(!first_debug.sources[0].listing_cache_hit);

        // Production clears entries_cache per query; replicate so the 2nd call
        // re-runs and reuses the root-file-list (listing) cache.
        parser.clear_entries_cache();
        parser.get_monthly("claude", "20260101");
        let second_debug = parser.last_query_debug().unwrap();
        assert!(second_debug.sources[0].listing_cache_hit);
    }

    #[test]
    fn root_file_list_cache_invalidates_when_tree_changes() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir.path().join("session-a.jsonl"),
            r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#,
        );
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());

        let first = parser.get_daily("claude", "20260101");
        assert_eq!(first.input_tokens, 100);
        let first_debug = parser.last_query_debug().unwrap();
        assert_eq!(first_debug.sources[0].discovered_paths, 1);
        assert!(!first_debug.sources[0].listing_cache_hit);

        write_file(
            &dir.path().join("session-b.jsonl"),
            r#"{"type":"assistant","timestamp":"2026-03-16T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":75}}}"#,
        );
        // Bump the directory mtime so the listing cache detects a change.
        // On Windows, fast writes may land within the same timestamp granularity.
        filetime::set_file_mtime(
            dir.path(),
            filetime::FileTime::from_system_time(
                std::time::SystemTime::now() + std::time::Duration::from_secs(2),
            ),
        )
        .unwrap();

        // entries_cache (keyed provider:since) would otherwise serve the stale
        // pre-change entries; production clears it per query, so do the same and
        // let the listing cache detect the bumped directory mtime.
        parser.clear_entries_cache();
        let second = parser.get_daily("claude", "20260101");
        assert_eq!(second.input_tokens, 300);
        let second_debug = parser.last_query_debug().unwrap();
        assert_eq!(second_debug.sources[0].discovered_paths, 2);
        assert!(!second_debug.sources[0].listing_cache_hit);
    }

    #[test]
    fn invalidate_if_changed_detects_append_to_existing_jsonl() {
        let dir = TempDir::new().unwrap();
        let session_path = dir.path().join("session.jsonl");
        write_file(
            &session_path,
            r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#,
        );
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        parser.get_daily("claude", "20260101");
        parser.store_cache("sentinel", UsagePayload::default());

        assert!(
            !parser.invalidate_if_changed(),
            "unchanged existing file should keep payload cache"
        );
        assert!(
            parser.check_cache("sentinel").is_some(),
            "baseline cache entry should still exist"
        );

        write_file(
            &session_path,
            r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"assistant","timestamp":"2026-03-15T12:05:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":75}}}"#,
        );

        assert!(
            parser.invalidate_if_changed(),
            "appending to an existing session log should invalidate payload cache"
        );
        assert!(
            parser.check_cache("sentinel").is_none(),
            "payload cache should be cleared after source file content changes"
        );
    }

    #[test]
    fn an_append_moves_only_its_own_providers_data_version() {
        let dir = TempDir::new().unwrap();
        let (claude_dir, codex_dir) = (dir.path().join("claude"), dir.path().join("codex"));
        fs::create_dir_all(&claude_dir).unwrap();
        fs::create_dir_all(&codex_dir).unwrap();
        let line = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
        let session = claude_dir.join("session.jsonl");
        write_file(&session, line);
        write_file(&codex_dir.join("rollout.jsonl"), "{}");
        let parser = UsageParser::with_dirs(claude_dir, codex_dir);
        parser.get_daily("claude", "20260101");
        parser.get_daily("codex", "20260101");
        let versions =
            || ["claude", "codex", "cursor"].map(|provider| parser.data_version(provider));
        let before = versions();

        use std::io::Write;
        let mut log = fs::OpenOptions::new().append(true).open(&session).unwrap();
        write!(log, "\n{line}").unwrap();
        drop(log);
        assert!(parser.invalidate_if_changed(), "guard: the sweep sees it");
        let after = versions();
        assert!(after[0] > before[0], "Claude's logs changed");
        assert_eq!(after[1], before[1], "Codex's did not");
        assert_eq!(after[2], before[2], "nor Cursor's: the sweep lists them");
    }

    #[test]
    fn content_append_keeps_earliest_date_cache_warm() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("session.jsonl");
        write_file(
            &path,
            r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#,
        );
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());

        // Warm the listing + earliest-date caches.
        parser.get_daily("claude", "20260101");
        let _ = parser.has_entries_before("claude", NaiveDate::from_ymd_opt(2026, 4, 1).unwrap());
        assert!(
            parser
                .earliest_date_cache
                .lock()
                .unwrap()
                .contains_key("claude"),
            "earliest-date cache should be warm after the first probe"
        );

        // Append a newer entry to the SAME file (in-place content change).
        write_file(
            &path,
            concat!(
                r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2026-03-16T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":75}}}"#,
            ),
        );

        assert!(
            parser.invalidate_if_changed(),
            "append must invalidate the payload cache"
        );

        // An append can never lower the global earliest date, so its cache must
        // be PRESERVED — no multi-hundred-ms re-parse-all on the next query.
        assert!(
            parser
                .earliest_date_cache
                .lock()
                .unwrap()
                .contains_key("claude"),
            "earliest-date cache must survive a content-only append"
        );
        // And the answer is still correct.
        assert!(parser.has_entries_before("claude", NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()));
        assert!(!parser.has_entries_before("claude", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
    }

    /// One Claude request logged at noon UTC on `day` of March 2026.
    fn march_line(day: u32) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"2026-03-{day:02}T12:00:00+00:00","message":{{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":50}}}}}}"#
        )
    }

    fn march(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, day).unwrap()
    }

    /// Move `dir`'s mtime past what the last sweep saw, as a file added to
    /// it does on a filesystem with a coarse mtime.
    fn touch_dir(dir: &Path) {
        filetime::set_file_mtime(
            dir,
            filetime::FileTime::from_system_time(
                std::time::SystemTime::now() + std::time::Duration::from_secs(2),
            ),
        )
        .unwrap();
    }

    #[test]
    fn a_new_file_reparses_only_itself_for_the_earliest_date() {
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("session-a.jsonl");
        write_file(&a, &march_line(15));
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        assert!(parser.has_entries_before("claude", march(16)));

        // A file that did not shrink keeps the date it was read at (an append
        // cannot lower it), so rewriting `a` in place at the same size goes
        // unseen: the new file is all the listing change parses.
        write_file(&a, &march_line(20));
        write_file(&dir.path().join("session-b.jsonl"), &march_line(18));
        touch_dir(dir.path());
        assert!(parser.invalidate_if_changed());
        assert!(
            parser.has_entries_before("claude", march(16)),
            "a's date comes from the memo"
        );
    }

    #[test]
    fn a_new_file_with_older_entries_lowers_the_earliest_date() {
        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("session-a.jsonl"), &march_line(15));
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        assert!(!parser.has_entries_before("claude", march(10)));

        // A resumed session can open a new file with old timestamps.
        write_file(&dir.path().join("session-b.jsonl"), &march_line(1));
        touch_dir(dir.path());
        assert!(parser.invalidate_if_changed());
        assert!(parser.has_entries_before("claude", march(10)));
    }

    #[test]
    fn a_new_file_is_listed_by_reading_only_its_directory() {
        let dir = TempDir::new().unwrap();
        let (claude_dir, codex_dir) = (dir.path().join("claude"), dir.path().join("codex"));
        let (project, other) = (claude_dir.join("project"), claude_dir.join("other"));
        for dir in [&project, &other, &codex_dir] {
            fs::create_dir_all(dir).unwrap();
        }
        write_file(&project.join("a.jsonl"), &march_line(15));
        write_file(&codex_dir.join("rollout.jsonl"), "{}");
        let parser = UsageParser::with_dirs(claude_dir.clone(), codex_dir.clone());
        parser.set_listings_frozen(true);
        parser.get_daily("claude", "20260101");
        parser.get_daily("codex", "20260101");
        let codex_version = parser.data_version("codex");

        // Not a log: its directory is read again, and nothing changed.
        write_file(&project.join("notes.txt"), "x");
        touch_dir(&project);
        assert_eq!(parser.sweep(false), LogChanges::None);
        assert_eq!(parser.sweep(false), LogChanges::None, "at its new stamp");

        write_file(&project.join("b.jsonl"), &march_line(16));
        touch_dir(&project);
        fs::remove_dir(&other).unwrap();
        fs::create_dir(claude_dir.join("new")).unwrap();
        write_file(&claude_dir.join("new").join("c.jsonl"), &march_line(17));
        touch_dir(&claude_dir);
        assert_eq!(parser.sweep(false), LogChanges::Any);
        let (entries, _, reports) = parser.load_entries("claude", None);
        assert!(reports[0].listing_cache_hit, "no walk of the tree");
        assert_eq!(entries.len(), 3);
        let listed = parser.root_file_lists.lock().unwrap()[&path_to_string(&claude_dir)].clone();
        let dirs: HashSet<PathBuf> = listed.directories.iter().map(|d| d.path.clone()).collect();
        assert_eq!(
            dirs,
            HashSet::from([claude_dir.clone(), project, claude_dir.join("new")])
        );
        assert_eq!(parser.data_version("codex"), codex_version, "Codex's stays");
    }

    #[test]
    fn a_log_back_in_the_listing_is_not_served_its_old_parse() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let log = project.join("a.jsonl");
        write_file(&log, &march_line(15));
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        parser.set_listings_frozen(true);
        parser.get_daily("claude", "20260101");

        fs::remove_file(&log).unwrap();
        touch_dir(&project);
        assert_eq!(parser.sweep(false), LogChanges::Any);
        write_file(&log, &format!("{}\n{}", march_line(15), march_line(16)));
        touch_dir(&project);
        assert_eq!(parser.sweep(false), LogChanges::Any);
        let (entries, _, reports) = parser.load_entries("claude", None);
        assert!(reports[0].listing_cache_hit, "guard: listed by the sweep");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn a_file_not_written_for_a_week_is_stat_ed_only_by_a_full_sweep() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("session.jsonl");
        write_file(&log, &march_line(15));
        let eight_days_ago = SystemTime::now() - std::time::Duration::from_secs(8 * 86_400);
        filetime::set_file_mtime(&log, filetime::FileTime::from_system_time(eight_days_ago))
            .unwrap();
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        parser.set_listings_frozen(true);
        parser.get_daily("claude", "20260101");

        use std::io::Write;
        let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
        write!(file, "\n{}", march_line(16)).unwrap();
        drop(file);
        assert_eq!(parser.sweep(false), LogChanges::None);
        let since = (DateTime::<Local>::from(eight_days_ago) - APPEND_DATE_SLACK).date_naive();
        assert_eq!(
            parser.sweep(true),
            LogChanges::Appended(LogAppends {
                integrations: vec![UsageIntegrationId::Claude],
                since,
            }),
            "dated no earlier than an hour before the file's previous write"
        );
        assert_eq!(parser.sweep(false), LogChanges::None, "hot from now on");

        write_file(&log, &march_line(15));
        assert_eq!(
            parser.sweep(false),
            LogChanges::Any,
            "a shrink is a rewrite"
        );
    }

    #[test]
    fn file_cache_drops_old_files_no_load_read_for_an_hour() {
        let (now, wall) = (Instant::now(), SystemTime::now());
        let entry = |modified: SystemTime, read_at: Instant| CachedFileEntries {
            stamp: FileStamp { modified, len: 0 },
            entries: Vec::new().into(),
            change_events: Vec::new().into(),
            earliest_date: None,
            last_accessed_at: read_at,
        };
        let old = wall - std::time::Duration::from_secs(40 * 86_400);
        let later = now + std::time::Duration::from_secs(2 * 3_600);
        let mut cache = HashMap::from([
            ("old".to_string(), entry(old, now)),
            ("old, read since".to_string(), entry(old, later)),
            ("recent".to_string(), entry(wall, now)),
        ]);

        prune_file_cache(&mut cache, later, wall);
        let mut kept: Vec<&str> = cache.keys().map(String::as_str).collect();
        kept.sort();
        assert_eq!(kept, ["old, read since", "recent"]);
    }

    #[test]
    fn content_append_keeps_listing_and_reparses_only_changed_file() {
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.jsonl");
        let b = dir.path().join("b.jsonl");
        write_file(
            &a,
            r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#,
        );
        write_file(
            &b,
            r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5}}}"#,
        );
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        parser.get_monthly("claude", "20260101");

        // Append only to a.jsonl (in-place content change, set unchanged).
        write_file(
            &a,
            concat!(
                r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2026-03-16T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":300,"output_tokens":80}}}"#,
            ),
        );
        assert!(parser.invalidate_if_changed());

        parser.get_monthly("claude", "20260101");
        let debug = parser.last_query_debug().unwrap();
        // Membership unchanged → the listing cache is kept (no tree re-walk).
        assert!(
            debug.sources[0].listing_cache_hit,
            "listing cache must survive a content-only append"
        );
        // Exactly the changed file re-parses; the untouched file is served from
        // the per-file cache.
        assert_eq!(debug.sources[0].opened_paths, 1);
        assert_eq!(debug.sources[0].cache_misses, 1);
        assert_eq!(debug.sources[0].cache_hits, 1);
    }

    #[test]
    fn frozen_listing_defers_new_file_to_next_sweep() {
        let entry_a = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
        let entry_b = r#"{"type":"assistant","timestamp":"2026-03-16T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":75}}}"#;
        let add_second_file = |dir: &Path| {
            write_file(&dir.join("session-b.jsonl"), entry_b);
            // Fast writes may land within the directory mtime granularity.
            filetime::set_file_mtime(
                dir,
                filetime::FileTime::from_system_time(
                    SystemTime::now() + std::time::Duration::from_secs(2),
                ),
            )
            .unwrap();
        };

        // Frozen: a query between sweeps keeps the listing, so the next sweep
        // still sees the new file and reports the change.
        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("session-a.jsonl"), entry_a);
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        parser.set_listings_frozen(true);
        assert_eq!(parser.load_entries("claude", None).0.len(), 1);
        add_second_file(dir.path());
        assert_eq!(
            parser.load_entries("claude", None).0.len(),
            1,
            "a frozen listing must not pick up the new file before the sweep"
        );
        assert!(
            parser.invalidate_if_changed(),
            "the sweep must report the new file"
        );
        assert_eq!(parser.load_entries("claude", None).0.len(), 2);

        // Not frozen: the query re-walks and absorbs the new file, so the next
        // sweep finds nothing to report.
        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("session-a.jsonl"), entry_a);
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        assert_eq!(parser.load_entries("claude", None).0.len(), 1);
        add_second_file(dir.path());
        assert_eq!(parser.load_entries("claude", None).0.len(), 2);
        assert!(!parser.invalidate_if_changed());
    }

    #[test]
    fn payload_ttl_follows_refresh_interval() {
        assert_eq!(payload_ttl_for(0), 86_400);
        assert_eq!(payload_ttl_for(30), 120);
        assert_eq!(payload_ttl_for(300), 600);
    }

    #[test]
    fn prune_keeps_entry_within_interval_ttl() {
        let dir = TempDir::new().unwrap();
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let aged = Instant::now() - std::time::Duration::from_secs(200);
        parser.cache.lock().unwrap().insert(
            String::from("view"),
            PayloadCacheEntry {
                payload: UsagePayload::default(),
                stored_at: aged,
                last_accessed_at: aged,
            },
        );

        parser.set_payload_ttl_secs(600);
        assert!(
            parser.check_cache("view").is_some(),
            "a 200 s old entry is live under a 600 s TTL"
        );
        parser.set_payload_ttl_secs(120);
        assert!(
            parser.check_cache("view").is_none(),
            "and expired under a 120 s TTL"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Monthly aggregation
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn monthly_aggregation_groups_by_month() {
        let content = r#"{"type":"assistant","timestamp":"2026-01-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500}}}
{"type":"assistant","timestamp":"2026-02-10T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":2000,"output_tokens":1000}}}
{"type":"assistant","timestamp":"2026-03-05T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":3000,"output_tokens":1500}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);
        let payload = parser.get_monthly("claude", "20260101");

        assert_eq!(
            payload.chart_buckets.len(),
            3,
            "should have 3 month buckets"
        );
        let labels: Vec<&str> = payload
            .chart_buckets
            .iter()
            .map(|b| b.label.as_str())
            .collect();
        assert!(labels.contains(&"Jan"), "should have Jan bucket");
        assert!(labels.contains(&"Feb"), "should have Feb bucket");
        assert!(labels.contains(&"Mar"), "should have Mar bucket");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Hourly aggregation
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn hourly_aggregation_groups_by_hour() {
        let target_date = Local::now().date_naive() - chrono::Duration::days(1);
        let ts1 = target_date
            .and_hms_opt(9, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .to_rfc3339();
        let ts2 = target_date
            .and_hms_opt(10, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .to_rfc3339();
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{ts1}","message":{{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":1000,"output_tokens":500}}}}}}
{{"type":"assistant","timestamp":"{ts2}","message":{{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":2000,"output_tokens":1000}}}}}}"#,
        );

        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("session.jsonl"), &content);
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());

        let target_day = target_date.format("%Y%m%d").to_string();
        let payload = parser.get_hourly("claude", &target_day);

        // Should have buckets covering from min_hour to current_hour
        assert!(
            !payload.chart_buckets.is_empty(),
            "should produce chart buckets"
        );
        let two_hours_ago_label = format_hour(9);
        let has_bucket = payload
            .chart_buckets
            .iter()
            .any(|b| b.label == two_hours_ago_label);
        assert!(has_bucket, "should have a bucket for 2 hours ago");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Blocks aggregation
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn blocks_detects_activity_windows() {
        // Two entries more than 30 minutes apart -> 2 blocks
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T09:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500}}}
{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":2000,"output_tokens":1000}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);
        let payload = parser.get_blocks("claude", "20260315");

        assert_eq!(
            payload.chart_buckets.len(),
            2,
            "entries >30 min apart should produce 2 activity blocks"
        );
    }

    #[test]
    fn inactive_last_block_returns_no_active_block_and_uses_total_cost() {
        let end = Local::now() - chrono::Duration::minutes(40);
        let start = end - chrono::Duration::minutes(10);
        let since = start.date_naive().format("%Y%m%d").to_string();
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":1000,"output_tokens":500}}}}}}
{{"type":"assistant","timestamp":"{}","message":{{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":500,"output_tokens":250}}}}}}"#,
            start.to_rfc3339(),
            end.to_rfc3339()
        );
        let (_dir, parser) = make_parser_with_claude_data(&content);
        let payload = parser.get_blocks("claude", &since);

        assert!(payload.active_block.is_none());
        assert!((payload.five_hour_cost - payload.total_cost).abs() < f64::EPSILON);
        assert!(payload.total_cost > 0.0);
    }

    #[test]
    fn time_range_keeps_cross_midnight_entries_inside_window() {
        let now = Local::now();
        let reset = now + chrono::Duration::hours(2);
        let start = reset - chrono::Duration::hours(5);
        let inside = start + chrono::Duration::minutes(30);
        let outside = start - chrono::Duration::minutes(1);
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":1000,"output_tokens":500}}}}}}
{{"type":"assistant","timestamp":"{}","message":{{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":2000,"output_tokens":1000}}}}}}"#,
            outside.to_rfc3339(),
            inside.to_rfc3339(),
        );
        let (_dir, parser) = make_parser_with_claude_data(&content);
        let payload = parser.get_time_range("claude", start, reset);

        assert_eq!(payload.session_count, 1);
        assert!(payload.total_cost > 0.0);
        assert!((payload.five_hour_cost - payload.total_cost).abs() < f64::EPSILON);
        assert!(payload.active_block.is_some());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Hourly aggregation — past day
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn get_hourly_past_day_returns_24_buckets() {
        let dir = TempDir::new().unwrap();
        // Build a timestamp at 9AM local on a past day, using that day's correct UTC offset
        let target_date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let naive_dt = target_date.and_hms_opt(9, 0, 0).unwrap();
        let local_dt = naive_dt.and_local_timezone(Local).unwrap();
        let ts = local_dt.to_rfc3339();
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":50}}}}}}"#,
            ts
        );
        write_file(&dir.path().join("session.jsonl"), &content);
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let payload = parser.get_hourly("claude", "20260115");
        assert_eq!(
            payload.chart_buckets.len(),
            24,
            "past day should have 24 hourly buckets"
        );
        let nine_am = payload
            .chart_buckets
            .iter()
            .find(|b| b.label == "9AM")
            .unwrap();
        assert!(nine_am.total > 0.0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // has_entries_before
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn has_entries_before_claude_returns_true_when_old_entries_exist() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-01-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        assert!(parser.has_entries_before("claude", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
    }

    #[test]
    fn has_entries_before_claude_returns_false_when_no_old_entries() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        assert!(!parser.has_entries_before("claude", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
    }

    #[test]
    fn has_entries_before_codex_returns_true_when_old_entries_exist() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("workspace").join("old");
        fs::create_dir_all(&session_dir).unwrap();
        write_file(
            &session_dir.join("session.jsonl"),
            r#"{"type":"event_msg","timestamp":"2026-01-15T12:00:00+00:00","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":50}}}}"#,
        );
        let parser = UsageParser::with_codex_dir(dir.path().to_path_buf());
        assert!(parser.has_entries_before("codex", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
    }

    #[test]
    fn has_entries_before_codex_returns_false_when_no_old_entries() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("workspace").join("recent");
        fs::create_dir_all(&session_dir).unwrap();
        write_file(
            &session_dir.join("session.jsonl"),
            r#"{"type":"event_msg","timestamp":"2026-03-15T12:00:00+00:00","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":50}}}}"#,
        );
        let parser = UsageParser::with_codex_dir(dir.path().to_path_buf());
        assert!(!parser.has_entries_before("codex", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
    }

    #[test]
    fn has_entries_before_empty_dir_returns_false() {
        let dir = TempDir::new().unwrap();
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        assert!(!parser.has_entries_before("claude", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Change event parsing (Edit / Write tool_use)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn count_lines_helper() {
        use crate::usage::claude_parser::test_count_lines as count_lines;
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("one"), 1);
        assert_eq!(count_lines("one\ntwo"), 2);
        assert_eq!(count_lines("one\ntwo\nthree"), 3);
    }

    #[test]
    fn parse_claude_edit_tool_result_prefers_structured_patch_counts() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Edit","input":{"file_path":"src/main.rs","old_string":"let a = 1;\nlet b = 2;","new_string":"let a = 1;\nlet b = 3;\nlet c = 4;"}}],"usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"user","timestamp":"2026-03-21T10:00:01+00:00","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_1","content":"Applied patch"}]},"toolUseResult":{"filePath":"src/main.rs","oldString":"let a = 1;\nlet b = 2;","newString":"let a = 1;\nlet b = 3;\nlet c = 4;","structuredPatch":[{"lines":["@@"," let a = 1;","-let b = 2;","+let b = 3;","+let c = 4;"]}]}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let (entries, change_events, _, _) =
            parse_claude_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(entries.len(), 1);
        assert_eq!(change_events.len(), 1);

        let cev = &change_events[0];
        assert_eq!(cev.path, "src/main.rs");
        assert_eq!(cev.model, "opus-4-6");
        assert_eq!(cev.provider, "claude");
        assert_eq!(cev.kind, ChangeEventKind::PatchEdit);
        assert_eq!(cev.removed_lines, 1);
        assert_eq!(cev.added_lines, 2);
        assert_eq!(cev.category, FileCategory::Code);
    }

    #[test]
    fn parse_claude_write_tool_result_emits_change_event_with_line_counts() {
        let dir = TempDir::new().unwrap();
        let content = concat!(
            "{\"type\":\"assistant\",\"timestamp\":\"2026-03-21T10:00:00+00:00\",\"requestId\":\"req_1\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet-4-6-20260301\",\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"Write\",\"input\":{\"file_path\":\"docs/README.md\",\"content\":\"# Hello\\nWorld\\nAgain\"}}],\"usage\":{\"input_tokens\":100,\"output_tokens\":50}}}",
            "\n",
            "{\"type\":\"user\",\"timestamp\":\"2026-03-21T10:00:01+00:00\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"tu_1\",\"content\":\"Wrote file\"}]},\"toolUseResult\":{\"filePath\":\"docs/README.md\",\"content\":\"# Hello\\nWorld\\nAgain\",\"originalFile\":\"# Hello\\nWorld\",\"structuredPatch\":[{\"lines\":[\"@@\",\" # Hello\",\" World\",\"+Again\"]}]}}"
        );
        write_file(&dir.path().join("session.jsonl"), content);

        let (entries, change_events, _, _) =
            parse_claude_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(entries.len(), 1);
        assert_eq!(change_events.len(), 1);

        let cev = &change_events[0];
        assert_eq!(cev.path, "docs/README.md");
        assert_eq!(cev.model, "sonnet-4-6");
        assert_eq!(cev.kind, ChangeEventKind::FullWrite);
        assert_eq!(cev.added_lines, 1);
        assert_eq!(cev.removed_lines, 0);
        assert_eq!(cev.category, FileCategory::Docs);
    }

    #[test]
    fn parse_claude_unresolved_write_tool_use_falls_back_to_zero_change_count() {
        let dir = TempDir::new().unwrap();
        let content = "{\"type\":\"assistant\",\"timestamp\":\"2026-03-21T10:00:00+00:00\",\"requestId\":\"req_1\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet-4-6-20260301\",\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"Write\",\"input\":{\"file_path\":\"docs/README.md\",\"content\":\"# Hello\\nWorld\"}}],\"usage\":{\"input_tokens\":100,\"output_tokens\":50}}}";
        write_file(&dir.path().join("session.jsonl"), content);

        let (_entries, change_events, _, _) =
            parse_claude_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(change_events.len(), 1);
        assert_eq!(change_events[0].kind, ChangeEventKind::FullWrite);
        assert_eq!(change_events[0].added_lines, 0);
        assert_eq!(change_events[0].removed_lines, 0);
    }

    #[test]
    fn parse_claude_multiple_tool_uses_in_one_message() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Edit","input":{"file_path":"src/a.rs","old_string":"a","new_string":"b\nc"}},{"type":"tool_use","id":"tu_2","name":"Edit","input":{"file_path":"src/b.rs","old_string":"x\ny","new_string":"z"}},{"type":"text","text":"Done"}],"usage":{"input_tokens":100,"output_tokens":50}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let (_entries, change_events, _, _) =
            parse_claude_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(change_events.len(), 2);

        assert_eq!(change_events[0].path, "src/a.rs");
        assert_eq!(change_events[0].removed_lines, 1);
        assert_eq!(change_events[0].added_lines, 2);

        assert_eq!(change_events[1].path, "src/b.rs");
        assert_eq!(change_events[1].removed_lines, 2);
        assert_eq!(change_events[1].added_lines, 1);
    }

    #[test]
    fn parse_claude_skips_provider_internal_paths() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Write","input":{"file_path":"/home/user/.claude/plans/plan_123.md","content":"step 1"}},{"type":"tool_use","id":"tu_2","name":"Edit","input":{"file_path":"src/real.rs","old_string":"old","new_string":"new"}}],"usage":{"input_tokens":100,"output_tokens":50}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let (_entries, change_events, _, _) =
            parse_claude_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(change_events.len(), 1);
        assert_eq!(change_events[0].path, "src/real.rs");
    }

    #[test]
    fn change_events_flow_through_cached_load() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Edit","input":{"file_path":"src/main.rs","old_string":"fn old()","new_string":"fn new()"}}],"usage":{"input_tokens":100,"output_tokens":50}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let (entries, change_events, _reports) = parser.load_claude_entries_with_debug(None);
        assert_eq!(entries.len(), 1);
        assert_eq!(change_events.len(), 1);
        assert_eq!(change_events[0].path, "src/main.rs");

        // Second call should come from cache and still have change events
        let (_entries2, change_events2, _reports2) = parser.load_claude_entries_with_debug(None);
        assert_eq!(change_events2.len(), 1);
        assert_eq!(change_events2[0].path, "src/main.rs");
    }

    #[test]
    fn change_events_filtered_by_since_date() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-01-01T10:00:00+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Edit","input":{"file_path":"src/old.rs","old_string":"a","new_string":"b"}}],"usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","requestId":"req_2","message":{"id":"msg_2","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_2","name":"Edit","input":{"file_path":"src/new.rs","old_string":"c","new_string":"d"}}],"usage":{"input_tokens":100,"output_tokens":50}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let since = parse_since_date("20260301");
        let (_entries, change_events, _reports) = parser.load_claude_entries_with_debug(since);
        assert_eq!(change_events.len(), 1);
        assert_eq!(change_events[0].path, "src/new.rs");
    }

    #[test]
    fn load_claude_entries_dedupes_change_events_across_roots() {
        let dir_a = TempDir::new().unwrap();
        let dir_b = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Edit","input":{"file_path":"src/main.rs","old_string":"old","new_string":"new\nextra"}}],"usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"user","timestamp":"2026-03-21T10:00:01+00:00","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_1","content":"Applied patch"}]},"toolUseResult":{"filePath":"src/main.rs","structuredPatch":[{"lines":["@@","-old","+new","+extra"]}]}}"#;
        write_file(&dir_a.path().join("session.jsonl"), content);
        write_file(&dir_b.path().join("session.jsonl"), content);

        let parser = UsageParser::with_claude_dirs(vec![
            dir_a.path().to_path_buf(),
            dir_b.path().to_path_buf(),
        ]);
        let (entries, change_events, _reports) = parser.load_claude_entries_with_debug(None);

        assert_eq!(entries.len(), 1);
        assert_eq!(change_events.len(), 1);
        assert_eq!(change_events[0].path, "src/main.rs");
        assert_eq!(change_events[0].added_lines, 2);
        assert_eq!(change_events[0].removed_lines, 1);
    }

    #[test]
    fn load_claude_entries_keeps_distinct_tool_use_change_events_for_same_request() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Edit","input":{"file_path":"src/a.rs","old_string":"old-a","new_string":"new-a"}}],"usage":{"input_tokens":100,"output_tokens":10}}}
{"type":"user","timestamp":"2026-03-21T10:00:01+00:00","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_1","content":"Applied patch"}]},"toolUseResult":{"filePath":"src/a.rs","structuredPatch":[{"lines":["@@","-old-a","+new-a"]}]}}
{"type":"assistant","timestamp":"2026-03-21T10:00:02+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_2","name":"Edit","input":{"file_path":"src/b.rs","old_string":"old-b","new_string":"new-b\nextra-b"}}],"usage":{"input_tokens":100,"output_tokens":20}}}
{"type":"user","timestamp":"2026-03-21T10:00:03+00:00","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_2","content":"Applied patch"}]},"toolUseResult":{"filePath":"src/b.rs","structuredPatch":[{"lines":["@@","-old-b","+new-b","+extra-b"]}]}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let (entries, change_events, _reports) = parser.load_claude_entries_with_debug(None);

        assert_eq!(
            entries.len(),
            1,
            "usage entries should still dedupe by request"
        );
        assert_eq!(change_events.len(), 2);
        assert_eq!(change_events[0].path, "src/a.rs");
        assert_eq!(change_events[1].path, "src/b.rs");
    }

    #[test]
    fn load_claude_entries_prefers_main_scope_for_mirrored_change_events() {
        // A message mirrored into a sidechain file with identical usage is the
        // main agent's own turn (the sidechain copy is the spawn record), so the
        // root copy wins an exact tie.
        let dir = TempDir::new().unwrap();
        let root = r#"{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","sessionId":"sess-1","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Edit","input":{"file_path":"src/main.rs","old_string":"old","new_string":"new"}}],"usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"user","timestamp":"2026-03-21T10:00:01+00:00","sessionId":"sess-1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_1","content":"Applied patch"}]},"toolUseResult":{"filePath":"src/main.rs","structuredPatch":[{"lines":["@@","-old","+new"]}]}}"#;
        let sidechain = r#"{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","isSidechain":true,"agentId":"agt-1","sessionId":"sess-1","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Edit","input":{"file_path":"src/main.rs","old_string":"old","new_string":"new"}}],"usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"user","timestamp":"2026-03-21T10:00:01+00:00","isSidechain":true,"agentId":"agt-1","sessionId":"sess-1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_1","content":"Applied patch"}]},"toolUseResult":{"filePath":"src/main.rs","structuredPatch":[{"lines":["@@","-old","+new"]}]}}"#;
        write_file(&dir.path().join("root.jsonl"), root);
        write_file(&dir.path().join("sidechain.jsonl"), sidechain);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let (entries, change_events, _reports) = parser.load_claude_entries_with_debug(None);

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].agent_scope,
            crate::stats::subagent::AgentScope::Main
        );
        assert_eq!(change_events.len(), 1);
        assert_eq!(
            change_events[0].agent_scope,
            crate::stats::subagent::AgentScope::Main
        );
    }

    #[test]
    fn no_content_field_produces_no_change_events() {
        let dir = TempDir::new().unwrap();
        // A normal assistant message with no content array (usage only)
        let content = r#"{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","message":{"model":"claude-opus-4-6-20260301","usage":{"input_tokens":100,"output_tokens":50}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let (_entries, change_events, _, _) =
            parse_claude_session_file(&dir.path().join("session.jsonl"));
        assert!(change_events.is_empty());
    }

    #[test]
    fn is_provider_internal_path_detects_plans() {
        use crate::usage::claude_parser::test_is_provider_internal_path as is_provider_internal_path;
        assert!(is_provider_internal_path(
            "/home/user/.claude/plans/plan_abc.md"
        ));
        assert!(is_provider_internal_path(
            "/Users/foo/.claude/plans/something"
        ));
        assert!(!is_provider_internal_path("src/main.rs"));
        assert!(!is_provider_internal_path(
            "/home/user/.claude/projects/foo.jsonl"
        ));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Codex apply_patch change event parsing
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn count_diff_lines_basic() {
        let patch = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!(\"old\");
+    println!(\"new\");
+    println!(\"extra\");
 }";
        let (added, removed) = count_diff_lines(patch);
        assert_eq!(added, 2);
        assert_eq!(removed, 1);
    }

    #[test]
    fn count_diff_lines_ignores_header_lines() {
        let patch = "\
--- a/foo.rs
+++ b/foo.rs
+added line";
        let (added, removed) = count_diff_lines(patch);
        assert_eq!(added, 1);
        assert_eq!(removed, 0);
    }

    #[test]
    fn extract_diff_paths_from_plus_plus_plus_b() {
        let patch = "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -1 +1 @@
-old
+new";
        let paths = extract_diff_paths(patch);
        assert_eq!(paths, vec!["src/main.rs"]);
    }

    #[test]
    fn extract_diff_paths_from_diff_git_header() {
        let patch = "diff --git a/src/lib.rs b/src/lib.rs\nindex abc..def 100644";
        let paths = extract_diff_paths(patch);
        assert_eq!(paths, vec!["src/lib.rs"]);
    }

    #[test]
    fn extract_diff_paths_skips_dev_null() {
        let patch = "\
--- /dev/null
+++ b/src/new_file.rs
+content";
        let paths = extract_diff_paths(patch);
        assert_eq!(paths, vec!["src/new_file.rs"]);
    }

    #[test]
    fn parse_codex_apply_patch_emits_change_event() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("workspace");
        fs::create_dir_all(&session_dir).unwrap();

        let ts = "2026-03-21T10:00:00+00:00";
        let content = format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp/demo","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"function_call","name":"apply_patch","arguments":"--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,4 @@\n fn main() {{\n-    old();\n+    new();\n+    extra();\n }}"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50}}}}}}}}"#,
            ts = ts
        );
        write_file(&session_dir.join("session.jsonl"), &content);

        let (_entries, change_events, _, _) =
            parse_codex_session_file(&session_dir.join("session.jsonl"));
        assert_eq!(change_events.len(), 1);

        let cev = &change_events[0];
        assert_eq!(cev.path, "src/main.rs");
        assert_eq!(cev.provider, "codex");
        assert_eq!(cev.model, "gpt-5.4");
        assert_eq!(cev.kind, ChangeEventKind::PatchEdit);
        assert_eq!(cev.added_lines, 2);
        assert_eq!(cev.removed_lines, 1);
        assert_eq!(cev.category, FileCategory::Code);
    }

    #[test]
    fn parse_codex_apply_patch_with_custom_tool_call() {
        let dir = TempDir::new().unwrap();

        let ts = "2026-03-21T10:00:00+00:00";
        let content = format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"o3-2025-04-16"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"custom_tool_call","name":"apply_patch","arguments":"--- a/config.yaml\n+++ b/config.yaml\n@@ -1 +1,2 @@\n key: old\n+key2: new"}}}}"#,
            ts = ts
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let (_entries, change_events, _, _) =
            parse_codex_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(change_events.len(), 1);

        let cev = &change_events[0];
        assert_eq!(cev.path, "config.yaml");
        assert_eq!(cev.model, "o3-2025-04-16");
        assert_eq!(cev.added_lines, 1);
        assert_eq!(cev.removed_lines, 0);
        assert_eq!(cev.category, FileCategory::Config);
    }

    #[test]
    fn parse_codex_apply_patch_flows_through_load_entries() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("workspace");
        fs::create_dir_all(&session_dir).unwrap();

        let ts = "2026-03-21T10:00:00+00:00";
        let content = format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"function_call","name":"apply_patch","arguments":"--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50}}}}}}}}"#,
            ts = ts
        );
        write_file(&session_dir.join("session.jsonl"), &content);

        let parser = UsageParser::with_codex_dir(dir.path().to_path_buf());
        let (_entries, change_events, _reports) =
            parser.load_entries("codex", parse_since_date("20260301"));
        assert_eq!(change_events.len(), 1);
        assert_eq!(change_events[0].path, "src/lib.rs");
        assert_eq!(change_events[0].provider, "codex");

        // Second call should come from cache and still have change events
        let (_entries2, change_events2, _reports2) =
            parser.load_entries("codex", parse_since_date("20260301"));
        assert_eq!(change_events2.len(), 1);
        assert_eq!(change_events2[0].path, "src/lib.rs");
    }

    #[test]
    fn codex_change_events_merge_in_all_provider() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();

        // Claude edit
        let claude_content = r#"{"type":"assistant","timestamp":"2026-03-21T10:00:00+00:00","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Edit","input":{"file_path":"src/a.rs","old_string":"a","new_string":"b"}}],"usage":{"input_tokens":100,"output_tokens":50}}}"#;
        write_file(&claude_dir.path().join("session.jsonl"), claude_content);

        // Codex apply_patch
        let ts = "2026-03-21T10:00:00+00:00";
        let codex_content = format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"function_call","name":"apply_patch","arguments":"--- a/src/b.rs\n+++ b/src/b.rs\n@@ -1 +1 @@\n-x\n+y"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50}}}}}}}}"#,
            ts = ts
        );
        write_file(&codex_dir.path().join("session.jsonl"), &codex_content);

        let parser = UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        );
        let (_entries, change_events, _reports) =
            parser.load_entries("all", parse_since_date("20260301"));
        assert_eq!(change_events.len(), 2);

        let providers: Vec<&str> = change_events.iter().map(|e| e.provider.as_str()).collect();
        assert!(providers.contains(&"claude"));
        assert!(providers.contains(&"codex"));
    }

    #[test]
    fn parse_codex_response_item_apply_patch() {
        // Newer Codex CLI emits apply_patch as "response_item" with "input" field
        // instead of "event_msg" with "arguments" field.
        let dir = TempDir::new().unwrap();

        let ts = "2026-03-21T10:00:00+00:00";
        let content = format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"response_item","timestamp":"{ts}","payload":{{"type":"custom_tool_call","status":"completed","name":"apply_patch","input":"*** Begin Patch\n*** Update File: /Users/test/project/src/main.rs\n@@\n-old_line\n+new_line\n+added_line"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50}}}}}}}}"#,
            ts = ts
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let (_entries, change_events, _, _) =
            parse_codex_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(change_events.len(), 1);

        let cev = &change_events[0];
        assert_eq!(cev.path, "/Users/test/project/src/main.rs");
        assert_eq!(cev.model, "gpt-5.4");
        assert_eq!(cev.added_lines, 2);
        assert_eq!(cev.removed_lines, 1);
        assert_eq!(cev.category, FileCategory::Code);

        // Token entries should still be parsed
        assert_eq!(_entries.len(), 1);
    }

    #[test]
    fn extract_diff_paths_from_codex_patch_format() {
        let patch = "*** Begin Patch\n*** Add File: /Users/test/project/src/new.rs\n+fn main() {}\n*** Update File: /Users/test/project/src/lib.rs\n@@\n-old\n+new";
        let paths = extract_diff_paths(patch);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], "/Users/test/project/src/new.rs");
        assert_eq!(paths[1], "/Users/test/project/src/lib.rs");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Claude subagent scope attribution
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn claude_root_session_defaults_to_main_scope() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","sessionId":"sess-1","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let entries = read_claude_entries(dir.path(), None);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].agent_scope,
            crate::stats::subagent::AgentScope::Main
        );
        assert!(
            entries[0].session_key.contains("main"),
            "session_key should contain 'main', got: {}",
            entries[0].session_key
        );
    }

    #[test]
    fn claude_sidechain_entry_maps_to_subagent_scope() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","isSidechain":true,"agentId":"a1b2c3d","sessionId":"sess-1","message":{"model":"claude-haiku-4-5","stop_reason":"end_turn","usage":{"input_tokens":50,"output_tokens":20}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let entries = read_claude_entries(dir.path(), None);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].agent_scope,
            crate::stats::subagent::AgentScope::Subagent
        );
        assert!(
            entries[0].session_key.contains("a1b2c3d"),
            "session_key should contain agentId, got: {}",
            entries[0].session_key
        );
    }

    #[test]
    fn claude_dedupe_collapses_root_and_sidechain_and_prefers_main_scope() {
        let dir = TempDir::new().unwrap();
        // Root and sidechain with same message.id and requestId
        let root = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","sessionId":"sess-1","requestId":"req-1","message":{"id":"msg-1","model":"claude-opus-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
        let sidechain = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:01+00:00","isSidechain":true,"agentId":"agt-1","sessionId":"sess-1","requestId":"req-1","message":{"id":"msg-1","model":"claude-opus-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
        write_file(&dir.path().join("root.jsonl"), root);
        write_file(&dir.path().join("sidechain.jsonl"), sidechain);

        let entries = read_claude_entries(dir.path(), None);
        assert_eq!(
            entries.len(),
            1,
            "root and sidechain mirrors should collapse"
        );
        assert_eq!(
            entries[0].agent_scope,
            crate::stats::subagent::AgentScope::Main
        );
        assert!(
            !entries[0].session_key.contains("agt-1"),
            "main-agent mirror should keep the root session_key"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Codex subagent scope attribution
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn codex_no_session_meta_defaults_to_main() {
        let dir = TempDir::new().unwrap();
        let ts = Local::now().format("%Y-%m-%dT12:00:00+00:00").to_string();
        let content = format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50}}}}}}}}"#
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let (entries, _, _, _) = parse_codex_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].agent_scope,
            crate::stats::subagent::AgentScope::Main
        );
    }

    #[test]
    fn codex_session_meta_with_subagent_other_maps_to_subagent() {
        let dir = TempDir::new().unwrap();
        let ts = Local::now().format("%Y-%m-%dT12:00:00+00:00").to_string();
        let content = format!(
            r#"{{"type":"session_meta","payload":{{"id":"sess-abc","source":{{"subagent":{{"other":"guardian"}}}}}}}}
{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50}}}}}}}}"#
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let (entries, _, _, _) = parse_codex_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].agent_scope,
            crate::stats::subagent::AgentScope::Subagent
        );
        assert_eq!(entries[0].session_key, "codex:sess-abc");
    }

    #[test]
    fn codex_session_meta_with_thread_spawn_maps_to_subagent() {
        let dir = TempDir::new().unwrap();
        let ts = Local::now().format("%Y-%m-%dT12:00:00+00:00").to_string();
        let content = format!(
            r#"{{"type":"session_meta","payload":{{"id":"sess-xyz","source":{{"subagent":{{"thread_spawn":{{"parent_thread_id":"parent-1","depth":1}}}}}}}}}}
{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":200,"output_tokens":80}}}}}}}}"#
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let (entries, _, _, _) = parse_codex_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].agent_scope,
            crate::stats::subagent::AgentScope::Subagent
        );
        assert_eq!(entries[0].session_key, "codex:sess-xyz");
    }

    #[test]
    fn codex_all_entries_in_file_share_same_session_key() {
        let dir = TempDir::new().unwrap();
        let ts1 = Local::now().format("%Y-%m-%dT12:00:00+00:00").to_string();
        let ts2 = Local::now().format("%Y-%m-%dT12:05:00+00:00").to_string();
        let content = format!(
            r#"{{"type":"session_meta","payload":{{"id":"sess-shared"}}}}
{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts1}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50}}}}}}}}
{{"type":"event_msg","timestamp":"{ts2}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":200,"output_tokens":80}}}}}}}}"#
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let (entries, _, _, _) = parse_codex_session_file(&dir.path().join("session.jsonl"));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].session_key, entries[1].session_key);
        assert_eq!(entries[0].session_key, "codex:sess-shared");
    }
}

#[cfg(test)]
mod debug_compare {
    use super::*;

    fn print_provider(label: &str, entries: &[ParsedEntry]) {
        let mut model_totals: std::collections::HashMap<String, (u64, u64, u64, u64, usize, f64)> =
            std::collections::HashMap::new();
        for e in entries {
            let (_, key) = normalize_model(&e.model);
            let cost = crate::usage::pricing::calculate_cost(
                &e.model,
                e.input_tokens,
                e.output_tokens,
                e.cache_creation_5m_tokens,
                e.cache_creation_1h_tokens,
                e.cache_read_tokens,
                e.web_search_requests,
            );
            let m = model_totals.entry(key).or_default();
            m.0 += e.input_tokens;
            m.1 += e.output_tokens;
            m.2 += e.cache_creation_5m_tokens + e.cache_creation_1h_tokens;
            m.3 += e.cache_read_tokens;
            m.4 += 1;
            m.5 += cost;
        }
        println!(
            "\n=== {}: Our parser ({} entries) ===",
            label,
            entries.len()
        );
        let mut total_tok = 0u64;
        let mut total_cost = 0.0f64;
        for (model, (inp, out, cw, cr, count, cost)) in &model_totals {
            let t = inp + out + cw + cr;
            total_tok += t;
            total_cost += *cost;
            println!(
                "  {}: inp={} out={} cw={} cr={} total={} n={} cost=${:.6}",
                model, inp, out, cw, cr, t, count, cost
            );
        }
        println!("  TOTAL: tokens={} cost=${:.6}", total_tok, total_cost);
    }

    #[test]
    fn compare_all_with_ccusage() {
        let parser = UsageParser::new();
        let today = chrono::Local::now().format("%Y%m%d").to_string();

        let (claude, _, _) = parser.load_entries("claude", Some(parse_since_date(&today).unwrap()));
        print_provider("CLAUDE", &claude);
        println!("\n=== CLAUDE: ccusage ===");
        println!("  opus:   inp=19,875 out=129,193 cw=3,180,937 cr=74,758,016 total=78,088,021 cost=$65.768004");
        println!(
            "  haiku:  inp=3,354 out=28,909 cw=612,190 cr=4,675,714 total=5,320,167 cost=$1.380708"
        );
        println!(
            "  sonnet: inp=60 out=4,597 cw=124,968 cr=2,128,900 total=2,258,525 cost=$1.176435"
        );
        println!("  TOTAL: tokens=85,666,713 cost=$68.325146");

        let (codex, _, _) = parser.load_entries("codex", Some(parse_since_date(&today).unwrap()));
        print_provider("CODEX", &codex);
        println!("\n=== CODEX: ccusage ===");
        println!("  gpt-5.4: inp=231,247 out=7,338 reasoning=5,997 total=238,585 cost=$0.277788");
        println!("  (reasoning is informational; both parsers bill against token_count usage)");
    }
}

#[cfg(test)]
mod path_a_smoke {
    //! Manual smoke probe for "Path A" — can we authenticate against
    //! Cursor's remote APIs using the access token that Cursor IDE itself
    //! stores locally in `state.vscdb`, instead of asking the user to
    //! manually copy `WorkosCursorSessionToken` out of cursor.com cookies?
    //!
    //! If the dashboard endpoint accepts `Authorization: Bearer <token>`
    //! where `<token>` comes from `cursorAuth/accessToken`, we can offer
    //! a zero-configuration Cursor integration: install TokenMonitor →
    //! it picks up the IDE's session automatically. If not, we fall back
    //! to "Path B" (in-app webview login).
    //!
    //! This test:
    //!   • is `#[ignore]` because it requires a logged-in Cursor IDE on
    //!     the host AND hits the real cursor.com / api.cursor.com servers;
    //!   • never asserts (so all four probes run regardless of which one
    //!     succeeds — useful for one-shot diagnosis);
    //!   • redacts the access token before printing.
    //!
    //! Run with:
    //! ```bash
    //! cargo test --lib path_a_smoke -- --ignored --nocapture
    //! ```

    use crate::usage::cursor_parser::*;
    use std::time::Duration;

    fn redact(token: &str) -> String {
        if token.len() <= 16 {
            format!("[short, {} chars]", token.len())
        } else {
            format!(
                "{}…{} ({} chars, {} JWT-style segments)",
                &token[..8],
                &token[token.len() - 8..],
                token.len(),
                token.matches('.').count() + 1,
            )
        }
    }

    fn print_response(label: &str, resp: reqwest::blocking::Response) {
        let status = resp.status();
        let headers_summary = format!(
            "content-type={:?} content-length={:?}",
            resp.headers().get("content-type"),
            resp.headers().get("content-length"),
        );
        let body = resp
            .text()
            .unwrap_or_else(|e| format!("[body read error: {e}]"));
        let preview_len = body.len().min(800);
        eprintln!("\n=== {label} ===");
        eprintln!("status:  {status}");
        eprintln!("headers: {headers_summary}");
        eprintln!("body (first {preview_len} chars):");
        eprintln!("{}", &body[..preview_len]);
        if body.len() > preview_len {
            eprintln!("[... {} more chars omitted ...]", body.len() - preview_len);
        }
    }

    #[test]
    #[ignore = "manual: requires a logged-in Cursor IDE on host + real network"]
    fn probe_cursor_ide_access_token_against_remote_endpoints() {
        let Some(db_path) = cursor_global_state_path_from_env()
            .or_else(crate::paths::cursor_global_state_vscdb_default)
        else {
            eprintln!("Could not locate state.vscdb on this host. Is Cursor IDE installed?");
            return;
        };
        eprintln!("state.vscdb: {}", db_path.display());

        let access_token =
            match read_cursor_state_value_from_sqlite3(&db_path, "cursorAuth/accessToken") {
                Ok(Some(t)) => t,
                Ok(None) => {
                    eprintln!(
                        "cursorAuth/accessToken not present in {} — sign into Cursor IDE first.",
                        db_path.display()
                    );
                    return;
                }
                Err(e) => {
                    eprintln!("sqlite3 read failed: {e}");
                    return;
                }
            };
        let refresh_token =
            read_cursor_state_value_from_sqlite3(&db_path, "cursorAuth/refreshToken")
                .ok()
                .flatten();
        let email = read_cursor_cached_email();
        let subscription =
            read_cursor_state_value_from_sqlite3(&db_path, "cursorAuth/stripeMembershipType")
                .ok()
                .flatten();

        eprintln!("\n--- Local Cursor IDE state ---");
        eprintln!("email:                {email:?}");
        eprintln!("subscription:         {subscription:?}");
        eprintln!("access_token:         {}", redact(&access_token));
        eprintln!("refresh_token found:  {}", refresh_token.is_some());

        let payload = serde_json::json!({
            "page": 1,
            "pageSize": 5,
            "startDate": 0_i64,
            "endDate": chrono::Local::now().timestamp_millis(),
        });

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("client build");

        // Common browser-ish headers added on every probe so we don't
        // accidentally fail Origin/Referer-style WAF checks.
        let with_browser_headers = |req: reqwest::blocking::RequestBuilder| {
            req.header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .header("User-Agent", "TokenMonitor/smoke-test")
        };

        // Probe 1: THE big question — Bearer auth against the dashboard
        // endpoint that powers cursor.com/dashboard/usage in the browser.
        let resp = with_browser_headers(
            client
                .post("https://cursor.com/api/dashboard/get-filtered-usage-events")
                .bearer_auth(&access_token)
                .header("Origin", "https://cursor.com")
                .header("Referer", "https://cursor.com/dashboard"),
        )
        .json(&payload)
        .send();
        match resp {
            Ok(r) => print_response("Probe 1: Bearer @ cursor.com dashboard endpoint", r),
            Err(e) => eprintln!("\n=== Probe 1 ===\nERROR: {e}"),
        }

        // Probe 2: same endpoint but the access token in the
        // WorkosCursorSessionToken cookie slot. The expected cookie
        // format is `<userId>::<JWT>`, so this almost certainly fails;
        // included to rule out a permissive server-side parser.
        let resp = with_browser_headers(
            client
                .post("https://cursor.com/api/dashboard/get-filtered-usage-events")
                .header(
                    reqwest::header::COOKIE,
                    format!("WorkosCursorSessionToken={access_token}"),
                )
                .header("Origin", "https://cursor.com")
                .header("Referer", "https://cursor.com/dashboard"),
        )
        .json(&payload)
        .send();
        match resp {
            Ok(r) => print_response(
                "Probe 2: Cookie WorkosCursorSessionToken=<accessToken> @ dashboard endpoint",
                r,
            ),
            Err(e) => eprintln!("\n=== Probe 2 ===\nERROR: {e}"),
        }

        // Probe 3: Enterprise admin endpoint with Bearer. Almost certainly
        // 401/403 for non-Enterprise users (they don't have admin scope),
        // but useful as a control: confirms the token isn't *accidentally*
        // a valid admin key.
        let resp = with_browser_headers(
            client
                .post("https://api.cursor.com/teams/filtered-usage-events")
                .bearer_auth(&access_token),
        )
        .json(&payload)
        .send();
        match resp {
            Ok(r) => print_response("Probe 3: Bearer @ api.cursor.com admin endpoint", r),
            Err(e) => eprintln!("\n=== Probe 3 ===\nERROR: {e}"),
        }

        // Probe 4: sanity check — does the access token authenticate at
        // all? `/api/auth/me` is a generic user-info endpoint the IDE
        // itself calls. If THIS returns 200 but Probe 1 doesn't, the
        // dashboard endpoint specifically locks to cookie auth and we
        // need Path B. If THIS also 401s, the token might be stale or
        // the path/header convention is wrong on this account.
        let resp = with_browser_headers(
            client
                .get("https://cursor.com/api/auth/me")
                .bearer_auth(&access_token),
        )
        .send();
        match resp {
            Ok(r) => print_response("Probe 4: GET /api/auth/me with Bearer (sanity)", r),
            Err(e) => eprintln!("\n=== Probe 4 ===\nERROR: {e}"),
        }

        // ── Path A' probes — find IDE-Bearer-friendly usage endpoints ────
        //
        // The dashboard endpoint above forces WorkOS cookie auth, but Cursor
        // IDE itself displays in-app token counts and subscription state, so
        // *some* Bearer-friendly endpoint must exist. The four below are the
        // most likely candidates per community reverse-engineering of the
        // IDE's network traffic. If any returns 200 with usable data, we can
        // drop the cookie requirement entirely.

        // Probe 5: `auth/full_stripe_profile` is what the Cursor IDE
        // settings panel calls to render "Pro+ — $X used this month". If
        // it includes a per-event breakdown, we can use it as the primary
        // usage source for detailed view.
        let resp = with_browser_headers(
            client
                .get("https://api2.cursor.sh/auth/full_stripe_profile")
                .bearer_auth(&access_token),
        )
        .send();
        match resp {
            Ok(r) => print_response(
                "Probe 5: GET api2.cursor.sh/auth/full_stripe_profile with Bearer",
                r,
            ),
            Err(e) => eprintln!("\n=== Probe 5 ===\nERROR: {e}"),
        }

        // Probes 6-9 below target the *real* Connect-Web service the IDE
        // uses, recovered by grepping the bundled Cursor IDE JS:
        //   • Service:  `aiserver.v1.DashboardService`  (NOT UsageService)
        //   • Methods:  `GetCurrentPeriodUsage`, `GetFilteredUsageEvents`,
        //               `GetTokenUsage`, `GetUsageBasedPremiumRequests`,
        //               `GetPlanInfo`, `GetAggregatedUsageEvents`, …
        //   • Host:     `api2.cursor.sh` (Probe 5 confirmed Bearer-friendly)
        //               with `api3.cursor.sh` as a fallback host the bundle
        //               also references.
        // The Connect-Web HTTP/JSON dialect accepts plain JSON request
        // bodies; for messages with no required fields, `{}` is valid.

        let connect_post = |url: &str, body: &str| {
            client
                .post(url)
                .bearer_auth(&access_token)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .header("Connect-Protocol-Version", "1")
                .header("User-Agent", "TokenMonitor/smoke-test")
                .body(body.to_string())
                .send()
        };

        // Probe 6: GetCurrentPeriodUsage — the call the IDE makes on
        // every prefetch. Returns aggregate spend + plan info for the
        // current billing period, NOT per-event detail.
        match connect_post(
            "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage",
            "{}",
        ) {
            Ok(r) => print_response(
                "Probe 6: POST api2 DashboardService.GetCurrentPeriodUsage (Bearer)",
                r,
            ),
            Err(e) => eprintln!("\n=== Probe 6 ===\nERROR: {e}"),
        }

        // Probe 7: THE BIG ONE — GetFilteredUsageEvents over Bearer.
        // Same method name as the cookie endpoint but reached via the
        // IDE's Connect-Web RPC layer. If this returns 200 with detailed
        // events, we have a fully zero-config integration path.
        let detailed_body = serde_json::json!({
            "pageSize": 5,
            "page": 1,
            "startDate": "0",
            "endDate": chrono::Local::now().timestamp_millis().to_string(),
        })
        .to_string();
        match connect_post(
            "https://api2.cursor.sh/aiserver.v1.DashboardService/GetFilteredUsageEvents",
            &detailed_body,
        ) {
            Ok(r) => print_response(
                "Probe 7: POST api2 DashboardService.GetFilteredUsageEvents (Bearer)",
                r,
            ),
            Err(e) => eprintln!("\n=== Probe 7 ===\nERROR: {e}"),
        }

        // Probe 8: GetTokenUsage — per-token breakdown candidate.
        match connect_post(
            "https://api2.cursor.sh/aiserver.v1.DashboardService/GetTokenUsage",
            "{}",
        ) {
            Ok(r) => print_response(
                "Probe 8: POST api2 DashboardService.GetTokenUsage (Bearer)",
                r,
            ),
            Err(e) => eprintln!("\n=== Probe 8 ===\nERROR: {e}"),
        }

        // Probe 9: same big endpoint but on the api3 host the bundle
        // also references. If api2 is locked down but api3 isn't (or
        // vice versa), this catches it cheaply.
        match connect_post(
            "https://api3.cursor.sh/aiserver.v1.DashboardService/GetFilteredUsageEvents",
            &detailed_body,
        ) {
            Ok(r) => print_response(
                "Probe 9: POST api3 DashboardService.GetFilteredUsageEvents (Bearer)",
                r,
            ),
            Err(e) => eprintln!("\n=== Probe 9 ===\nERROR: {e}"),
        }

        eprintln!(
            "\n--- Interpretation guide ---\n\
             Probe 1 → 200 with `usageEvents`: GREAT. Path A works as-is. Wire up\n  \
                       a `CursorAuth::IdeBearer` variant and prime it from state.vscdb.\n\
             Probe 1 → 401/403 BUT Probe 4 → 200: token is valid but dashboard locks\n  \
                       to cookie auth. Path A blocked → fall back to Path B (webview).\n\
             Probe 1 → 401/403 AND Probe 4 → 401: token may be expired. Re-sign-in to\n  \
                       Cursor IDE (which forces a refresh) and re-run.\n\
             Probe 2 → 200: surprising; would mean the cookie value doesn't need the\n  \
                       `<userId>::<JWT>` format. Sanity-double-check before relying on it.\n\
             Probe 3 → 401/403: expected for non-Enterprise users.\n\
             ── Path A' (DashboardService over Bearer) ─────────────────────────────\n\
             Probe 5 → 200 with subscription JSON: confirmed Bearer works on api2.\n\
             Probe 6 → 200 with current-period usage: aggregate-only fallback, but\n  \
                       enough to render the existing TM 'monthly spend' UI silently.\n\
             Probe 7 → 200 with `usageEvents`: JACKPOT — silent zero-config detailed\n  \
                       events. Drop the cookie requirement, prime auth from state.vscdb.\n\
             Probe 7 → 401/403 BUT Probe 6 → 200: same service, different ACL. Detailed\n  \
                       events lock to admin/cookie auth. Use aggregate as 'better than\n  \
                       nothing' fallback when the user hasn't pasted a cookie.\n\
             Probe 8 → 200: token-level breakdown — could complement detailed events.\n\
             Probe 9 → 200: api3 is the real host (api2 redirects?) — pivot accordingly.\n"
        );
    }

    /// End-to-end smoke test of the production Path A integration: prime
    /// the IDE token from `state.vscdb`, then go through the same
    /// `fetch_cursor_remote_entries` code path that the live usage refresh
    /// uses. If this returns parsed entries, the integration is healthy
    /// from `state.vscdb` all the way through to `ParsedEntry`.
    #[test]
    #[ignore = "manual: requires logged-in Cursor IDE + real network"]
    fn ide_bearer_end_to_end_through_production_pipeline() {
        if !prime_ide_access_token() {
            eprintln!(
                "Could not prime IDE access token — Cursor IDE may not be installed/logged-in."
            );
            return;
        }

        let auth = resolve_cursor_auth().expect("resolve_cursor_auth should return IdeBearer");
        eprintln!("Resolved auth kind: {:?}", auth.kind());
        assert_eq!(
            auth.kind(),
            CursorAuthKind::IdeBearer,
            "no user-pasted secret should be present in this test run"
        );

        let result = fetch_cursor_remote_entries(None);
        match result {
            Ok(Some(entries)) => {
                eprintln!(
                    "Got {} parsed entries from production pipeline",
                    entries.len()
                );
                if let Some(first) = entries.first() {
                    eprintln!("First entry:");
                    eprintln!("  timestamp:    {}", first.timestamp);
                    eprintln!("  model:        {}", first.model);
                    eprintln!("  input:        {}", first.input_tokens);
                    eprintln!("  output:       {}", first.output_tokens);
                    eprintln!("  cache_read:   {}", first.cache_read_tokens);
                    eprintln!("  cache_write:  {}", first.cache_creation_1h_tokens);
                    eprintln!("  session_key:  {}", first.session_key);
                    assert_eq!(
                        first.session_key, "cursor-ide",
                        "entries should be tagged with the IDE-bearer session key"
                    );
                } else {
                    eprintln!("No entries — billing cycle may be empty.");
                }
            }
            Ok(None) => eprintln!("fetch_cursor_remote_entries returned Ok(None) — auth missing?"),
            Err(e) => eprintln!("ERROR: {e}"),
        }
    }
}

#[cfg(test)]
mod cursor_remote_cache_tests {
    //! Range-aware, non-consuming Cursor remote cache (cursor-global-cache-reuse):
    //! one fetch of the widest opened range serves every period view by filtering
    //! on the request's `since`, killing the old consume-once + narrow->wide race.
    use super::*;
    use chrono::{Local, NaiveDate, NaiveTime, TimeZone};

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn make_cursor_entry(day: NaiveDate) -> ParsedEntry {
        let naive_dt = day.and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
        let timestamp = Local.from_local_datetime(&naive_dt).single().unwrap();
        ParsedEntry {
            timestamp,
            model: "cursor-gpt".to_string(),
            input_tokens: 1,
            output_tokens: 1,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: 0,
            web_search_requests: 0,
            unique_hash: None,
            session_key: "test-cursor".to_string(),
            agent_scope: crate::stats::subagent::AgentScope::Main,
        }
    }

    fn make_cursor_entry_with_input(day: NaiveDate, input_tokens: u64) -> ParsedEntry {
        ParsedEntry {
            input_tokens,
            ..make_cursor_entry(day)
        }
    }

    fn input_tokens_by_day(entries: &[ParsedEntry]) -> Vec<(NaiveDate, u64)> {
        let mut days: Vec<_> = entries
            .iter()
            .map(|e| (e.timestamp.date_naive(), e.input_tokens))
            .collect();
        days.sort();
        days
    }

    fn covered_since(parser: &UsageParser) -> Option<NaiveDate> {
        parser
            .cursor_remote_cache
            .lock()
            .unwrap()
            .as_ref()
            .expect("cache is set")
            .covered_since
    }

    #[test]
    fn range_covers_only_when_request_is_a_subset() {
        let jan1 = date(2026, 1, 1);
        let jun1 = date(2026, 6, 1);
        // A wide cache (since=Jan1) covers a narrower request (since=Jun1).
        assert!(cursor_range_covers(Some(jan1), Some(jun1)));
        assert!(cursor_range_covers(Some(jan1), Some(jan1)));
        // A narrow cache (since=Jun1) cannot serve a wider request (since=Jan1).
        assert!(!cursor_range_covers(Some(jun1), Some(jan1)));
        // All-time cache covers anything; a bounded cache can't cover all-time.
        assert!(cursor_range_covers(None, Some(jan1)));
        assert!(!cursor_range_covers(Some(jan1), None));
    }

    #[test]
    fn cursor_remote_for_is_non_consuming_and_filters_by_since() {
        let parser = UsageParser::new();
        let jan1 = date(2026, 1, 1);
        let mar1 = date(2026, 3, 1);
        let jun1 = date(2026, 6, 1);
        // One fetch covering [Jan1, now] with a Jan entry and a Jun entry.
        let entries = vec![make_cursor_entry(jan1), make_cursor_entry(jun1)];
        parser.store_cursor_remote(entries, Some(jan1));

        // Year view (since=Jan1): covered, both entries.
        assert_eq!(parser.cursor_remote_for(Some(jan1)).unwrap().len(), 2);
        // Non-consuming: a second read still returns the data.
        assert_eq!(parser.cursor_remote_for(Some(jan1)).unwrap().len(), 2);
        // Narrower views filter to entries on/after `since`.
        let recent = parser.cursor_remote_for(Some(jun1)).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].timestamp.date_naive(), jun1);
        assert_eq!(parser.cursor_remote_for(Some(mar1)).unwrap().len(), 1);
    }

    #[test]
    fn cursor_remote_for_misses_when_request_is_wider_than_cache() {
        let parser = UsageParser::new();
        let jan1 = date(2026, 1, 1);
        let jun1 = date(2026, 6, 1);
        // Cache only covers [Jun1, now].
        parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1));
        // A year request wants older data we don't have -> miss (triggers fetch).
        assert!(parser.cursor_remote_for(Some(jan1)).is_none());
        // The narrow request it does cover is still served.
        assert_eq!(parser.cursor_remote_for(Some(jun1)).unwrap().len(), 1);
    }

    #[test]
    fn cursor_remote_for_serves_stale_entries_after_ttl() {
        let parser = UsageParser::new();
        let jun1 = date(2026, 6, 1);
        parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1));
        parser.age_cursor_remote_cache_for_test(std::time::Duration::from_secs(CACHE_TTL_SECS + 1));

        // Stale-while-revalidate: still serve for cost/UI after TTL.
        // (needs_cursor_remote_fetch also requires auth, so assert TTL aging directly.)
        assert!(parser.cursor_remote_ttl_expired_for_test());
        assert_eq!(parser.cursor_remote_for(Some(jun1)).unwrap().len(), 1);
    }

    #[test]
    fn store_keeps_widest_fresh_cache_against_late_narrow_fetch() {
        let parser = UsageParser::new();
        let jan1 = date(2026, 1, 1);
        let jun1 = date(2026, 6, 1);
        // Wide fetch lands first.
        let wide = vec![make_cursor_entry(jan1), make_cursor_entry(jun1)];
        parser.store_cursor_remote(wide, Some(jan1));
        // A later narrow (day-range) fetch must not clobber the wider dataset.
        parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1));
        // Year view still served fully from the retained wide cache.
        assert_eq!(parser.cursor_remote_for(Some(jan1)).unwrap().len(), 2);
    }

    /// An expired year cache must not shrink to the range of the next
    /// today-only refresh: the refresh replaces only the part it covers.
    #[test]
    fn expired_wide_cache_merges_narrow_refresh() {
        let parser = UsageParser::new();
        let jan1 = date(2026, 1, 1);
        let jun1 = date(2026, 6, 1);
        let jun2 = date(2026, 6, 2);
        parser.store_cursor_remote(
            vec![make_cursor_entry(jan1), make_cursor_entry(jun1)],
            Some(jan1),
        );
        parser.age_cursor_remote_cache_for_test(std::time::Duration::from_secs(CACHE_TTL_SECS + 1));

        parser.store_cursor_remote(
            vec![
                make_cursor_entry_with_input(jun1, 5),
                make_cursor_entry(jun2),
            ],
            Some(jun1),
        );

        let year = parser
            .cursor_remote_for(Some(jan1))
            .expect("the year range stays covered");
        assert_eq!(
            input_tokens_by_day(&year),
            vec![(jan1, 1), (jun1, 5), (jun2, 1)]
        );
        assert_eq!(covered_since(&parser), Some(jan1));
        // The refresh restarts the TTL.
        assert!(!parser.cursor_remote_ttl_expired_for_test());
    }

    /// A widening fetch fills in only the days before the old coverage, so the
    /// recent part the tray and open views already show does not move.
    #[test]
    fn wide_fetch_adds_only_older_days() {
        let parser = UsageParser::new();
        let jan1 = date(2026, 1, 1);
        let jun1 = date(2026, 6, 1);
        let jun2 = date(2026, 6, 2);
        parser.store_cursor_remote(
            vec![make_cursor_entry(jun1), make_cursor_entry(jun2)],
            Some(jun1),
        );
        parser.age_cursor_remote_cache_for_test(std::time::Duration::from_secs(CACHE_TTL_SECS + 1));

        parser.store_cursor_remote(
            vec![
                make_cursor_entry(jan1),
                make_cursor_entry_with_input(jun1, 7),
                make_cursor_entry_with_input(jun2, 7),
            ],
            Some(jan1),
        );

        let year = parser
            .cursor_remote_for(Some(jan1))
            .expect("coverage widened to jan1");
        assert_eq!(
            input_tokens_by_day(&year),
            vec![(jan1, 1), (jun1, 1), (jun2, 1)]
        );
        assert_eq!(covered_since(&parser), Some(jan1));
        // Widening keeps stored_at: the recent part is still due its refresh.
        assert!(parser.cursor_remote_ttl_expired_for_test());
    }

    #[test]
    fn identical_refresh_reports_unchanged() {
        let parser = UsageParser::new();
        let jun1 = date(2026, 6, 1);
        let jun2 = date(2026, 6, 2);
        let recent = || vec![make_cursor_entry(jun1), make_cursor_entry(jun2)];
        assert!(
            parser.store_cursor_remote(recent(), Some(jun1)),
            "first store"
        );
        assert!(!parser.store_cursor_remote(recent(), Some(jun1)));
        // A narrower refresh compares only the part it replaces.
        assert!(!parser.store_cursor_remote(vec![make_cursor_entry(jun2)], Some(jun2)));
        assert!(parser.store_cursor_remote(vec![make_cursor_entry_with_input(jun2, 3)], Some(jun2)));
        // Same token total, different mix: the cost changes, so it is a change.
        let shifted = ParsedEntry {
            cache_read_tokens: 1,
            ..make_cursor_entry_with_input(jun2, 2)
        };
        assert!(parser.store_cursor_remote(vec![shifted], Some(jun2)));
    }

    /// The post-fetch tray repaint relies on an unchanged refresh still
    /// renewing the cache: only a fresh cache releases the tray's cost holdover.
    #[test]
    fn unchanged_refresh_still_renews_ttl() {
        let parser = UsageParser::new();
        let jun1 = date(2026, 6, 1);
        parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1));
        parser.age_cursor_remote_cache_for_test(std::time::Duration::from_secs(CACHE_TTL_SECS + 1));
        assert!(!parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1)));
        assert!(!parser.cursor_remote_ttl_expired_for_test());
    }

    #[test]
    fn unchanged_refreshes_back_off_the_ttl_until_cursor_wakes() {
        let parser = UsageParser::new();
        let jun1 = date(2026, 6, 1);
        let same = || vec![make_cursor_entry(jun1)];
        let ttl = || parser.cursor_remote_ttl_secs.load(Ordering::SeqCst);
        parser.store_cursor_remote(same(), Some(jun1));
        parser.store_cursor_remote(same(), Some(jun1));
        assert_eq!(ttl(), 2 * CACHE_TTL_SECS);
        for _ in 0..10 {
            parser.store_cursor_remote(same(), Some(jun1));
        }
        assert_eq!(ttl(), CURSOR_IDLE_MAX_SECS);
        parser.age_cursor_remote_cache_for_test(std::time::Duration::from_secs(CACHE_TTL_SECS + 1));
        assert!(!parser.cursor_remote_ttl_expired_for_test(), "backed off");

        parser.reset_cursor_remote_ttl();
        assert!(parser.cursor_remote_ttl_expired_for_test(), "woken");
        parser.store_cursor_remote(same(), Some(jun1));
        parser.store_cursor_remote(vec![make_cursor_entry_with_input(jun1, 9)], Some(jun1));
        assert_eq!(ttl(), CACHE_TTL_SECS, "a change drops it back");
    }

    /// Views computed while the range was uncovered carry no Cursor data at
    /// all, so widening coverage is a change even when no older day is added.
    #[test]
    fn widening_without_older_entries_reports_changed() {
        let parser = UsageParser::new();
        let jan1 = date(2026, 1, 1);
        let jun1 = date(2026, 6, 1);
        parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1));
        assert!(parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jan1)));
        assert_eq!(covered_since(&parser), Some(jan1));
    }

    #[test]
    fn uncovered_ignores_ttl_expiry() {
        let parser = UsageParser::new();
        let jan1 = date(2026, 1, 1);
        let jun1 = date(2026, 6, 1);
        assert!(
            parser.cursor_remote_cache_uncovered(Some(jun1)),
            "no cache yet"
        );

        parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1));
        parser.age_cursor_remote_cache_for_test(std::time::Duration::from_secs(CACHE_TTL_SECS + 1));
        // Expired but still covering: served as-is, no refetch from the view.
        assert!(!parser.cursor_remote_cache_uncovered(Some(jun1)));
        assert!(parser.cursor_remote_cache_uncovered(Some(jan1)));
    }

    #[test]
    fn clear_cursor_remote_drops_cache_and_cooldown() {
        let parser = UsageParser::new();
        let jun1 = date(2026, 6, 1);
        parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1));
        parser.note_cursor_remote_failure();

        parser.clear_cursor_remote();

        assert!(parser.cursor_remote_for(Some(jun1)).is_none());
        assert!(!parser.cursor_remote_failure_cooldown_active());
    }

    /// A view computed while a Cursor fetch lands was built from the old
    /// snapshot. The fetch's completion clears the payload caches first, so a
    /// store after that clear would outlive it; only an unchanged refresh
    /// leaves the snapshot current.
    #[test]
    fn payload_built_across_a_cursor_change_is_not_cached() {
        let parser = UsageParser::new();
        let jan1 = date(2026, 1, 1);
        let jun1 = date(2026, 6, 1);
        parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1));

        let generation = parser.cursor_remote_generation();
        assert!(!parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1)));
        assert!(parser.store_cache_at_cursor_generation(
            "view",
            UsagePayload::default(),
            generation
        ));
        assert!(parser.check_cache("view").is_some());

        // New data lands mid-compute; the completion clears, then the
        // compute finishes and tries to store.
        let generation = parser.cursor_remote_generation();
        assert!(parser.store_cursor_remote(vec![make_cursor_entry_with_input(jun1, 9)], Some(jun1)));
        parser.clear_payload_cache();
        assert!(!parser.store_cache_at_cursor_generation(
            "view",
            UsagePayload::default(),
            generation
        ));
        assert!(parser.check_cache("view").is_none());

        // Widening and clearing replace the snapshot too.
        let generation = parser.cursor_remote_generation();
        parser.store_cursor_remote(Vec::new(), Some(jan1));
        assert_ne!(parser.cursor_remote_generation(), generation);
        let generation = parser.cursor_remote_generation();
        parser.clear_cursor_remote();
        assert_ne!(parser.cursor_remote_generation(), generation);
    }

    /// A fetch that began before a clear (the Cursor account may have
    /// changed) used the old credentials: neither its entries nor its
    /// failure may land after the clear.
    #[test]
    fn a_fetch_begun_before_a_clear_does_not_land() {
        let parser = UsageParser::new();
        let jun1 = date(2026, 6, 1);
        parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1));

        let begun = parser.cursor_remote_generation();
        parser.clear_cursor_remote();
        assert_eq!(
            parser.store_cursor_remote_if_current(
                vec![make_cursor_entry_with_input(jun1, 9)],
                Some(jun1),
                begun
            ),
            None,
            "superseded"
        );
        assert!(
            parser.cursor_remote_for(Some(jun1)).is_none(),
            "the clear stands"
        );
        assert!(!parser.note_cursor_remote_failure_if_current(begun));
        assert!(
            !parser.cursor_remote_failure_cooldown_active(),
            "the new account is not held back by the old one's failure"
        );

        // A fetch begun after the clear lands.
        let begun = parser.cursor_remote_generation();
        assert_eq!(
            parser.store_cursor_remote_if_current(vec![make_cursor_entry(jun1)], Some(jun1), begun),
            Some(true)
        );
        assert_eq!(parser.cursor_remote_for(Some(jun1)).unwrap().len(), 1);
        let begun = parser.cursor_remote_generation();
        assert!(parser.note_cursor_remote_failure_if_current(begun));
        assert!(parser.cursor_remote_failure_cooldown_active());
    }

    /// A failed fetch caches nothing, so before the cooldown existed every
    /// `data-updated` refresh re-entered spawn → fetch → fail → emit, ~4x/s.
    /// Simulate that refresh storm: only the gate decides, and it must let
    /// exactly one attempt through per cooldown window.
    #[test]
    fn failing_fetch_is_retried_at_most_once_per_refresh_interval() {
        let parser = UsageParser::new();
        let mut attempts = 0;

        let run_refresh_storm = |attempts: &mut usize| {
            for _ in 0..100 {
                if !parser.cursor_remote_failure_cooldown_active() {
                    *attempts += 1;
                    // Every attempt fails, exactly as in the observed loop.
                    parser.note_cursor_remote_failure();
                }
            }
        };

        run_refresh_storm(&mut attempts);
        assert_eq!(attempts, 1, "100 refreshes must yield a single fetch");

        // Still inside the window: no further attempts.
        parser.age_cursor_remote_failure_for_test(std::time::Duration::from_secs(
            CURSOR_REMOTE_FAILURE_COOLDOWN_SECS - 1,
        ));
        run_refresh_storm(&mut attempts);
        assert_eq!(
            attempts, 1,
            "cooldown must suppress retries until it lapses"
        );

        // Next interval: exactly one more attempt, not a burst.
        parser.age_cursor_remote_failure_for_test(std::time::Duration::from_secs(
            CURSOR_REMOTE_FAILURE_COOLDOWN_SECS + 1,
        ));
        run_refresh_storm(&mut attempts);
        assert_eq!(attempts, 2, "one retry per interval, not one per refresh");
    }

    #[test]
    fn failure_cooldown_blocks_needs_fetch_and_clears_on_success() {
        let parser = UsageParser::new();
        let jun1 = date(2026, 6, 1);

        parser.note_cursor_remote_failure();
        assert!(parser.cursor_remote_failure_cooldown_active());
        // The gate every spawn site (usage query + tray) goes through.
        assert!(!parser.needs_cursor_remote_fetch(Some(jun1)));

        // A successful fetch proves the endpoint works — drop the cooldown.
        parser.store_cursor_remote(vec![make_cursor_entry(jun1)], Some(jun1));
        assert!(!parser.cursor_remote_failure_cooldown_active());
    }

    #[test]
    fn failure_cooldown_expires_and_clear_cache_resets_it() {
        let parser = UsageParser::new();

        parser.note_cursor_remote_failure();
        parser.age_cursor_remote_failure_for_test(std::time::Duration::from_secs(
            CURSOR_REMOTE_FAILURE_COOLDOWN_SECS + 1,
        ));
        assert!(!parser.cursor_remote_failure_cooldown_active());

        // Explicit cache clear is the user's escape hatch from the cooldown.
        parser.note_cursor_remote_failure();
        assert!(parser.cursor_remote_failure_cooldown_active());
        parser.clear_cache();
        assert!(!parser.cursor_remote_failure_cooldown_active());
    }
}
