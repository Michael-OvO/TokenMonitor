use chrono::{DateTime, Local, NaiveDate};
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::stats::change::{classify_file, ChangeEventKind, ParsedChangeEvent};

use super::parser::{
    count_diff_lines, glob_jsonl_files, modified_since, path_to_string, push_sample_path,
    ParsedEntry, ProviderReadDebug, SessionParseResult,
};

// ─────────────────────────────────────────────────────────────────────────────
// Codex JSONL serde types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct CodexJsonlEntry {
    #[serde(rename = "type", default)]
    entry_type: String,
    timestamp: Option<String>,
    payload: Option<Value>,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct CodexRawUsage {
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    total_tokens: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Codex helper functions
// ─────────────────────────────────────────────────────────────────────────────

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
// Diff helpers (used only by Codex patch parsing)
// ─────────────────────────────────────────────────────────────────────────────

/// Extract file paths from unified diff headers.
/// Looks for `+++ b/path` lines and strips the `b/` prefix.
/// Falls back to `diff --git a/path b/path` headers.
pub(crate) fn extract_diff_paths(patch: &str) -> Vec<String> {
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

// ─────────────────────────────────────────────────────────────────────────────
// Codex session file parser
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a single Codex session JSONL file.
/// Codex `event_msg` / `token_count` events may include either per-turn
/// `last_token_usage` or cumulative `total_token_usage`. We normalize both
/// forms into per-event deltas and track model context via `turn_context`.
///
/// In current Codex logs, `input_tokens` already includes cached input.
/// Normalize it to billable uncached input here so downstream pricing and
/// token totals do not count cached input twice.
pub(crate) fn parse_codex_session_file(path: &Path) -> SessionParseResult {
    tracing::debug!(path = %path.display(), "opening file (codex session)");
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
            web_search_requests: 0,
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

    entries.sort_by_key(|a| a.timestamp);

    if parse_failures > 0 && entries.is_empty() && lines_read > 10 {
        tracing::warn!(
            "All {} lines failed to parse in {}; JSONL schema may have changed",
            parse_failures,
            path.display()
        );
    }

    (entries, change_events, lines_read, true)
}

// ─────────────────────────────────────────────────────────────────────────────
// Codex directory reader
// ─────────────────────────────────────────────────────────────────────────────

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

    entries.sort_by_key(|a| a.timestamp);
    report.emitted_entries = entries.len();
    (entries, report)
}

#[allow(dead_code)]
pub fn read_codex_entries(sessions_dir: &Path, since: Option<NaiveDate>) -> Vec<ParsedEntry> {
    read_codex_entries_with_debug(sessions_dir, since).0
}
