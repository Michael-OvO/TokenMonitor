use crate::models::{ActiveBlock, ChartBucket, ChartSegment, ModelSummary, UsagePayload};
use chrono::{DateTime, Local, NaiveDate, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime};

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
    pub unique_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderReadDebug {
    pub provider: String,
    pub root_dir: String,
    pub root_exists: bool,
    pub since: Option<String>,
    pub strategy: String,
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
struct CachedFileEntries {
    stamp: FileStamp,
    entries: Vec<ParsedEntry>,
    earliest_date: Option<NaiveDate>,
}

#[derive(Clone, Copy)]
enum ProviderFileKind {
    Claude,
    Codex,
}

struct CachedFileLoad {
    entries: Vec<ParsedEntry>,
    earliest_date: Option<NaiveDate>,
    lines_read: usize,
    opened: bool,
    from_cache: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Claude JSONL serde types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ClaudeJsonlEntry {
    #[serde(rename = "type", default)]
    entry_type: String,
    #[serde(default)]
    timestamp: String,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    message: Option<ClaudeJsonlMessage>,
}

#[derive(Deserialize)]
struct ClaudeJsonlMessage {
    model: Option<String>,
    usage: Option<ClaudeJsonlUsage>,
    id: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeJsonlUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation: Option<CacheCreationBreakdown>,
}

#[derive(Deserialize)]
struct CacheCreationBreakdown {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Codex JSONL serde types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CodexJsonlEntry {
    #[serde(rename = "type", default)]
    entry_type: String,
    timestamp: Option<String>,
    payload: Option<Value>,
}

// ─────────────────────────────────────────────────────────────────────────────
// File scanning helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Recursively find all `.jsonl` files under `dir`.
pub fn glob_jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if !dir.exists() {
        return results;
    }
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return results,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
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
pub fn parse_since_date(since: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(since, "%Y%m%d").ok()
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn push_sample_path(sample_paths: &mut Vec<String>, path: &Path) {
    if sample_paths.len() < 5 {
        sample_paths.push(path_to_string(path));
    }
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

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for path in paths {
        let key = path_to_string(&path);
        if seen.insert(key) {
            out.push(path);
        }
    }

    out
}

fn normalize_claude_projects_dir(path: PathBuf) -> PathBuf {
    if path.file_name().is_some_and(|name| name == "projects") {
        path
    } else {
        path.join("projects")
    }
}

fn normalize_codex_sessions_dir(path: PathBuf) -> PathBuf {
    if path.file_name().is_some_and(|name| name == "sessions") {
        path
    } else {
        path.join("sessions")
    }
}

fn detect_claude_project_dirs() -> Vec<PathBuf> {
    if let Ok(raw) = env::var("CLAUDE_CONFIG_DIR") {
        let explicit = raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(normalize_claude_projects_dir)
            .collect::<Vec<_>>();

        if !explicit.is_empty() {
            return dedupe_paths(explicit);
        }
    }

    let home = dirs::home_dir().unwrap_or_default();
    let config_dir = dirs::config_dir().unwrap_or_else(|| home.join(".config"));

    dedupe_paths(vec![
        config_dir.join("claude").join("projects"),
        home.join(".claude").join("projects"),
    ])
}

fn detect_codex_sessions_dir() -> PathBuf {
    if let Ok(raw) = env::var("CODEX_HOME") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return normalize_codex_sessions_dir(PathBuf::from(trimmed));
        }
    }

    let home = dirs::home_dir().unwrap_or_default();
    home.join(".codex").join("sessions")
}

fn create_claude_unique_hash(entry: &ClaudeJsonlEntry) -> Option<String> {
    let message_id = entry.message.as_ref()?.id.as_ref()?;
    let request_id = entry.request_id.as_ref()?;

    Some(format!("{message_id}:{request_id}"))
}

#[derive(Clone, Copy, Default)]
struct CodexRawUsage {
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    total_tokens: u64,
}

fn codex_usage_is_zero(usage: CodexRawUsage) -> bool {
    usage.input_tokens == 0
        && usage.cached_input_tokens == 0
        && usage.output_tokens == 0
        && usage.reasoning_output_tokens == 0
        && usage.total_tokens == 0
}

fn ensure_u64(value: Option<&Value>) -> u64 {
    value.and_then(Value::as_u64).unwrap_or(0)
}

fn normalize_codex_raw_usage(value: Option<&Value>) -> Option<CodexRawUsage> {
    let record = value?.as_object()?;

    let input_tokens = ensure_u64(record.get("input_tokens"));
    let cached_input_tokens = ensure_u64(
        record
            .get("cached_input_tokens")
            .or_else(|| record.get("cache_read_input_tokens")),
    );
    let output_tokens = ensure_u64(record.get("output_tokens"));
    let reasoning_output_tokens = ensure_u64(record.get("reasoning_output_tokens"));
    let total_tokens = ensure_u64(record.get("total_tokens"));

    Some(CodexRawUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        total_tokens: if total_tokens > 0 {
            total_tokens
        } else {
            input_tokens + output_tokens
        },
    })
}

fn subtract_codex_raw_usage(
    current: CodexRawUsage,
    previous: Option<CodexRawUsage>,
) -> CodexRawUsage {
    let previous = previous.unwrap_or_default();

    CodexRawUsage {
        input_tokens: current.input_tokens.saturating_sub(previous.input_tokens),
        cached_input_tokens: current
            .cached_input_tokens
            .saturating_sub(previous.cached_input_tokens),
        output_tokens: current.output_tokens.saturating_sub(previous.output_tokens),
        reasoning_output_tokens: current
            .reasoning_output_tokens
            .saturating_sub(previous.reasoning_output_tokens),
        total_tokens: current.total_tokens.saturating_sub(previous.total_tokens),
    }
}

fn value_as_non_empty_string(value: Option<&Value>) -> Option<String> {
    let raw = value?.as_str()?.trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

fn extract_codex_model(value: &Value) -> Option<String> {
    if let Some(info) = value.get("info") {
        if let Some(model) = value_as_non_empty_string(info.get("model")) {
            return Some(model);
        }
        if let Some(model) = value_as_non_empty_string(info.get("model_name")) {
            return Some(model);
        }
        if let Some(model) = info
            .get("metadata")
            .and_then(|metadata| value_as_non_empty_string(metadata.get("model")))
        {
            return Some(model);
        }
    }

    if let Some(model) = value_as_non_empty_string(value.get("model")) {
        return Some(model);
    }

    value
        .get("metadata")
        .and_then(|metadata| value_as_non_empty_string(metadata.get("model")))
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
fn modified_since(path: &Path, since: NaiveDate) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| {
            let dt: chrono::DateTime<Local> = t.into();
            dt.date_naive() >= since
        })
        .unwrap_or(true) // if we can't read metadata, include the file
}

type ClaudeParseResult = (Vec<ParsedEntry>, usize, bool);

fn parse_claude_session_file(path: &Path) -> ClaudeParseResult {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return (Vec::new(), 0, false),
    };

    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut lines_read = 0;

    for line in reader.lines() {
        lines_read += 1;
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };

        // Fast pre-filter: skip lines that can't be assistant entries
        if !line.contains("\"assistant\"") {
            continue;
        }

        let entry: ClaudeJsonlEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.entry_type != "assistant" {
            continue;
        }

        let msg = match &entry.message {
            Some(message) => message,
            None => continue,
        };
        let usage = match &msg.usage {
            Some(usage) => usage,
            None => continue,
        };
        let model = match &msg.model {
            Some(model) if !model.starts_with('<') => model.clone(), // skip <synthetic> etc.
            _ => continue,
        };
        let ts = match chrono::DateTime::parse_from_rfc3339(&entry.timestamp) {
            Ok(ts) => ts.with_timezone(&Local),
            Err(_) => continue,
        };

        // Split cache creation into 5m and 1h tiers.
        // If the breakdown sub-object exists, use it directly.
        // Otherwise default all cache creation to 1h (Claude Code's default).
        let total_cw = usage.cache_creation_input_tokens.unwrap_or(0);
        let (cw_5m, cw_1h) = match &usage.cache_creation {
            Some(bd) => (
                bd.ephemeral_5m_input_tokens.unwrap_or(0),
                bd.ephemeral_1h_input_tokens.unwrap_or(0),
            ),
            None => (0, total_cw),
        };

        entries.push(ParsedEntry {
            timestamp: ts,
            model,
            input_tokens: usage.input_tokens.unwrap_or(0),
            output_tokens: usage.output_tokens.unwrap_or(0),
            cache_creation_5m_tokens: cw_5m,
            cache_creation_1h_tokens: cw_1h,
            cache_read_tokens: usage.cache_read_input_tokens.unwrap_or(0),
            unique_hash: create_claude_unique_hash(&entry),
        });
    }

    (entries, lines_read, true)
}

