use crate::models::{
    ActiveBlock, ChartBucket, ChartSegment, ModelSummary, UsagePayload, UsageSource,
};
use crate::stats::change::ParsedChangeEvent;
#[cfg(test)]
use crate::stats::change::{ChangeEventKind, FileCategory};
use crate::usage::integrations::{
    provider_matches_model, UsageIntegrationId, UsageIntegrationSelection,
};
use chrono::{DateTime, Local, NaiveDate, Timelike};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

#[cfg(test)]
use super::claude_parser::read_claude_entries;
use super::claude_parser::{
    parse_claude_session_file, upsert_claude_change_event, upsert_claude_entry, ClaudeDedupeAction,
};
use super::codex_parser::parse_codex_session_file;
use super::cursor_parser::{
    cursor_last_warning, fetch_cursor_remote_entries, glob_cursor_chat_session_files,
    load_cursor_local_entries, parse_cursor_session_file, set_cursor_warning,
};

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
            scan_jsonl_tree_into(&path, files, directories);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            files.push(path);
        }
    }
}

fn scan_jsonl_tree(dir: &Path) -> (Vec<PathBuf>, Vec<DirectoryStamp>) {
    let mut files = Vec::new();
    let mut directories = Vec::new();
    if !dir.exists() {
        return (files, directories);
    }
    scan_jsonl_tree_into(dir, &mut files, &mut directories);
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

// ─────────────────────────────────────────────────────────────────────────────
// Shared aggregation utility — build segments map for a bucket
// ─────────────────────────────────────────────────────────────────────────────

/// Aggregate (display_name, cost, tokens) keyed by model_key for a slice of entries.
fn build_segment_map(entries: &[&ParsedEntry]) -> HashMap<String, (String, f64, u64)> {
    let mut map: HashMap<String, (String, f64, u64)> = HashMap::new();
    for e in entries {
        let (name, key) = normalize_model(&e.model);
        let cost = crate::usage::pricing::calculate_cost_for_key(
            &key,
            e.input_tokens,
            e.output_tokens,
            e.cache_creation_5m_tokens,
            e.cache_creation_1h_tokens,
            e.cache_read_tokens,
            e.web_search_requests,
        );
        let entry = map.entry(key).or_insert((name, 0.0, 0));
        entry.1 += cost;
        entry.2 += entry_total_tokens(e);
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

fn segment_map_to_vec(map: HashMap<String, (String, f64, u64)>) -> Vec<ChartSegment> {
    map.into_iter()
        .map(|(key, (name, cost, tokens))| ChartSegment {
            model: name,
            model_key: key,
            cost,
            tokens,
        })
        .collect()
}

fn segment_map_to_model_summaries(map: &HashMap<String, (String, f64, u64)>) -> Vec<ModelSummary> {
    map.iter()
        .map(|(key, (name, cost, tokens))| ModelSummary {
            display_name: name.clone(),
            model_key: key.clone(),
            cost: *cost,
            tokens: *tokens,
            change_stats: None,
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// UsageParser
// ─────────────────────────────────────────────────────────────────────────────

const CACHE_TTL_SECS: u64 = 120;
const MAX_PAYLOAD_CACHE_ENTRIES: usize = 256;
const MAX_FILE_CACHE_ENTRIES: usize = 4096;

pub struct UsageParser {
    integrations: Vec<UsageIntegrationConfig>,
    cache: Mutex<HashMap<String, PayloadCacheEntry>>,
    file_cache: Mutex<HashMap<String, CachedFileEntries>>,
    root_file_lists: Mutex<HashMap<String, CachedRootFileList>>,
    last_query_debug: Mutex<Option<UsageQueryDebugReport>>,
    archive: Mutex<Option<super::archive::ArchiveManager>>,
}

fn prune_payload_cache(cache: &mut HashMap<String, PayloadCacheEntry>) {
    let now = Instant::now();
    cache.retain(|_, entry| now.duration_since(entry.stored_at).as_secs() < CACHE_TTL_SECS);

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

fn prune_file_cache(cache: &mut HashMap<String, CachedFileEntries>) {
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
        }
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
        if let Ok(mut c) = self.file_cache.lock() {
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
    }

    pub fn clear_payload_cache(&self) {
        if let Ok(mut c) = self.cache.lock() {
            c.clear();
        }
    }

    pub fn clear_payload_cache_prefix(&self, prefix: &str) {
        if let Ok(mut c) = self.cache.lock() {
            c.retain(|key, _| !key.starts_with(prefix));
        }
    }

    /// Returns true if any integration root directory has changed since last scan.
    /// When false, the payload cache is still valid and callers can skip a full refresh.
    pub fn have_sources_changed(&self) -> bool {
        let cache = match self.root_file_lists.lock() {
            Ok(c) => c,
            Err(_) => return true,
        };

        if cache.is_empty() {
            return true;
        }

        for entry in cache.values() {
            if !Self::root_listing_is_fresh(entry) {
                return true;
            }
        }

        false
    }

    /// Invalidate the payload cache only if source files have changed.
    /// Returns true if the cache was cleared.
    pub fn invalidate_if_changed(&self) -> bool {
        if self.have_sources_changed() {
            self.clear_payload_cache();
            true
        } else {
            false
        }
    }

    pub fn check_cache(&self, key: &str) -> Option<UsagePayload> {
        let mut c = self.cache.lock().ok()?;
        prune_payload_cache(&mut c);

        if let Some(entry) = c.get_mut(key) {
            entry.last_accessed_at = Instant::now();
            let mut payload = entry.payload.clone();
            payload.from_cache = true;
            return Some(payload);
        }

        None
    }

    pub fn store_cache(&self, key: &str, payload: UsagePayload) {
        if let Ok(mut c) = self.cache.lock() {
            let now = Instant::now();
            c.insert(
                key.to_string(),
                PayloadCacheEntry {
                    payload,
                    stored_at: now,
                    last_accessed_at: now,
                },
            );
            prune_payload_cache(&mut c);
        }
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

        entry
            .file_stamps
            .iter()
            .all(|file| file_stamp(&file.path).is_some_and(|stamp| stamp == file.stamp))
    }

    fn cached_jsonl_files(&self, dir: &Path) -> (Arc<[PathBuf]>, bool) {
        if !dir.exists() {
            return (Arc::from(Vec::<PathBuf>::new()), false);
        }

        let cache_key = path_to_string(dir);
        if let Ok(mut cache) = self.root_file_lists.lock() {
            if let Some(entry) = cache.get_mut(&cache_key) {
                if Self::root_listing_is_fresh(entry) {
                    entry.last_accessed_at = Instant::now();
                    return (entry.files.clone(), true);
                }
                cache.remove(&cache_key);
            }
        }

        let (files, directories) = scan_jsonl_tree(dir);
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
                        file_stamps,
                        last_accessed_at: now,
                    },
                );
                prune_root_file_list_cache(&mut cache);
            }
        }

        (files, false)
    }

    fn load_cached_file(&self, path: &Path, kind: ProviderFileKind) -> CachedFileLoad {
        let cache_key = path_to_string(path);
        let stamp = file_stamp(path);

        if let Some(stamp) = stamp.as_ref() {
            if let Ok(mut cache) = self.file_cache.lock() {
                if let Some(cached) = cache.get_mut(&cache_key) {
                    if &cached.stamp == stamp {
                        cached.last_accessed_at = Instant::now();
                        return CachedFileLoad {
                            entries: cached.entries.clone(),
                            change_events: cached.change_events.clone(),
                            earliest_date: cached.earliest_date,
                            lines_read: 0,
                            opened: false,
                            from_cache: true,
                        };
                    }
                }
            }
        }

        let (entries, change_events, lines_read, opened) = match kind {
            ProviderFileKind::Claude => parse_claude_session_file(path),
            ProviderFileKind::Codex => {
                let (e, ce, lr, op) = parse_codex_session_file(path);
                (e, ce, lr, op)
            }
            ProviderFileKind::Cursor => parse_cursor_session_file(path),
        };
        let earliest_date = earliest_entry_date(&entries);
        let entries: Arc<[ParsedEntry]> = entries.into();
        let change_events: Arc<[ParsedChangeEvent]> = change_events.into();

        if let Ok(mut cache) = self.file_cache.lock() {
            if opened {
                if let Some(stamp) = stamp {
                    let now = Instant::now();
                    cache.insert(
                        cache_key,
                        CachedFileEntries {
                            stamp,
                            entries: entries.clone(),
                            change_events: change_events.clone(),
                            earliest_date,
                            last_accessed_at: now,
                        },
                    );
                    prune_file_cache(&mut cache);
                } else {
                    cache.remove(&cache_key);
                }
            } else {
                cache.remove(&cache_key);
            }
        }

        CachedFileLoad {
            entries,
            change_events,
            earliest_date,
            lines_read,
            opened,
            from_cache: false,
        }
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

        for root_dir in &config.roots {
            let (files, listing_cache_hit) = self.cached_jsonl_files(root_dir);
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

            for path in files.iter() {
                if let Some(since_date) = since {
                    if !modified_since(path, since_date) {
                        let report = reports
                            .get_mut(report_idx)
                            .expect("report should exist for current root");
                        report.skipped_paths += 1;
                        report.skipped_by_mtime += 1;
                        push_sample_path(&mut report.sample_skipped_paths, path);
                        continue;
                    }
                }

                {
                    let report = reports
                        .get_mut(report_idx)
                        .expect("report should exist for current root");
                    report.attempted_paths += 1;
                    push_sample_path(&mut report.sample_paths, path);
                }

                let loaded = self.load_cached_file(path, config.file_kind());
                {
                    let report = reports
                        .get_mut(report_idx)
                        .expect("report should exist for current root");
                    report.lines_read += loaded.lines_read;
                    if loaded.from_cache {
                        report.cache_hits += 1;
                    } else {
                        report.cache_misses += 1;
                        if loaded.opened {
                            report.opened_paths += 1;
                        } else {
                            report.failed_paths += 1;
                            continue;
                        }
                    }
                }

                // Collect change events (filtered by since date)
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

    fn load_cursor_local_entries_with_debug(
        &self,
        since: Option<NaiveDate>,
    ) -> (Vec<ParsedEntry>, ProviderReadDebug) {
        let config = self
            .integration_config(UsageIntegrationId::Cursor)
            .expect("cursor integration should be configured");
        let root_dir = config.roots.first().cloned().unwrap_or_default();
        load_cursor_local_entries(&root_dir, since)
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

        match fetch_cursor_remote_entries(since) {
            Ok(Some(entries)) => {
                report.strategy = format!("{}+cursor-remote-api", report.strategy);
                report.emitted_entries = entries.len();
                if entries.is_empty() {
                    tracing::info!(
                        "Cursor remote API returned no usage entries for the selected period"
                    );
                }
                set_cursor_warning(None);
                (entries, Vec::new(), report)
            }
            Ok(None) => {
                report.strategy = format!("{}+cursor-remote-api-not-configured", report.strategy);
                let warning = String::from(
                    "Cursor remote auth is not configured. Paste a `WorkosCursorSessionToken` from cursor.com cookies (recommended for Pro/Pro+/Ultra) — or, for Enterprise admins, paste a `key_…` Admin API Key from Cursor Dashboard → Settings → Advanced.",
                );
                set_cursor_warning(Some(warning));
                (Vec::new(), Vec::new(), report)
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Cursor remote API source failed"
                );
                push_sample_path(
                    &mut report.sample_skipped_paths,
                    Path::new(&format!("cursor-remote-api: {error}")),
                );
                report.strategy = format!("{}+cursor-remote-api-failed", report.strategy);
                set_cursor_warning(None);
                (Vec::new(), Vec::new(), report)
            }
        }
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
        let Some(selection) = UsageIntegrationSelection::parse(provider) else {
            return (Vec::new(), Vec::new(), Vec::new());
        };

        let mut entries = Vec::new();
        let mut change_events = Vec::new();
        let mut reports = Vec::new();

        let archive_guard = self.archive.lock().unwrap();
        let archive = archive_guard.as_ref();

        for integration_id in selection.integration_ids() {
            let source_key = format!("local:{}", integration_id.as_str());
            let frontier = archive.and_then(|a| a.frontier(&source_key));

            // Load archived entries for completed hours (up to frontier).
            if let (Some(a), Some(_frontier)) = (archive, frontier) {
                let archived = a.load_archived(&source_key, since);
                entries.extend(archived);
            }

            // Load live entries from source JSONL files.
            match integration_id {
                UsageIntegrationId::Claude => {
                    let (next_entries, next_change_events, next_reports) =
                        self.load_claude_entries_with_debug(since);

                    // If archive covers some hours, filter live entries to
                    // only include those AFTER the archive frontier.
                    if let Some(ref f) = frontier {
                        entries.extend(next_entries.into_iter().filter(|e| {
                            !f.covers(e.timestamp.date_naive(), e.timestamp.hour() as u8)
                        }));
                    } else {
                        entries.extend(next_entries);
                    }
                    change_events.extend(next_change_events);
                    reports.extend(next_reports);
                }
                UsageIntegrationId::Codex => {
                    let (next_entries, next_change_events, next_report) =
                        self.load_codex_entries_with_debug(since);

                    if let Some(ref f) = frontier {
                        entries.extend(next_entries.into_iter().filter(|e| {
                            !f.covers(e.timestamp.date_naive(), e.timestamp.hour() as u8)
                        }));
                    } else {
                        entries.extend(next_entries);
                    }
                    change_events.extend(next_change_events);
                    reports.push(next_report);
                }
                UsageIntegrationId::Cursor => {
                    let (next_entries, next_change_events, next_report) =
                        self.load_cursor_entries_with_debug(since);

                    if let Some(ref f) = frontier {
                        entries.extend(next_entries.into_iter().filter(|e| {
                            !f.covers(e.timestamp.date_naive(), e.timestamp.hour() as u8)
                        }));
                    } else {
                        entries.extend(next_entries);
                    }
                    change_events.extend(next_change_events);
                    reports.push(next_report);
                }
            }
        }

        // Drop rows whose model doesn't belong to the selected provider tab.
        // A third-party model logged through any CLI (e.g. GLM-5 via a Claude
        // Code proxy) should not show up in the Claude tab; otherwise the
        // main dashboard total diverges from the Per-Device breakdown, which
        // applies the same predicate to remote SSH rows.
        entries.retain(|e| provider_matches_model(provider, &e.model));

        (entries, change_events, reports)
    }

    // ── has_entries_before: check if data exists before a given date ──

    pub fn has_entries_before(&self, provider: &str, before_date: NaiveDate) -> bool {
        let Some(selection) = UsageIntegrationSelection::parse(provider) else {
            return false;
        };

        selection
            .integration_ids()
            .iter()
            .copied()
            .any(|integration_id| match integration_id {
                UsageIntegrationId::Claude => self.has_claude_entries_before(before_date),
                UsageIntegrationId::Codex => self.has_codex_entries_before(before_date),
                UsageIntegrationId::Cursor => self.has_cursor_entries_before(before_date),
            })
    }

    fn has_integration_entries_before(
        &self,
        config: &UsageIntegrationConfig,
        before_date: NaiveDate,
    ) -> bool {
        for root_dir in &config.roots {
            let (files, _) = self.cached_jsonl_files(root_dir);
            let mut files: Vec<PathBuf> = files.iter().cloned().collect();
            files.sort_by_key(|path| {
                fs::metadata(path)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .map(|modified| {
                        let modified: chrono::DateTime<Local> = modified.into();
                        let modified_date = modified.date_naive();
                        (modified_date >= before_date, modified_date)
                    })
                    .unwrap_or((true, Local::now().date_naive()))
            });

            for path in files {
                let loaded = self.load_cached_file(&path, config.file_kind());
                if loaded
                    .earliest_date
                    .is_some_and(|entry_date| entry_date < before_date)
                {
                    return true;
                }
            }
        }

        false
    }

    fn has_claude_entries_before(&self, before_date: NaiveDate) -> bool {
        let config = self
            .integration_config(UsageIntegrationId::Claude)
            .expect("claude integration should be configured");
        self.has_integration_entries_before(config, before_date)
    }

    fn has_codex_entries_before(&self, before_date: NaiveDate) -> bool {
        let config = self
            .integration_config(UsageIntegrationId::Codex)
            .expect("codex integration should be configured");
        self.has_integration_entries_before(config, before_date)
    }

    fn has_cursor_entries_before(&self, before_date: NaiveDate) -> bool {
        let config = self
            .integration_config(UsageIntegrationId::Cursor)
            .expect("cursor integration should be configured");
        for root_dir in &config.roots {
            for path in glob_cursor_chat_session_files(root_dir) {
                let (entries, _changes, _lines_read, _opened) = parse_cursor_session_file(&path);
                if entries
                    .iter()
                    .any(|entry| entry.timestamp.date_naive() < before_date)
                {
                    return true;
                }
            }
        }
        false
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
        let (entries, _change_events, sources) = self.load_entries(provider, since_date);
        self.set_last_query_debug(UsageQueryDebugReport {
            provider: provider.to_string(),
            aggregation: String::from("daily"),
            since: since.to_string(),
            cache_key: cache_key.clone(),
            from_cache: false,
            entry_count: entries.len(),
            sources,
        });

        // Group by NaiveDate using a BTreeMap so dates are ordered
        let mut day_map: std::collections::BTreeMap<NaiveDate, Vec<&ParsedEntry>> =
            std::collections::BTreeMap::new();
        for e in &entries {
            day_map.entry(e.timestamp.date_naive()).or_default().push(e);
        }

        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, (String, f64, u64)> = HashMap::new();

        for (date, day_entries) in &day_map {
            let label = date.format("%b %-d").to_string();
            let seg_map = build_segment_map(day_entries);
            let bucket_cost: f64 = seg_map.values().map(|(_, c, _)| c).sum();
            let bucket_tokens: u64 = seg_map.values().map(|(_, _, t)| t).sum();

            total_cost += bucket_cost;
            total_tokens += bucket_tokens;

            for e in day_entries.iter() {
                total_input += e.input_tokens;
                total_output += e.output_tokens;
            }

            // Merge into global model map
            for (key, (name, cost, tokens)) in &seg_map {
                let gm = global_model_map
                    .entry(key.clone())
                    .or_insert((name.clone(), 0.0, 0));
                gm.1 += cost;
                gm.2 += tokens;
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
        }
    }

    // ── Aggregation: monthly ──

    pub fn get_monthly(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("monthly:{}:{}", provider, since);
        let since_date = parse_since_date(since);
        let (entries, _change_events, sources) = self.load_entries(provider, since_date);
        self.set_last_query_debug(UsageQueryDebugReport {
            provider: provider.to_string(),
            aggregation: String::from("monthly"),
            since: since.to_string(),
            cache_key: cache_key.clone(),
            from_cache: false,
            entry_count: entries.len(),
            sources,
        });

        // Group by YYYY-MM string using a BTreeMap for order
        let mut month_map: std::collections::BTreeMap<String, Vec<&ParsedEntry>> =
            std::collections::BTreeMap::new();
        for e in &entries {
            let key = e.timestamp.format("%Y-%m").to_string();
            month_map.entry(key).or_default().push(e);
        }

        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, (String, f64, u64)> = HashMap::new();

        for (ym, month_entries) in &month_map {
            // Label: parse "YYYY-MM" -> "Jan", "Feb", etc.
            let label = NaiveDate::parse_from_str(&format!("{}-01", ym), "%Y-%m-%d")
                .map(|d| d.format("%b").to_string())
                .unwrap_or_else(|_| ym.clone());

            let seg_map = build_segment_map(month_entries);
            let bucket_cost: f64 = seg_map.values().map(|(_, c, _)| c).sum();
            let bucket_tokens: u64 = seg_map.values().map(|(_, _, t)| t).sum();

            total_cost += bucket_cost;
            total_tokens += bucket_tokens;

            for e in month_entries.iter() {
                total_input += e.input_tokens;
                total_output += e.output_tokens;
            }

            for (key, (name, cost, tokens)) in &seg_map {
                let gm = global_model_map
                    .entry(key.clone())
                    .or_insert((name.clone(), 0.0, 0));
                gm.1 += cost;
                gm.2 += tokens;
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
        }
    }

    // ── Aggregation: hourly ──

    pub fn get_hourly(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("hourly:{}:{}", provider, since);
        let since_date = parse_since_date(since);
        let end_date = since_date.map(|date| date + chrono::Duration::days(1));
        let (entries, _change_events, sources) = self.load_entries(provider, since_date);
        let entries: Vec<ParsedEntry> = entries
            .into_iter()
            .filter(|entry| end_date.is_none_or(|end| entry.timestamp.date_naive() < end))
            .collect();
        self.set_last_query_debug(UsageQueryDebugReport {
            provider: provider.to_string(),
            aggregation: String::from("hourly"),
            since: since.to_string(),
            cache_key: cache_key.clone(),
            from_cache: false,
            entry_count: entries.len(),
            sources,
        });

        // Group by hour (0-23)
        let mut hour_map: HashMap<u32, Vec<&ParsedEntry>> = HashMap::new();
        for e in &entries {
            hour_map.entry(e.timestamp.hour()).or_default().push(e);
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
        let mut global_model_map: HashMap<String, (String, f64, u64)> = HashMap::new();

        for h in start_hour..=end_hour {
            let label = format_hour(h);
            let hour_entries = hour_map.get(&h).map(|v| v.as_slice()).unwrap_or(&[]);

            let seg_map = build_segment_map(hour_entries);
            let bucket_cost: f64 = seg_map.values().map(|(_, c, _)| c).sum();
            let bucket_tokens: u64 = seg_map.values().map(|(_, _, t)| t).sum();

            total_cost += bucket_cost;
            total_tokens += bucket_tokens;

            for e in hour_entries.iter() {
                total_input += e.input_tokens;
                total_output += e.output_tokens;
            }

            for (key, (name, cost, tokens)) in &seg_map {
                let gm = global_model_map
                    .entry(key.clone())
                    .or_insert((name.clone(), 0.0, 0));
                gm.1 += cost;
                gm.2 += tokens;
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
        }
    }

    // ── Aggregation: blocks ──

    pub fn get_blocks(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("blocks:{}:{}", provider, since);
        let since_date = parse_since_date(since);
        let (mut entries, _change_events, sources) = self.load_entries(provider, since_date);
        self.set_last_query_debug(UsageQueryDebugReport {
            provider: provider.to_string(),
            aggregation: String::from("blocks"),
            since: since.to_string(),
            cache_key: cache_key.clone(),
            from_cache: false,
            entry_count: entries.len(),
            sources,
        });

        // Sort by timestamp ascending
        entries.sort_by_key(|a| a.timestamp);

        // NOT a const — chrono::Duration::minutes() is not const fn
        let gap_threshold = chrono::Duration::minutes(30);

        // Split into blocks separated by gaps > 30 minutes
        let mut blocks: Vec<Vec<&ParsedEntry>> = Vec::new();
        {
            let entry_refs: Vec<&ParsedEntry> = entries.iter().collect();
            let mut current_block: Vec<&ParsedEntry> = Vec::new();
            let mut prev_ts: Option<DateTime<Local>> = None;

            for e in &entry_refs {
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
        let mut global_model_map: HashMap<String, (String, f64, u64)> = HashMap::new();
        let mut active_block: Option<ActiveBlock> = None;
        let mut five_hour_cost = 0.0f64;

        for (idx, block) in blocks.iter().enumerate() {
            let seg_map = build_segment_map(block);
            let block_cost: f64 = seg_map.values().map(|(_, c, _)| c).sum();
            let block_tokens: u64 = seg_map.values().map(|(_, _, t)| t).sum();

            total_cost += block_cost;
            total_tokens += block_tokens;

            for (key, (name, cost, tokens)) in &seg_map {
                let gm = global_model_map
                    .entry(key.clone())
                    .or_insert((name.clone(), 0.0, 0));
                gm.1 += cost;
                gm.2 += tokens;
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
        scan_jsonl_tree_into(root.path(), &mut files, &mut dirs);
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
    fn parse_cursor_official_usage_events_errors_when_neither_array_present() {
        let data = serde_json::json!({"someOtherField": []});
        match parse_cursor_official_usage_events(&data, None, "cursor-admin") {
            Ok(_) => panic!("expected error when payload is missing the events array"),
            Err(err) => assert!(
                err.contains("usageEvents/usageEventsDisplay"),
                "error should mention both array names so users can debug, got: {err}"
            ),
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
    fn load_claude_entries_prefers_subagent_scope_for_mirrored_change_events() {
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
            crate::stats::subagent::AgentScope::Subagent
        );
        assert_eq!(change_events.len(), 1);
        assert_eq!(
            change_events[0].agent_scope,
            crate::stats::subagent::AgentScope::Subagent
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
    fn claude_dedupe_collapses_root_and_sidechain_and_prefers_subagent_scope() {
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
            crate::stats::subagent::AgentScope::Subagent
        );
        assert!(
            entries[0].session_key.contains("agt-1"),
            "subagent mirror should keep the sidechain session_key"
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

    use super::*;
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
