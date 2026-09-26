use chrono::{DateTime, Local, NaiveDate};
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::stats::change::{classify_file, ChangeEventKind, ParsedChangeEvent};

use super::parser::{
    count_diff_lines, glob_jsonl_files, modified_since, path_to_string, push_sample_path,
    DiffLineCounter, ParsedEntry, ProviderReadDebug, SessionParseResult,
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

/// Billable output for one Codex usage record.
///
/// ponytail: logs mix two layouts. `total_tokens ≈ input+output` → reasoning is
/// already inside `output_tokens` (A, do not add). `total_tokens ≈
/// input+output+reasoning` → reasoning is extra (B, add). Missing total is
/// treated as A (every fixture in this repo). Equal distance prefers A so we
/// don't double-count.
fn billed_output_tokens(usage: CodexRawUsage) -> u64 {
    let without_reasoning = usage.input_tokens.saturating_add(usage.output_tokens);
    let with_reasoning = without_reasoning.saturating_add(usage.reasoning_output_tokens);
    if usage.reasoning_output_tokens > 0 && usage.total_tokens > 0 {
        let dist_a = usage.total_tokens.abs_diff(without_reasoning);
        let dist_b = usage.total_tokens.abs_diff(with_reasoning);
        if dist_b < dist_a {
            return usage
                .output_tokens
                .saturating_add(usage.reasoning_output_tokens);
        }
    }
    usage.output_tokens
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
    session_key: &str,
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
        session_key: session_key.to_string(),
    });

    if model_key.is_none() {
        pending_model_indices.push(change_events.len() - 1);
    }
}