/// Read all Claude assistant entries from JSONL files under `projects_dir`,
/// optionally filtering to entries on or after `since`.
fn read_claude_entries_with_debug(
    projects_dir: &Path,
    since: Option<NaiveDate>,
) -> (Vec<ParsedEntry>, ProviderReadDebug) {
    let mut entries = Vec::new();
    let mut processed_hashes = HashSet::new();
    let files = glob_jsonl_files(projects_dir);
    let mut report = ProviderReadDebug {
        provider: String::from("claude"),
        root_dir: path_to_string(projects_dir),
        root_exists: projects_dir.exists(),
        since: since.map(|date| date.format("%Y-%m-%d").to_string()),
        strategy: String::from("recursive-jsonl-glob"),
        discovered_paths: files.len(),
        ..ProviderReadDebug::default()
    };

    for path in files {
        // Skip files that haven't been modified since the since date.
        // This avoids reading hundreds of old files for short time periods.
        if let Some(since_date) = since {
            if !modified_since(&path, since_date) {
                report.skipped_paths += 1;
                report.skipped_by_mtime += 1;
                push_sample_path(&mut report.sample_skipped_paths, &path);
                continue;
            }
        }
        report.attempted_paths += 1;
        push_sample_path(&mut report.sample_paths, &path);

        let (parsed_entries, lines_read, opened) = parse_claude_session_file(&path);
        report.lines_read += lines_read;
        if opened {
            report.opened_paths += 1;
        } else {
            report.failed_paths += 1;
            continue;
        }

        for entry in parsed_entries {
            if since.is_some_and(|since_date| entry.timestamp.date_naive() < since_date) {
                continue;
            }
            if let Some(unique_hash) = entry.unique_hash.as_ref() {
                if !processed_hashes.insert(unique_hash.clone()) {
                    continue;
                }
            }
            entries.push(entry);
        }
    }
    report.emitted_entries = entries.len();
    (entries, report)
}

