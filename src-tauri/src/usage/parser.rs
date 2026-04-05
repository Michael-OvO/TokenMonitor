use crate::models::{
    ActiveBlock, ChartBucket, ChartSegment, ModelSummary, UsagePayload, UsageSource,
};
#[cfg(test)]
use crate::stats::change::FileCategory;
use crate::stats::change::{classify_file, ChangeEventKind, ParsedChangeEvent};
use crate::usage::integrations::{UsageIntegrationId, UsageIntegrationSelection};
use chrono::{DateTime, Local, NaiveDate, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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
struct CachedRootFileList {
    files: Arc<[PathBuf]>,
    directories: Arc<[DirectoryStamp]>,
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
        }
    }

    fn dedupe_entry_hashes(&self) -> bool {
        matches!(self.id, UsageIntegrationId::Claude)
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
    #[serde(rename = "toolUseResult")]
    tool_use_result: Option<ClaudeToolUseResult>,
    message: Option<ClaudeJsonlMessage>,
    #[serde(rename = "isSidechain", default)]
    is_sidechain: Option<bool>,
    #[serde(rename = "sessionId", default)]
    session_id: Option<String>,
    #[serde(rename = "agentId", default)]
    agent_id: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeJsonlMessage {
    model: Option<String>,
    usage: Option<ClaudeJsonlUsage>,
    id: Option<String>,
    #[serde(default)]
    content: Vec<ClaudeContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClaudeContentBlock {
    #[serde(rename = "tool_use")]
    ToolUse {
        id: Option<String>,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(rename = "tool_use_id")]
        tool_use_id: String,
    },
    #[serde(other)]
    Other,
}

/// Input fields for the Edit tool_use.
#[derive(Deserialize)]
struct EditToolInput {
    file_path: String,
    old_string: String,
    new_string: String,
}

/// Input fields for the Write tool_use.
#[derive(Deserialize)]
struct WriteToolInput {
    file_path: String,
}

#[derive(Deserialize, Default)]
struct ClaudeToolUseResult {
    #[serde(rename = "filePath")]
    file_path: Option<String>,
    #[serde(rename = "oldString")]
    old_string: Option<String>,
    #[serde(rename = "newString")]
    new_string: Option<String>,
    #[serde(rename = "originalFile")]
    original_file: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(rename = "structuredPatch", default)]
    structured_patch: Vec<ClaudeStructuredPatchChunk>,
}

