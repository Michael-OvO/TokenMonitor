use crate::stats::change::{classify_file, ChangeEventKind, ParsedChangeEvent};
use crate::stats::subagent::AgentScope;
use chrono::{DateTime, Local};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::parser::{
    count_diff_lines, glob_jsonl_files, modified_since, path_to_string, push_sample_path,
    ParsedEntry, ProviderReadDebug,
};

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
    server_tool_use: Option<ServerToolUse>,
    speed: Option<String>,
}

#[derive(Deserialize)]
struct ServerToolUse {
    web_search_requests: Option<u64>,
}

#[derive(Deserialize)]
struct CacheCreationBreakdown {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Claude-specific helper functions
// ─────────────────────────────────────────────────────────────────────────────

fn create_claude_unique_hash(entry: &ClaudeJsonlEntry) -> Option<String> {
    let message_id = entry.message.as_ref()?.id.as_ref()?;
    match entry.request_id.as_ref() {
        Some(request_id) => Some(format!("{message_id}:{request_id}")),
        None => Some(message_id.clone()),
    }
}

fn claude_scope_priority(scope: AgentScope) -> u8 {
    match scope {
        AgentScope::Main => 0,
        AgentScope::Subagent => 1,
    }
}

pub(crate) fn should_prefer_claude_entry(candidate: &ParsedEntry, existing: &ParsedEntry) -> bool {
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

pub(crate) fn should_prefer_claude_change_event(
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
    agent_scope: AgentScope,
}

// ─────────────────────────────────────────────────────────────────────────────
// Claude change event / tool extraction helpers
// ─────────────────────────────────────────────────────────────────────────────

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

// ─────────────────────────────────────────────────────────────────────────────
// Claude session file parser
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) type ClaudeParseResult = (Vec<ParsedEntry>, Vec<ParsedChangeEvent>, usize, bool);

pub(crate) fn parse_claude_session_file(path: &Path) -> ClaudeParseResult {
    tracing::debug!(path = %path.display(), "opening file (claude session)");
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
                    AgentScope::Subagent
                } else {
                    AgentScope::Main
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
                    // Append "-fast" to the raw model name when speed is "fast"
                    // so it gets a distinct model key and pricing.
                    let effective_model = if usage.speed.as_deref() == Some("fast") {
                        format!("{model}-fast")
                    } else {
                        model
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

                    let web_search_requests = usage
                        .server_tool_use
                        .as_ref()
                        .and_then(|s| s.web_search_requests)
                        .unwrap_or(0);

                    entries.push(ParsedEntry {
                        timestamp: ts,
                        model: effective_model,
                        input_tokens: usage.input_tokens.unwrap_or(0),
                        output_tokens: usage.output_tokens.unwrap_or(0),
                        cache_creation_5m_tokens: cw_5m,
                        cache_creation_1h_tokens: cw_1h,
                        cache_read_tokens: usage.cache_read_input_tokens.unwrap_or(0),
                        web_search_requests,
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

// ─────────────────────────────────────────────────────────────────────────────
// Claude dedup types and functions
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) enum ClaudeDedupeAction {
    Inserted,
    Replaced(usize),
    Skipped,
}

pub(crate) fn upsert_claude_entry(
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

pub(crate) fn upsert_claude_change_event(
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
pub(crate) fn read_claude_entries_with_debug(
    projects_dir: &Path,
    since: Option<chrono::NaiveDate>,
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
pub(crate) fn read_claude_entries(
    projects_dir: &Path,
    since: Option<chrono::NaiveDate>,
) -> Vec<ParsedEntry> {
    read_claude_entries_with_debug(projects_dir, since).0
}

// Re-export count_lines and is_provider_internal_path for tests in parser.rs
#[cfg(test)]
pub(crate) fn test_count_lines(s: &str) -> u64 {
    count_lines(s)
}

#[cfg(test)]
pub(crate) fn test_is_provider_internal_path(path: &str) -> bool {
    is_provider_internal_path(path)
}