#[allow(dead_code)]
pub fn read_claude_entries(projects_dir: &Path, since: Option<NaiveDate>) -> Vec<ParsedEntry> {
    read_claude_entries_with_debug(projects_dir, since).0
}

/// Parse a single Codex session JSONL file.
/// Codex `event_msg` / `token_count` events may include either per-turn
/// `last_token_usage` or cumulative `total_token_usage`. We normalize both
/// forms into per-event deltas and track model context via `turn_context`.
///
/// In current Codex logs, `input_tokens` already includes cached input.
/// Normalize it to billable uncached input here so downstream pricing and
/// token totals do not count cached input twice.
type CodexParseResult = (Vec<ParsedEntry>, usize, bool);

fn parse_codex_session_file(path: &Path) -> CodexParseResult {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return (Vec::new(), 0, false),
    };
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut previous_totals: Option<CodexRawUsage> = None;
    let mut current_model: Option<String> = None;
    let mut lines_read = 0;

    for line in reader.lines() {
        lines_read += 1;
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };

        let entry: CodexJsonlEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.entry_type == "turn_context" {
            if let Some(payload) = entry.payload.as_ref() {
                if let Some(model) = extract_codex_model(payload) {
                    current_model = Some(model);
                }
            }
            continue;
        }

        if entry.entry_type != "event_msg" {
            continue;
        }

        let payload = match entry.payload.as_ref() {
            Some(p) => p,
            None => continue,
        };
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }

        let info = payload.get("info");
        let last_usage = normalize_codex_raw_usage(info.and_then(|value| value.get("last_token_usage")));
        let total_usage =
            normalize_codex_raw_usage(info.and_then(|value| value.get("total_token_usage")));

        let raw_usage = if let Some(last_usage) = last_usage {
            last_usage
        } else if let Some(total_usage) = total_usage {
            subtract_codex_raw_usage(total_usage, previous_totals)
        } else {
            continue;
        };

        if let Some(total_usage) = total_usage {
            previous_totals = Some(total_usage);
        }

        if codex_usage_is_zero(raw_usage) {
            continue;
        }

        let timestamp = match entry.timestamp.as_deref() {
            Some(timestamp) => timestamp,
            None => continue,
        };

        let ts = match chrono::DateTime::parse_from_rfc3339(timestamp) {
            Ok(dt) => dt.with_timezone(&Local),
            Err(_) => continue,
        };

        let extracted_model = extract_codex_model(payload);
        if let Some(model) = extracted_model.as_ref() {
            current_model = Some(model.clone());
        }

        let model = extracted_model
            .or_else(|| current_model.clone())
            .unwrap_or_else(|| String::from("gpt-5"));

        let uncached_input_tokens = raw_usage
            .input_tokens
            .saturating_sub(raw_usage.cached_input_tokens);

        entries.push(ParsedEntry {
            timestamp: ts,
            model,
            input_tokens: uncached_input_tokens,
            output_tokens: raw_usage.output_tokens,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: raw_usage.cached_input_tokens,
            unique_hash: None,
        });
    }

    entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    (entries, lines_read, true)
}