/// Emit one change event per path in a Codex `FileChange` item. `changes`
/// maps each path to `{type: "add" | "delete", content}` or
/// `{type: "update", unified_diff}`. Items that did not complete are skipped.
fn push_codex_file_change_events(
    item: Option<&Value>,
    timestamp: Option<&str>,
    current_model: Option<&str>,
    agent_scope: crate::stats::subagent::AgentScope,
    session_key: &str,
    change_events: &mut Vec<ParsedChangeEvent>,
    pending_model_indices: &mut Vec<usize>,
) {
    let Some(item) = item else { return };
    if item
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status != "completed")
    {
        return;
    }
    let Some(changes) = item.get("changes").and_then(Value::as_object) else {
        return;
    };
    let Some(ts) = timestamp.and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok()) else {
        return;
    };
    let ts = ts.with_timezone(&Local);
    let model_key = current_model.map(crate::models::normalized_model_key);

    for (path, change) in changes {
        let text = |key: &str| change.get(key).and_then(Value::as_str).unwrap_or("");
        let (added, removed) = match change.get("type").and_then(Value::as_str) {
            Some("add") => (text("content").lines().count() as u64, 0),
            Some("delete") => (0, text("content").lines().count() as u64),
            Some("update") => count_diff_lines(text("unified_diff")),
            _ => (0, 0),
        };
        if added == 0 && removed == 0 {
            continue;
        }
        push_codex_change_event(
            change_events,
            pending_model_indices,
            model_key.as_deref(),
            ts,
            path.clone(),
            ChangeEventKind::PatchEdit,
            added,
            removed,
            agent_scope,
            session_key,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Diff helpers (used only by Codex patch parsing)
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the paths recognized by the same parser used for line attribution.
#[cfg(test)]
pub(crate) fn extract_diff_paths(patch: &str) -> Vec<String> {
    split_patch_by_file(patch)
        .into_iter()
        .map(|(path, _, _)| path)
        .filter(|path| path != "unknown")
        .collect()
}

/// Attribute each hunk to its exact file header. Unattributed lines stay
/// unknown; distributing them across known files would invent categories.
fn split_patch_by_file(patch: &str) -> Vec<(String, u64, u64)> {
    let mut results = Vec::new();
    let mut path = None;
    let (mut added, mut removed) = (0, 0);
    let mut counter = DiffLineCounter::default();
    let mut saw_old_header = false;

    let flush =
        |results: &mut Vec<_>, path: &mut Option<String>, added: &mut u64, removed: &mut u64| {
            if path.is_some() || *added > 0 || *removed > 0 {
                results.push((
                    path.take().unwrap_or_else(|| "unknown".to_string()),
                    std::mem::take(added),
                    std::mem::take(removed),
                ));
            }
        };
    let header_path = |header: &str, prefix: &str| {
        // Unified diff timestamps, when present, follow a tab.
        let value = header.split('\t').next().unwrap_or("").trim();
        (!value.is_empty() && value != "/dev/null")
            .then(|| value.strip_prefix(prefix).unwrap_or(value).to_string())
    };

    for line in patch.lines() {
        let codex_path = line
            .strip_prefix("*** Add File: ")
            .or_else(|| line.strip_prefix("*** Update File: "))
            .or_else(|| line.strip_prefix("*** Delete File: "));
        if let Some(file_path) = codex_path {
            flush(&mut results, &mut path, &mut added, &mut removed);
            path = header_path(file_path, "");
            counter = DiffLineCounter::default();
            counter.count_line(line);
            saw_old_header = false;
        } else if let Some(header) = line.strip_prefix("diff --git ") {
            flush(&mut results, &mut path, &mut added, &mut removed);
            path = header
                .split_once(" b/")
                .and_then(|(_, value)| header_path(value, ""));
            counter = DiffLineCounter::default();
            saw_old_header = false;
        } else if !counter.in_hunk() && line.starts_with("--- ") {
            // The first old-file header belongs to the preceding `diff --git`.
            // Later pairs can begin files without a `diff --git` separator.
            if saw_old_header || added > 0 || removed > 0 {
                flush(&mut results, &mut path, &mut added, &mut removed);
            }
            path = header_path(&line[4..], "a/");
            counter = DiffLineCounter::default();
            saw_old_header = true;
        } else if !counter.in_hunk() && line.starts_with("+++ ") {
            // Deletions have no new path: retain their old-file header.
            if let Some(new_path) = header_path(&line[4..], "b/") {
                path = Some(new_path);
            }
        } else {
            let (line_added, line_removed) = counter.count_line(line);
            added += line_added;
            removed += line_removed;
        }
    }
    flush(&mut results, &mut path, &mut added, &mut removed);
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
    let mut apply_patch_event_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut saw_file_change_items = false;
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
                    // Spawned subagents take the same `<parent>:subagent:<id>`
                    // shape as Claude's, so their edits and cost group with
                    // the conversation that spawned them.
                    let parent = payload
                        .pointer("/source/subagent/thread_spawn/parent_thread_id")
                        .and_then(Value::as_str);
                    session_key = match parent {
                        Some(parent) => format!("codex:{parent}:subagent:{id}"),
                        None => format!("codex:{id}"),
                    };
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
        let payload_type = payload.get("type").and_then(Value::as_str).unwrap_or("");

        // Codex records every applied edit as a FileChange item, whichever
        // tool made it. Current CLIs edit through `exec` (JS calling
        // `tools.apply_patch`), so this is the only place those edits show up.
        if payload_type == "item_completed" {
            let item = payload.get("item");
            if item
                .and_then(|item| item.get("type"))
                .and_then(Value::as_str)
                == Some("FileChange")
            {
                saw_file_change_items = true;
                push_codex_file_change_events(
                    item,
                    entry.timestamp.as_deref(),
                    current_model.as_deref(),
                    agent_scope,
                    &session_key,
                    &mut change_events,
                    &mut pending_change_model_indices,
                );
            }
            continue;
        }

        // Check for apply_patch tool calls (change events)
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
                let first_patch_event = change_events.len();
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
                        for (file_path, file_added, file_removed) in split_patch_by_file(&patch) {
                            if file_added == 0 && file_removed == 0 {
                                continue;
                            }
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
                                &session_key,
                            );
                        }
                    }
                }
                apply_patch_event_ranges.push(first_patch_event..change_events.len());
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
            output_tokens: billed_output_tokens(raw_usage),
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

    // Sessions that log FileChange items also log the apply_patch call that
    // produced each one. Keep the FileChange copy so an edit counts once.
    if saw_file_change_items && !apply_patch_event_ranges.is_empty() {
        let mut keep = vec![true; change_events.len()];
        for range in apply_patch_event_ranges {
            keep[range].fill(false);
        }
        let mut keep = keep.into_iter();
        change_events.retain(|_| keep.next().unwrap_or(true));
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_patch_changes(patch: &str) -> Vec<ParsedChangeEvent> {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("session.jsonl");
        let context = serde_json::json!({
            "type": "turn_context", "payload": {"model": "gpt-5.4"}
        });
        let call = serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-03-21T10:00:00+00:00",
            "payload": {"type": "custom_tool_call", "name": "apply_patch", "input": patch}
        });
        fs::write(&path, format!("{context}\n{call}\n")).unwrap();
        parse_codex_session_file(&path).1
    }

    #[test]
    fn diff_attribution_keeps_prefix_filenames_separate() {
        let patches = [
            "*** Begin Patch\n*** Update File: sample.py\n@@\n-old\n+new\n*** Add File: sample.py.md\n+doc 1\n+doc 2\n+doc 3\n*** End Patch\n",
            "diff --git a/sample.py b/sample.py\n--- a/sample.py\n+++ b/sample.py\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/sample.py.md b/sample.py.md\n--- /dev/null\n+++ b/sample.py.md\n@@ -0,0 +1,3 @@\n+doc 1\n+doc 2\n+doc 3\n",
        ];
        for patch in patches {
            let changes = parse_patch_changes(patch);
            let actual: Vec<_> = changes
                .iter()
                .map(|event| {
                    (
                        event.path.as_str(),
                        event.added_lines,
                        event.removed_lines,
                        event.category,
                    )
                })
                .collect();
            assert_eq!(
                actual,
                vec![
                    ("sample.py", 1, 1, crate::stats::change::FileCategory::Code),
                    (
                        "sample.py.md",
                        3,
                        0,
                        crate::stats::change::FileCategory::Docs
                    ),
                ]
            );
        }
    }

    #[test]
    fn diff_attribution_preserves_deleted_files_in_mixed_patch() {
        let patch = "diff --git a/README.md b/README.md\n--- a/README.md\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-doc 1\n-doc 2\ndiff --git a/main.rs b/main.rs\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let changes = parse_patch_changes(patch);
        let actual: Vec<_> = changes
            .iter()
            .map(|event| {
                (
                    event.path.as_str(),
                    event.added_lines,
                    event.removed_lines,
                    event.category,
                )
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                ("README.md", 0, 2, crate::stats::change::FileCategory::Docs),
                ("main.rs", 1, 1, crate::stats::change::FileCategory::Code),
            ]
        );
    }

    #[test]
    fn diff_attribution_keeps_header_like_content_in_its_file() {
        let patch = "--- a/first.md\n+++ b/first.md\n@@ -1 +1 @@\n--- old\n+++ new\n--- a/second.rs\n+++ b/second.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let changes = parse_patch_changes(patch);
        let actual: Vec<_> = changes
            .iter()
            .map(|event| (event.path.as_str(), event.added_lines, event.removed_lines))
            .collect();
        assert_eq!(actual, vec![("first.md", 1, 1), ("second.rs", 1, 1)]);
    }

    #[test]
    fn diff_attribution_preserves_header_like_codex_additions() {
        // Codex Add File sections do not have an @@ hunk marker.
        let patch = "*** Begin Patch\n*** Add File: sample.md\n+++ heading\n+---\n*** End Patch\n";
        assert_eq!(count_diff_lines(patch), (2, 0));
        let changes = parse_patch_changes(patch);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "sample.md");
        assert_eq!((changes[0].added_lines, changes[0].removed_lines), (2, 0));
    }

    #[test]
    fn diff_attribution_keeps_unattributed_lines_unknown() {
        let patch = "+unattributed 1\n+unattributed 2\n+unattributed 3\n*** Update File: main.rs\n@@\n-old\n+new\n";
        let changes = parse_patch_changes(patch);
        let actual: Vec<_> = changes
            .iter()
            .map(|event| (event.path.as_str(), event.added_lines, event.removed_lines))
            .collect();
        assert_eq!(actual, vec![("unknown", 3, 0), ("main.rs", 1, 1)]);
    }

    fn usage(input: u64, output: u64, reasoning: u64, total: u64) -> CodexRawUsage {
        CodexRawUsage {
            input_tokens: input,
            cached_input_tokens: 0,
            output_tokens: output,
            reasoning_output_tokens: reasoning,
            total_tokens: total,
        }
    }

    #[test]
    fn reasoning_billing_follows_total_tokens() {
        // A: total = input+output → reasoning already in output.
        assert_eq!(billed_output_tokens(usage(100, 10, 5, 110)), 10);
        // B: total = input+output+reasoning → reasoning is extra.
        assert_eq!(billed_output_tokens(usage(100, 10, 5, 115)), 15);
        // Missing total (normalized to input+output elsewhere) → A.
        assert_eq!(billed_output_tokens(usage(100, 10, 5, 110)), 10);
    }
}