#[derive(Deserialize, Default)]
struct ClaudeStructuredPatchChunk {
    #[serde(default)]
    lines: Vec<String>,
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

fn scan_jsonl_tree_into(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    directories: &mut Vec<DirectoryStamp>,
) {
    let metadata = match fs::metadata(dir) {
        Ok(metadata) => metadata,
        Err(_) => return,
    };
    let modified = match metadata.modified() {
        Ok(modified) => modified,
        Err(_) => return,
    };
    directories.push(DirectoryStamp {
        path: dir.to_path_buf(),
        modified,
    });

    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
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

fn create_claude_unique_hash(entry: &ClaudeJsonlEntry) -> Option<String> {
    let message_id = entry.message.as_ref()?.id.as_ref()?;
    let request_id = entry.request_id.as_ref()?;
    Some(format!("{message_id}:{request_id}"))
}

fn claude_scope_priority(scope: crate::stats::subagent::AgentScope) -> u8 {
    match scope {
        crate::stats::subagent::AgentScope::Main => 0,
        crate::stats::subagent::AgentScope::Subagent => 1,
    }
}

fn should_prefer_claude_entry(candidate: &ParsedEntry, existing: &ParsedEntry) -> bool {
    if candidate.output_tokens != existing.output_tokens {
        return candidate.output_tokens > existing.output_tokens;
    }

    let candidate_scope = claude_scope_priority(candidate.agent_scope);
    let existing_scope = claude_scope_priority(existing.agent_scope);
    if candidate_scope != existing_scope {
        return candidate_scope > existing_scope;
    }

    if candidate.timestamp != existing.timestamp {
        return candidate.timestamp > existing.timestamp;
    }

    if candidate.session_key != existing.session_key {
        return candidate.session_key < existing.session_key;
    }

    if candidate.model != existing.model {
        return candidate.model < existing.model;
    }

    false
}

fn should_prefer_claude_change_event(
    candidate: &ParsedChangeEvent,
    existing: &ParsedChangeEvent,
) -> bool {
    let candidate_lines = candidate.added_lines + candidate.removed_lines;
    let existing_lines = existing.added_lines + existing.removed_lines;
    if candidate_lines != existing_lines {
        return candidate_lines > existing_lines;
    }

    let candidate_scope = claude_scope_priority(candidate.agent_scope);
    let existing_scope = claude_scope_priority(existing.agent_scope);
    if candidate_scope != existing_scope {
        return candidate_scope > existing_scope;
    }

    if candidate.timestamp != existing.timestamp {
        return candidate.timestamp > existing.timestamp;
    }

    if candidate.path != existing.path {
        return candidate.path < existing.path;
    }

    if candidate.model != existing.model {
        return candidate.model < existing.model;
    }

    false
}

fn create_claude_tool_dedupe_key(
    unique_hash: Option<&String>,
    tool_id: Option<&String>,
    block_index: usize,
) -> Option<String> {
    let hash = unique_hash?;
    let suffix = tool_id.as_ref().map(|id| id.as_str()).unwrap_or_else(|| "");
    if suffix.is_empty() {
        Some(format!("{hash}:{block_index}"))
    } else {
        Some(format!("{hash}:{suffix}"))
    }
}

struct PendingClaudeTool {
    model_key: String,
    timestamp: DateTime<Local>,
    path: String,
    kind: ChangeEventKind,
    fallback_added_lines: u64,
    fallback_removed_lines: u64,
    dedupe_key: Option<String>,
    agent_scope: crate::stats::subagent::AgentScope,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct CodexRawUsage {
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    total_tokens: u64,
}

fn codex_usage_is_zero(usage: CodexRawUsage) -> bool {
    usage == CodexRawUsage::default()
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

fn assign_pending_codex_models(
    model_raw: &str,
    entries: &mut [ParsedEntry],
    pending_entry_indices: &mut Vec<usize>,
    change_events: &mut [ParsedChangeEvent],
    pending_change_indices: &mut Vec<usize>,
) {
    let model_raw = model_raw.to_string();
    let model_key = crate::models::normalized_model_key(&model_raw);

    for idx in pending_entry_indices.drain(..) {
        if let Some(entry) = entries.get_mut(idx) {
            entry.model = model_raw.clone();
        }
    }

    for idx in pending_change_indices.drain(..) {
        if let Some(event) = change_events.get_mut(idx) {
            event.model = model_key.clone();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_codex_change_event(
    change_events: &mut Vec<ParsedChangeEvent>,
    pending_model_indices: &mut Vec<usize>,
    model_key: Option<&str>,
    timestamp: DateTime<Local>,
    path: String,
    kind: ChangeEventKind,
    added_lines: u64,
    removed_lines: u64,
    agent_scope: crate::stats::subagent::AgentScope,
) {
    change_events.push(ParsedChangeEvent {
        timestamp,
        model: model_key.unwrap_or("").to_string(),
        provider: "codex".to_string(),
        category: classify_file(&path),
        path,
        kind,
        added_lines,
        removed_lines,
        dedupe_key: None,
        agent_scope,
    });

    if model_key.is_none() {
        pending_model_indices.push(change_events.len() - 1);
    }
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

/// Count lines in a string (an empty string has 0 lines).
fn count_lines(s: &str) -> u64 {
    if s.is_empty() {
        0
    } else {
        s.lines().count() as u64
    }
}

/// Returns true for provider-internal paths that should not be counted as user edits.
fn is_provider_internal_path(path: &str) -> bool {
    path.contains("/.claude/plans/")
}

fn count_claude_structured_patch_lines(
    chunks: &[ClaudeStructuredPatchChunk],
) -> Option<(u64, u64)> {
    if chunks.is_empty() {
        return None;
    }

    let patch = chunks
        .iter()
        .flat_map(|chunk| chunk.lines.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let (added, removed) = count_diff_lines(&patch);
    Some((added, removed))
}

fn extract_claude_tool_result_counts(
    tool_result: &ClaudeToolUseResult,
    pending: &PendingClaudeTool,
) -> (u64, u64) {
    if let Some(counts) = count_claude_structured_patch_lines(&tool_result.structured_patch) {
        return counts;
    }

    if let (Some(old), Some(new)) = (
        tool_result.old_string.as_deref(),
        tool_result.new_string.as_deref(),
    ) {
        return (count_lines(new), count_lines(old));
    }

    if let (Some(old), Some(new)) = (
        tool_result.original_file.as_deref(),
        tool_result.content.as_deref(),
    ) {
        return (count_lines(new), count_lines(old));
    }

    if pending.kind == ChangeEventKind::FullWrite {
        if let Some(content) = tool_result.content.as_deref() {
            return (count_lines(content), 0);
        }
    }

    (pending.fallback_added_lines, pending.fallback_removed_lines)
}

pub type ClaudeParseResult = (Vec<ParsedEntry>, Vec<ParsedChangeEvent>, usize, bool);

pub fn parse_claude_session_file(path: &Path) -> ClaudeParseResult {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("Failed to open session file {}: {e}", path.display());
            }
            return (Vec::new(), Vec::new(), 0, false);
        }
    };

    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut change_events = Vec::new();
    let mut pending_tools: Vec<Option<PendingClaudeTool>> = Vec::new();
    let mut pending_tool_indices: HashMap<String, usize> = HashMap::new();
    let mut lines_read = 0;
    let mut parse_failures = 0_usize;

    for line in reader.lines() {
        lines_read += 1;
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };

        // Fast pre-filter: skip lines that can't contain assistant/tool_result data
        if !line.contains("\"assistant\"") && !line.contains("\"tool_result\"") {
            continue;
        }

        let entry: ClaudeJsonlEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => {
                parse_failures += 1;
                continue;
            }
        };
        let ts = match chrono::DateTime::parse_from_rfc3339(&entry.timestamp) {
            Ok(ts) => ts.with_timezone(&Local),
            Err(_) => continue,
        };

        match entry.entry_type.as_str() {
            "assistant" => {
                let msg = match &entry.message {
                    Some(message) => message,
                    None => continue,
                };
                let model = match &msg.model {
                    Some(model) if !model.starts_with('<') => model.clone(), // skip <synthetic> etc.
                    _ => continue,
                };

                let agent_scope = if entry.is_sidechain == Some(true) {
                    crate::stats::subagent::AgentScope::Subagent
                } else {
                    crate::stats::subagent::AgentScope::Main
                };
                let session_key = match (&entry.session_id, &entry.agent_id, entry.is_sidechain) {
                    (Some(sid), Some(aid), Some(true)) => format!("claude:{sid}:subagent:{aid}"),
                    (Some(sid), _, _) => format!("claude:{sid}:main"),
                    _ => format!("claude:file:{}", path_to_string(path)),
                };

                let model_key = crate::models::normalized_model_key(&model);
                let unique_hash = create_claude_unique_hash(&entry);
                for (block_index, block) in msg.content.iter().enumerate() {
                    let pending = match block {
                        ClaudeContentBlock::ToolUse { id, name, input } if name == "Edit" => {
                            serde_json::from_value::<EditToolInput>(input.clone())
                                .ok()
                                .and_then(|edit| {
                                    if is_provider_internal_path(&edit.file_path) {
                                        return None;
                                    }
                                    Some((
                                        id.clone(),
                                        PendingClaudeTool {
                                            model_key: model_key.clone(),
                                            timestamp: ts,
                                            path: edit.file_path.clone(),
                                            kind: ChangeEventKind::PatchEdit,
                                            fallback_added_lines: count_lines(&edit.new_string),
                                            fallback_removed_lines: count_lines(&edit.old_string),
                                            dedupe_key: create_claude_tool_dedupe_key(
                                                unique_hash.as_ref(),
                                                id.as_ref(),
                                                block_index,
                                            ),
                                            agent_scope,
                                        },
                                    ))
                                })
                        }
                        ClaudeContentBlock::ToolUse { id, name, input } if name == "Write" => {
                            serde_json::from_value::<WriteToolInput>(input.clone())
                                .ok()
                                .and_then(|write| {
                                    if is_provider_internal_path(&write.file_path) {
                                        return None;
                                    }
                                    Some((
                                        id.clone(),
                                        PendingClaudeTool {
                                            model_key: model_key.clone(),
                                            timestamp: ts,
                                            path: write.file_path.clone(),
                                            kind: ChangeEventKind::FullWrite,
                                            fallback_added_lines: 0,
                                            fallback_removed_lines: 0,
                                            dedupe_key: create_claude_tool_dedupe_key(
                                                unique_hash.as_ref(),
                                                id.as_ref(),
                                                block_index,
                                            ),
                                            agent_scope,
                                        },
                                    ))
                                })
                        }
                        _ => None,
                    };

                    if let Some((tool_id, pending)) = pending {
                        let idx = pending_tools.len();
                        pending_tools.push(Some(pending));
                        if let Some(tool_id) = tool_id {
                            pending_tool_indices.insert(tool_id, idx);
                        }
                    }
                }

                if let Some(usage) = msg.usage.as_ref() {
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
                        unique_hash,
                        session_key: session_key.clone(),
                        agent_scope,
                    });
                }
            }
            "user" => {
                let Some(tool_result) = entry.tool_use_result.as_ref() else {
                    continue;
                };
                let Some(msg) = entry.message.as_ref() else {
                    continue;
                };

                for block in &msg.content {
                    let ClaudeContentBlock::ToolResult { tool_use_id } = block else {
                        continue;
                    };
                    let Some(idx) = pending_tool_indices.remove(tool_use_id) else {
                        continue;
                    };
                    let Some(pending) = pending_tools.get_mut(idx).and_then(Option::take) else {
                        continue;
                    };

                    let path = tool_result
                        .file_path
                        .clone()
                        .filter(|path| !is_provider_internal_path(path))
                        .unwrap_or_else(|| pending.path.clone());
                    let (added_lines, removed_lines) =
                        extract_claude_tool_result_counts(tool_result, &pending);

                    change_events.push(ParsedChangeEvent {
                        timestamp: ts,
                        model: pending.model_key,
                        provider: "claude".to_string(),
                        path: path.clone(),
                        kind: pending.kind,
                        added_lines,
                        removed_lines,
                        category: classify_file(&path),
                        dedupe_key: pending.dedupe_key,
                        agent_scope: pending.agent_scope,
                    });
                }
            }
            _ => {}
        }
    }

    for pending in pending_tools.into_iter().flatten() {
        let path = pending.path.clone();
        change_events.push(ParsedChangeEvent {
            timestamp: pending.timestamp,
            model: pending.model_key,
            provider: "claude".to_string(),
            path: path.clone(),
            kind: pending.kind,
            added_lines: pending.fallback_added_lines,
            removed_lines: pending.fallback_removed_lines,
            category: classify_file(&path),
            dedupe_key: pending.dedupe_key,
            agent_scope: pending.agent_scope,
        });
    }

    // Warn when a high proportion of candidate lines fail to parse,
    // which may indicate a schema change in the JSONL format.
    if parse_failures > 0 && entries.is_empty() && lines_read > 10 {
        tracing::warn!(
            "All {} candidate lines failed to parse in {}; JSONL schema may have changed",
            parse_failures,
            path.display()
        );
    }

    (entries, change_events, lines_read, true)
}

enum ClaudeDedupeAction {
    Inserted,
    Replaced(usize),
    Skipped,
}

fn upsert_claude_entry(
    entries: &mut Vec<ParsedEntry>,
    processed_hashes: &mut HashMap<String, usize>,
    entry: ParsedEntry,
) -> ClaudeDedupeAction {
    let Some(unique_hash) = entry.unique_hash.clone() else {
        entries.push(entry);
        return ClaudeDedupeAction::Inserted;
    };

    if let Some(existing_idx) = processed_hashes.get(&unique_hash).copied() {
        if should_prefer_claude_entry(&entry, &entries[existing_idx]) {
            entries[existing_idx] = entry;
            ClaudeDedupeAction::Replaced(existing_idx)
        } else {
            ClaudeDedupeAction::Skipped
        }
    } else {
        let idx = entries.len();
        entries.push(entry);
        processed_hashes.insert(unique_hash, idx);
        ClaudeDedupeAction::Inserted
    }
}

fn upsert_claude_change_event(
    change_events: &mut Vec<ParsedChangeEvent>,
    processed_change_keys: &mut HashMap<String, usize>,
    change_event: ParsedChangeEvent,
) -> ClaudeDedupeAction {
    let Some(dedupe_key) = change_event.dedupe_key.clone() else {
        change_events.push(change_event);
        return ClaudeDedupeAction::Inserted;
    };

    if let Some(existing_idx) = processed_change_keys.get(&dedupe_key).copied() {
        if should_prefer_claude_change_event(&change_event, &change_events[existing_idx]) {
            change_events[existing_idx] = change_event;
            ClaudeDedupeAction::Replaced(existing_idx)
        } else {
            ClaudeDedupeAction::Skipped
        }
    } else {
        let idx = change_events.len();
        change_events.push(change_event);
        processed_change_keys.insert(dedupe_key, idx);
        ClaudeDedupeAction::Inserted
    }
}

/// Read all Claude assistant entries from JSONL files under `projects_dir`,
/// optionally filtering to entries on or after `since`.
fn read_claude_entries_with_debug(
    projects_dir: &Path,
    since: Option<NaiveDate>,
) -> (Vec<ParsedEntry>, ProviderReadDebug) {
    let mut entries = Vec::new();
    let mut processed_hashes = HashMap::new();
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

        let (parsed_entries, _change_events, lines_read, opened) = parse_claude_session_file(&path);
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
            let _ = upsert_claude_entry(&mut entries, &mut processed_hashes, entry);
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
/// Count added and removed lines in a unified diff.
/// Lines starting with `+` (but not `+++`) are additions.
/// Lines starting with `-` (but not `---`) are removals.
fn count_diff_lines(patch: &str) -> (u64, u64) {
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

/// Extract file paths from unified diff headers.
/// Looks for `+++ b/path` lines and strips the `b/` prefix.
/// Falls back to `diff --git a/path b/path` headers.
fn extract_diff_paths(patch: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            let path = rest.trim();
            if !path.is_empty() {
                paths.push(path.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            // Handle `+++ path` without `b/` prefix (but skip `+++ /dev/null`)
            let path = rest.trim();
            if !path.is_empty() && path != "/dev/null" {
                paths.push(path.to_string());
            }
        }
    }
    // Fall back to diff --git headers if no +++ lines found
    if paths.is_empty() {
        for line in patch.lines() {
            if let Some(rest) = line.strip_prefix("diff --git ") {
                // Format: "a/path b/path"
                if let Some(b_idx) = rest.find(" b/") {
                    let path = &rest[b_idx + 3..];
                    let path = path.trim();
                    if !path.is_empty() {
                        paths.push(path.to_string());
                    }
                }
            }
        }
    }
    // Fall back to Codex *** Add/Update File: headers
    if paths.is_empty() {
        for line in patch.lines() {
            let rest = line
                .strip_prefix("*** Add File: ")
                .or_else(|| line.strip_prefix("*** Update File: "))
                .or_else(|| line.strip_prefix("*** Delete File: "));
            if let Some(file_path) = rest {
                let file_path = file_path.trim();
                if !file_path.is_empty() {
                    paths.push(file_path.to_string());
                }
            }
        }
    }
    paths
}

/// Split a multi-file unified diff into per-file (path, added, removed) tuples.
fn split_patch_by_file(patch: &str, paths: &[String]) -> Vec<(String, u64, u64)> {
    // Split on `diff --git` boundaries or `--- a/` boundaries
    let mut results = Vec::new();
    let mut current_path_idx: Option<usize> = None;
    let mut current_added: u64 = 0;
    let mut current_removed: u64 = 0;

    for line in patch.lines() {
        if line.starts_with("diff --git ")
            || line.starts_with("--- a/")
            || line.starts_with("--- ")
            || line.starts_with("*** Add File: ")
            || line.starts_with("*** Update File: ")
            || line.starts_with("*** Delete File: ")
        {
            // Check if this starts a new file section
            if let Some(new_idx) = paths.iter().position(|p| line.contains(p)) {
                // Flush previous file
                if let Some(idx) = current_path_idx {
                    results.push((paths[idx].clone(), current_added, current_removed));
                }
                current_path_idx = Some(new_idx);
                current_added = 0;
                current_removed = 0;
                continue;
            }
        }
        if current_path_idx.is_some() {
            if line.starts_with('+') && !line.starts_with("+++") {
                current_added += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                current_removed += 1;
            }
        }
    }
    // Flush last file
    if let Some(idx) = current_path_idx {
        results.push((paths[idx].clone(), current_added, current_removed));
    }

    // If splitting failed, fall back to one entry per path with even distribution
    if results.is_empty() && !paths.is_empty() {
        let (total_added, total_removed) = count_diff_lines(patch);
        for path in paths {
            results.push((
                path.clone(),
                total_added / paths.len() as u64,
                total_removed / paths.len() as u64,
            ));
        }
    }

    results
}

type CodexParseResult = (Vec<ParsedEntry>, Vec<ParsedChangeEvent>, usize, bool);

fn parse_codex_session_file(path: &Path) -> CodexParseResult {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("Failed to open session file {}: {e}", path.display());
            }
            return (Vec::new(), Vec::new(), 0, false);
        }
    };
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut change_events = Vec::new();
    let mut previous_totals: Option<CodexRawUsage> = None;
    let mut current_model: Option<String> = None;
    let mut pending_entry_model_indices = Vec::new();
    let mut pending_change_model_indices = Vec::new();
    let mut lines_read = 0;
    let mut parse_failures = 0_usize;
    let mut session_key = format!("codex-file:{}", path_to_string(path));
    let mut agent_scope = crate::stats::subagent::AgentScope::Main;

    for line in reader.lines() {
        lines_read += 1;
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };

        let entry: CodexJsonlEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => {
                parse_failures += 1;
                continue;
            }
        };