/// Read all Codex session entries from `sessions_dir`, recursively scanning
/// all JSONL session files under the directory.
fn read_codex_entries_with_debug(
    sessions_dir: &Path,
    since: Option<NaiveDate>,
) -> (Vec<ParsedEntry>, ProviderReadDebug) {
    let mut entries = Vec::new();
    let files = glob_jsonl_files(sessions_dir);
    let mut report = ProviderReadDebug {
        provider: String::from("codex"),
        root_dir: path_to_string(sessions_dir),
        root_exists: sessions_dir.exists(),
        since: since.map(|date| date.format("%Y-%m-%d").to_string()),
        strategy: String::from("recursive-jsonl-glob"),
        discovered_paths: files.len(),
        ..ProviderReadDebug::default()
    };

    for path in files {
        if let Some(since_date) = since {
            if !modified_since(&path, since_date) {
                report.skipped_paths += 1;
                report.skipped_by_mtime += 1;
                push_sample_path(&mut report.sample_skipped_paths, &path);
                continue;
            }
        }

        report.attempted_paths += 1;
        push_sample_path(&mut report.sample_paths, &path);
        let (parsed_entries, lines_read, opened) = parse_codex_session_file(&path);
        report.lines_read += lines_read;
        if opened {
            report.opened_paths += 1;
        } else {
            report.failed_paths += 1;
            continue;
        }

        for parsed in parsed_entries {
            if since.is_some_and(|since_date| parsed.timestamp.date_naive() < since_date) {
                continue;
            }
            entries.push(parsed);
        }
    }

    entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    report.emitted_entries = entries.len();
    (entries, report)
}

#[allow(dead_code)]
pub fn read_codex_entries(sessions_dir: &Path, since: Option<NaiveDate>) -> Vec<ParsedEntry> {
    read_codex_entries_with_debug(sessions_dir, since).0
}

// ─────────────────────────────────────────────────────────────────────────────
// Hour label helper
// ─────────────────────────────────────────────────────────────────────────────