        if entry.entry_type == "session_meta" {
            if let Some(payload) = entry.payload.as_ref() {
                if let Some(id) = payload.get("id").and_then(Value::as_str) {
                    session_key = format!("codex:{id}");
                }
                if payload.pointer("/source/subagent").is_some() {
                    agent_scope = crate::stats::subagent::AgentScope::Subagent;
                }
            }
            continue;
        }

        if entry.entry_type == "turn_context" {
            if let Some(payload) = entry.payload.as_ref() {
                if let Some(model) = extract_codex_model(payload) {
                    current_model = Some(model);
                    assign_pending_codex_models(
                        current_model.as_deref().unwrap_or("gpt-5"),
                        &mut entries,
                        &mut pending_entry_model_indices,
                        &mut change_events,
                        &mut pending_change_model_indices,
                    );
                }
            }
            continue;
        }

        // Accept both "event_msg" and "response_item" — newer Codex CLI versions
        // emit apply_patch tool calls as "response_item" entries.
        if entry.entry_type != "event_msg" && entry.entry_type != "response_item" {
            continue;
        }

        let payload = match entry.payload.as_ref() {
            Some(p) => p,
            None => continue,
        };
        // Check for apply_patch tool calls (change events)
        let payload_type = payload.get("type").and_then(Value::as_str).unwrap_or("");
        if payload_type == "function_call"
            || payload_type == "custom_tool_call"
            || payload_type == "tool_call"
        {
            let model_raw = extract_codex_model(payload).or_else(|| current_model.clone());
            if let Some(model) = model_raw.as_ref() {
                current_model = Some(model.clone());
                assign_pending_codex_models(
                    model,
                    &mut entries,
                    &mut pending_entry_model_indices,
                    &mut change_events,
                    &mut pending_change_model_indices,
                );
            }

            let tool_name = payload
                .get("name")
                .or_else(|| payload.get("function"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if tool_name == "apply_patch" || tool_name.ends_with("apply_patch") {
                let patch_content = payload
                    .get("arguments")
                    .or_else(|| payload.get("content"))
                    .or_else(|| payload.get("input"))
                    .and_then(|v| {
                        // Could be a string directly or a JSON object with a "patch" key
                        v.as_str()
                            .map(String::from)
                            .or_else(|| v.get("patch").and_then(Value::as_str).map(String::from))
                    });

                if let Some(patch) = patch_content {
                    let ts_str = entry.timestamp.as_deref().unwrap_or("");
                    if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                        let ts = ts.with_timezone(&Local);
                        let model_key = model_raw
                            .as_deref()
                            .map(crate::models::normalized_model_key);
                        let paths = extract_diff_paths(&patch);
                        let (total_added, total_removed) = count_diff_lines(&patch);

                        if paths.is_empty() {
                            // Single file or unparseable diff — emit one event
                            if total_added > 0 || total_removed > 0 {
                                push_codex_change_event(
                                    &mut change_events,
                                    &mut pending_change_model_indices,
                                    model_key.as_deref(),
                                    ts,
                                    String::from("unknown"),
                                    ChangeEventKind::PatchEdit,
                                    total_added,
                                    total_removed,
                                    agent_scope,
                                );
                            }
                        } else if paths.len() == 1 {
                            push_codex_change_event(
                                &mut change_events,
                                &mut pending_change_model_indices,
                                model_key.as_deref(),
                                ts,
                                paths[0].clone(),
                                ChangeEventKind::PatchEdit,
                                total_added,
                                total_removed,
                                agent_scope,
                            );
                        } else {
                            // Multiple files in one patch — split by file
                            // Re-parse per-file diffs using `diff --git` or `--- a/` separators
                            let file_diffs = split_patch_by_file(&patch, &paths);
                            for (file_path, file_added, file_removed) in file_diffs {
                                push_codex_change_event(
                                    &mut change_events,
                                    &mut pending_change_model_indices,
                                    model_key.as_deref(),
                                    ts,
                                    file_path,
                                    ChangeEventKind::PatchEdit,
                                    file_added,
                                    file_removed,
                                    agent_scope,
                                );
                            }
                        }
                    }
                }
            }
            continue;
        }

        if payload_type != "token_count" {
            continue;
        }

        let info = payload.get("info");
        let last_usage =
            normalize_codex_raw_usage(info.and_then(|value| value.get("last_token_usage")));
        let total_usage =
            normalize_codex_raw_usage(info.and_then(|value| value.get("total_token_usage")));

        let raw_usage = if let Some(total_usage) = total_usage {
            subtract_codex_raw_usage(total_usage, previous_totals)
        } else if let Some(last_usage) = last_usage {
            last_usage
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
            assign_pending_codex_models(
                model,
                &mut entries,
                &mut pending_entry_model_indices,
                &mut change_events,
                &mut pending_change_model_indices,
            );
        }

        let model = extracted_model.or_else(|| current_model.clone());

        let uncached_input_tokens = raw_usage
            .input_tokens
            .saturating_sub(raw_usage.cached_input_tokens);

        let entry_model = model.unwrap_or_default();
        entries.push(ParsedEntry {
            timestamp: ts,
            model: entry_model,
            input_tokens: uncached_input_tokens,
            output_tokens: raw_usage.output_tokens,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: raw_usage.cached_input_tokens,
            unique_hash: None,
            session_key: session_key.clone(),
            agent_scope,
        });
        if entries.last().is_some_and(|entry| entry.model.is_empty()) {
            pending_entry_model_indices.push(entries.len() - 1);
        }
    }

    assign_pending_codex_models(
        "gpt-5",
        &mut entries,
        &mut pending_entry_model_indices,
        &mut change_events,
        &mut pending_change_model_indices,
    );

    entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    if parse_failures > 0 && entries.is_empty() && lines_read > 10 {
        tracing::warn!(
            "All {} lines failed to parse in {}; JSONL schema may have changed",
            parse_failures,
            path.display()
        );
    }

    (entries, change_events, lines_read, true)
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
        let (parsed_entries, _change_events, lines_read, opened) = parse_codex_session_file(&path);
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
    ]
}

fn usage_integration_configs_with_overrides(
    claude_roots: Option<Vec<PathBuf>>,
    codex_roots: Option<Vec<PathBuf>>,
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
        }
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
        ))
    }

    /// Create with an explicit Codex sessions directory (for testing).
    #[allow(dead_code)]
    pub fn with_codex_dir(codex_dir: PathBuf) -> Self {
        Self::from_integrations(usage_integration_configs_with_overrides(
            None,
            Some(vec![codex_dir]),
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
        ))
    }

    // ── Cache helpers ──

    #[allow(dead_code)]
    pub fn clear_cache(&self) {
        self.clear_payload_cache();
        if let Ok(mut c) = self.file_cache.lock() {
            c.clear();
        }
        if let Ok(mut c) = self.root_file_lists.lock() {
            c.clear();
        }
        if let Ok(mut current) = self.last_query_debug.lock() {
            *current = None;
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
        !entry.directories.is_empty()
            && entry.directories.iter().all(|directory| {
                fs::metadata(&directory.path)
                    .and_then(|metadata| metadata.modified())
                    .map(|modified| modified == directory.modified)
                    .unwrap_or(false)
            })
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
        let files: Arc<[PathBuf]> = files.into();
        let directories: Arc<[DirectoryStamp]> = directories.into();

        if !directories.is_empty() {
            if let Ok(mut cache) = self.root_file_lists.lock() {
                let now = Instant::now();
                cache.insert(
                    cache_key,
                    CachedRootFileList {
                        files: files.clone(),
                        directories,
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

        for integration_id in selection.integration_ids() {
            match integration_id {
                UsageIntegrationId::Claude => {
                    let (next_entries, next_change_events, next_reports) =
                        self.load_claude_entries_with_debug(since);
                    entries.extend(next_entries);
                    change_events.extend(next_change_events);
                    reports.extend(next_reports);
                }
                UsageIntegrationId::Codex => {
                    let (next_entries, next_change_events, next_report) =
                        self.load_codex_entries_with_debug(since);
                    entries.extend(next_entries);
                    change_events.extend(next_change_events);
                    reports.push(next_report);
                }
            }
        }

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
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            usage_source: UsageSource::Parser,
            usage_warning: None,
            period_label: String::new(),
            has_earlier_data: false,
            change_stats: None,
            subagent_stats: None,
            device_breakdown: None,
            device_chart_buckets: None,
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
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            usage_source: UsageSource::Parser,
            usage_warning: None,
            period_label: String::new(),
            has_earlier_data: false,
            change_stats: None,
            subagent_stats: None,
            device_breakdown: None,
            device_chart_buckets: None,
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
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            usage_source: UsageSource::Parser,
            usage_warning: None,
            period_label: String::new(),
            has_earlier_data: false,
            change_stats: None,
            subagent_stats: None,
            device_breakdown: None,
            device_chart_buckets: None,
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
            input_tokens: total_input,
            output_tokens: total_output,
            chart_buckets,
            model_breakdown,
            active_block,
            five_hour_cost,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            usage_source: UsageSource::Parser,
            usage_warning: None,
            period_label: String::new(),
            has_earlier_data: false,
            change_stats: None,
            subagent_stats: None,
            device_breakdown: None,
            device_chart_buckets: None,
        }
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