fn format_hour(h: u32) -> String {
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
        let cost = crate::pricing::calculate_cost(
            &e.model,
            e.input_tokens,
            e.output_tokens,
            e.cache_creation_5m_tokens,
            e.cache_creation_1h_tokens,
            e.cache_read_tokens,
        );
        let (name, key) = normalize_model(&e.model);
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
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// UsageParser
// ─────────────────────────────────────────────────────────────────────────────

const CACHE_TTL_SECS: u64 = 120;

pub struct UsageParser {
    claude_dirs: Vec<PathBuf>,
    codex_dir: PathBuf,
    cache: Mutex<HashMap<String, (UsagePayload, Instant)>>,
    file_cache: Mutex<HashMap<String, CachedFileEntries>>,
    last_query_debug: Mutex<Option<UsageQueryDebugReport>>,
}

impl UsageParser {
    /// Create with default home-directory paths.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            claude_dirs: detect_claude_project_dirs(),
            codex_dir: detect_codex_sessions_dir(),
            cache: Mutex::new(HashMap::new()),
            file_cache: Mutex::new(HashMap::new()),
            last_query_debug: Mutex::new(None),
        }
    }

    /// Create with an explicit Claude projects directory (for testing).
    #[allow(dead_code)]
    pub fn with_claude_dir(claude_dir: PathBuf) -> Self {
        Self {
            claude_dirs: vec![claude_dir],
            codex_dir: detect_codex_sessions_dir(),
            cache: Mutex::new(HashMap::new()),
            file_cache: Mutex::new(HashMap::new()),
            last_query_debug: Mutex::new(None),
        }
    }

    /// Create with explicit Claude projects directories (for testing).
    #[allow(dead_code)]
    pub fn with_claude_dirs(claude_dirs: Vec<PathBuf>) -> Self {
        Self {
            claude_dirs,
            codex_dir: detect_codex_sessions_dir(),
            cache: Mutex::new(HashMap::new()),
            file_cache: Mutex::new(HashMap::new()),
            last_query_debug: Mutex::new(None),
        }
    }

    /// Create with an explicit Codex sessions directory (for testing).
    #[allow(dead_code)]
    pub fn with_codex_dir(codex_dir: PathBuf) -> Self {
        Self {
            claude_dirs: detect_claude_project_dirs(),
            codex_dir,
            cache: Mutex::new(HashMap::new()),
            file_cache: Mutex::new(HashMap::new()),
            last_query_debug: Mutex::new(None),
        }
    }

    /// Create with explicit directories for both providers (for testing).
    #[allow(dead_code)]
    pub fn with_dirs(claude_dir: PathBuf, codex_dir: PathBuf) -> Self {
        Self {
            claude_dirs: vec![claude_dir],
            codex_dir,
            cache: Mutex::new(HashMap::new()),
            file_cache: Mutex::new(HashMap::new()),
            last_query_debug: Mutex::new(None),
        }
    }

    // ── Cache helpers ──

    #[allow(dead_code)]
    pub fn clear_cache(&self) {
        if let Ok(mut c) = self.cache.lock() {
            c.clear();
        }
        if let Ok(mut c) = self.file_cache.lock() {
            c.clear();
        }
        if let Ok(mut current) = self.last_query_debug.lock() {
            *current = None;
        }
    }

    pub fn check_cache(&self, key: &str) -> Option<UsagePayload> {
        let c = self.cache.lock().ok()?;
        if let Some((payload, ts)) = c.get(key) {
            if ts.elapsed().as_secs() < CACHE_TTL_SECS {
                let mut p = payload.clone();
                p.from_cache = true;
                return Some(p);
            }
        }
        None
    }

    pub fn store_cache(&self, key: &str, payload: UsagePayload) {
        if let Ok(mut c) = self.cache.lock() {
            c.insert(key.to_string(), (payload, Instant::now()));
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

    fn load_cached_file(&self, path: &Path, kind: ProviderFileKind) -> CachedFileLoad {
        let cache_key = path_to_string(path);
        let stamp = file_stamp(path);

        if let Some(stamp) = stamp.as_ref() {
            if let Ok(cache) = self.file_cache.lock() {
                if let Some(cached) = cache.get(&cache_key) {
                    if &cached.stamp == stamp {
                        return CachedFileLoad {
                            entries: cached.entries.clone(),
                            earliest_date: cached.earliest_date,
                            lines_read: 0,
                            opened: false,
                            from_cache: true,
                        };
                    }
                }
            }
        }

        let (entries, lines_read, opened) = match kind {
            ProviderFileKind::Claude => parse_claude_session_file(path),
            ProviderFileKind::Codex => parse_codex_session_file(path),
        };
        let earliest_date = earliest_entry_date(&entries);

        if let Ok(mut cache) = self.file_cache.lock() {
            if opened {
                if let Some(stamp) = stamp {
                    cache.insert(
                        cache_key,
                        CachedFileEntries {
                            stamp,
                            entries: entries.clone(),
                            earliest_date,
                        },
                    );
                } else {
                    cache.remove(&cache_key);
                }
            } else {
                cache.remove(&cache_key);
            }
        }

        CachedFileLoad {
            entries,
            earliest_date,
            lines_read,
            opened,
            from_cache: false,
        }
    }

    fn load_claude_entries_with_debug(
        &self,
        since: Option<NaiveDate>,
    ) -> (Vec<ParsedEntry>, Vec<ProviderReadDebug>) {
        let mut entries = Vec::new();
        let mut reports = Vec::new();
        let mut processed_hashes = HashSet::new();

        for claude_dir in &self.claude_dirs {
            let files = glob_jsonl_files(claude_dir);
            let mut report = ProviderReadDebug {
                provider: String::from("claude"),
                root_dir: path_to_string(claude_dir),
                root_exists: claude_dir.exists(),
                since: since.map(|date| date.format("%Y-%m-%d").to_string()),
                strategy: String::from("recursive-jsonl-glob+parsed-file-cache+dedupe"),
                discovered_paths: files.len(),
                ..ProviderReadDebug::default()
            };

            for path in files {
                if let Some(since_date) = since {
                    if !modified_since(&path, since_date) {
                        report.skipped_paths += 1;
                        report.skipped_by_mtime += 1;
                        push_sample_path(&mut report.sample_skipped_paths, &path);
                        continue;
                    }
                }

                report.attempted_paths += 1;
                push_sample_path(&mut report.sample_paths, &path);

                let loaded = self.load_cached_file(&path, ProviderFileKind::Claude);
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

                for entry in loaded.entries {
                    if since.is_some_and(|since_date| entry.timestamp.date_naive() < since_date) {
                        continue;
                    }
                    if let Some(unique_hash) = entry.unique_hash.as_ref() {
                        if !processed_hashes.insert(unique_hash.clone()) {
                            continue;
                        }
                    }
                    report.emitted_entries += 1;
                    entries.push(entry);
                }
            }

            reports.push(report);
        }

        (entries, reports)
    }

    fn load_codex_entries_with_debug(
        &self,
        since: Option<NaiveDate>,
    ) -> (Vec<ParsedEntry>, ProviderReadDebug) {
        let mut entries = Vec::new();
        let files = glob_jsonl_files(&self.codex_dir);
        let mut report = ProviderReadDebug {
            provider: String::from("codex"),
            root_dir: path_to_string(&self.codex_dir),
            root_exists: self.codex_dir.exists(),
            since: since.map(|date| date.format("%Y-%m-%d").to_string()),
            strategy: String::from("recursive-jsonl-glob+parsed-file-cache+token-delta"),
            discovered_paths: files.len(),
            ..ProviderReadDebug::default()
        };

        for path in files {
            if let Some(since_date) = since {
                if !modified_since(&path, since_date) {
                    report.skipped_paths += 1;
                    report.skipped_by_mtime += 1;
                    push_sample_path(&mut report.sample_skipped_paths, &path);
                    continue;
                }
            }

            report.attempted_paths += 1;
            push_sample_path(&mut report.sample_paths, &path);

            let loaded = self.load_cached_file(&path, ProviderFileKind::Codex);
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

            for parsed in loaded.entries {
                if since.is_some_and(|since_date| parsed.timestamp.date_naive() < since_date) {
                    continue;
                }
                report.emitted_entries += 1;
                entries.push(parsed);
            }
        }

        (entries, report)
    }

    // ── Internal: load entries for a provider/since combination ──

    pub(crate) fn load_entries(
        &self,
        provider: &str,
        since: Option<NaiveDate>,
    ) -> (Vec<ParsedEntry>, Vec<ProviderReadDebug>) {
        match provider {
            "claude" => self.load_claude_entries_with_debug(since),
            "codex" => {
                let (entries, report) = self.load_codex_entries_with_debug(since);
                (entries, vec![report])
            }
            _ => {
                let (mut claude_entries, mut claude_reports) =
                    self.load_claude_entries_with_debug(since);
                let (codex_entries, codex_report) = self.load_codex_entries_with_debug(since);
                claude_entries.extend(codex_entries);
                claude_reports.push(codex_report);
                (claude_entries, claude_reports)
            }
        }
    }

    // ── has_entries_before: check if data exists before a given date ──

    pub fn has_entries_before(&self, provider: &str, before_date: NaiveDate) -> bool {
        match provider {
            "claude" => self.has_claude_entries_before(before_date),
            "codex" => self.has_codex_entries_before(before_date),
            _ => {
                self.has_claude_entries_before(before_date)
                    || self.has_codex_entries_before(before_date)
            }
        }
    }

    fn has_claude_entries_before(&self, before_date: NaiveDate) -> bool {
        for claude_dir in &self.claude_dirs {
            let mut files = glob_jsonl_files(claude_dir);
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
                let loaded = self.load_cached_file(&path, ProviderFileKind::Claude);
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

    fn has_codex_entries_before(&self, before_date: NaiveDate) -> bool {
        let mut files = glob_jsonl_files(&self.codex_dir);
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
            let loaded = self.load_cached_file(&path, ProviderFileKind::Codex);
            if loaded
                .earliest_date
                .is_some_and(|entry_date| entry_date < before_date)
            {
                return true;
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

    // ── Aggregation: daily ──

    pub fn get_daily(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("daily:{}:{}", provider, since);
        if let Some(cached) = self.check_cache(&cache_key) {
            self.set_last_query_debug(UsageQueryDebugReport {
                provider: provider.to_string(),
                aggregation: String::from("daily"),
                since: since.to_string(),
                cache_key: cache_key.clone(),
                from_cache: true,
                entry_count: 0,
                sources: vec![],
            });
            return cached;
        }

        let since_date = parse_since_date(since);
        let (entries, sources) = self.load_entries(provider, since_date);
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

        let payload = UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            period_label: String::new(),
            has_earlier_data: false,
        };

        self.store_cache(&cache_key, payload.clone());
        payload
    }

    // ── Aggregation: monthly ──

    pub fn get_monthly(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("monthly:{}:{}", provider, since);
        if let Some(cached) = self.check_cache(&cache_key) {
            self.set_last_query_debug(UsageQueryDebugReport {
                provider: provider.to_string(),
                aggregation: String::from("monthly"),
                since: since.to_string(),
                cache_key: cache_key.clone(),
                from_cache: true,
                entry_count: 0,
                sources: vec![],
            });
            return cached;
        }

        let since_date = parse_since_date(since);
        let (entries, sources) = self.load_entries(provider, since_date);
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

        let payload = UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            period_label: String::new(),
            has_earlier_data: false,
        };

        self.store_cache(&cache_key, payload.clone());
        payload
    }

    // ── Aggregation: hourly ──

    pub fn get_hourly(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("hourly:{}:{}", provider, since);
        if let Some(cached) = self.check_cache(&cache_key) {
            self.set_last_query_debug(UsageQueryDebugReport {
                provider: provider.to_string(),
                aggregation: String::from("hourly"),
                since: since.to_string(),
                cache_key: cache_key.clone(),
                from_cache: true,
                entry_count: 0,
                sources: vec![],
            });
            return cached;
        }

        let since_date = parse_since_date(since);
        let (entries, sources) = self.load_entries(provider, since_date);
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

        let payload = UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            period_label: String::new(),
            has_earlier_data: false,
        };

        self.store_cache(&cache_key, payload.clone());
        payload
    }

    // ── Aggregation: blocks ──

    pub fn get_blocks(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("blocks:{}:{}", provider, since);
        if let Some(cached) = self.check_cache(&cache_key) {
            self.set_last_query_debug(UsageQueryDebugReport {
                provider: provider.to_string(),
                aggregation: String::from("blocks"),
                since: since.to_string(),
                cache_key: cache_key.clone(),
                from_cache: true,
                entry_count: 0,
                sources: vec![],
            });
            return cached;
        }

        let since_date = parse_since_date(since);
        let (mut entries, sources) = self.load_entries(provider, since_date);
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
        entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

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
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, (String, f64, u64)> = HashMap::new();
        let mut active_block: Option<ActiveBlock> = None;
        let mut five_hour_cost = 0.0f64;

        for (idx, block) in blocks.iter().enumerate() {
            let seg_map = build_segment_map(block);
            let block_cost: f64 = seg_map.values().map(|(_, c, _)| c).sum();
            let block_tokens: u64 = seg_map.values().map(|(_, _, t)| t).sum();

            total_cost += block_cost;
            total_tokens += block_tokens;

            for e in block.iter() {
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
                let is_active = (now - last_entry_ts) <= gap_threshold;

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

        let payload = UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            chart_buckets,
            model_breakdown,
            active_block,
            five_hour_cost,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            period_label: String::new(),
            has_earlier_data: false,
        };

        self.store_cache(&cache_key, payload.clone());
        payload
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── Helpers ──

    fn write_file(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
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
        let (entries, reports) = parser.load_entries("claude", parse_since_date("20260301"));

        assert_eq!(entries.len(), 1, "duplicate assistant transcript entries should count once");
        assert_eq!(entries[0].input_tokens, 10);
        assert_eq!(entries[0].output_tokens, 5);
        assert_eq!(entries[0].cache_creation_1h_tokens, 20);
        assert_eq!(entries[0].cache_read_tokens, 30);
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
        assert!(keys.contains(&"sonnet"), "should include sonnet");
        assert!(keys.contains(&"opus"), "should include opus");
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
    fn cache_returns_same_payload_within_ttl() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);

        let first = parser.get_daily("claude", "20260315");
        assert!(!first.from_cache, "first call should NOT be from cache");

        let second = parser.get_daily("claude", "20260315");
        assert!(
            second.from_cache,
            "second call within TTL should be from cache"
        );
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
        // Use local timestamps at past hours so hour values are guaranteed to be
        // <= current_hour regardless of the system timezone.
        let now = Local::now();
        let ts1 = (now - chrono::Duration::hours(2)).to_rfc3339();
        let ts2 = (now - chrono::Duration::hours(1)).to_rfc3339();
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{ts1}","message":{{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":1000,"output_tokens":500}}}}}}
{{"type":"assistant","timestamp":"{ts2}","message":{{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":2000,"output_tokens":1000}}}}}}"#,
        );

        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("session.jsonl"), &content);
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());

        let today_str = now.format("%Y%m%d").to_string();
        let payload = parser.get_hourly("claude", &today_str);

        // Should have buckets covering from min_hour to current_hour
        assert!(
            !payload.chart_buckets.is_empty(),
            "should produce chart buckets"
        );
        // The entry 2 hours ago should appear in the buckets
        let two_hours_ago_label = format_hour((now - chrono::Duration::hours(2)).hour());
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
}

#[cfg(test)]
mod debug_compare {
    use super::*;

    fn print_provider(label: &str, entries: &[ParsedEntry]) {
        let mut model_totals: std::collections::HashMap<String, (u64, u64, u64, u64, usize, f64)> =
            std::collections::HashMap::new();
        for e in entries {
            let (_, key) = normalize_model(&e.model);
            let cost = crate::pricing::calculate_cost(
                &e.model,
                e.input_tokens,
                e.output_tokens,
                e.cache_creation_5m_tokens,
                e.cache_creation_1h_tokens,
                e.cache_read_tokens,
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

        let claude = parser
            .load_entries("claude", Some(parse_since_date(&today).unwrap()))
            .0;
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

        let codex = parser
            .load_entries("codex", Some(parse_since_date(&today).unwrap()))
            .0;
        print_provider("CODEX", &codex);
        println!("\n=== CODEX: ccusage ===");
        println!("  gpt-5.4: inp=231,247 out=7,338 reasoning=5,997 total=238,585 cost=$0.277788");
        println!("  (reasoning is informational; both parsers bill against token_count usage)");
    }
}
