//! Parser for Gemini CLI chat logs.
//!
//! The CLI records each session under its data home as
//! `~/.gemini/tmp/<project>/chats/session-*.json`: one JSON document whose
//! `messages` array carries a `tokens` object on every model reply:
//!
//! ```json
//! {"id":"…","timestamp":"2026-01-02T03:04:05.000Z","type":"gemini","model":"gemini-2.5-pro",
//!  "tokens":{"input":1200,"output":80,"cached":1000,"thoughts":20,"tool":0,"total":1300}}
//! ```
//!
//! Newer CLIs append the same message objects one per line to a `.jsonl` file
//! and rewrite a message by appending it again under the same `id`, so the
//! last record for an id wins in both shapes.

use chrono::{DateTime, Local};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use super::parser::{path_to_string, ParsedEntry, SessionParseResult};

fn message_entry(message: &Value, session_key: &str) -> Option<ParsedEntry> {
    if message.get("type")?.as_str()? != "gemini" {
        return None;
    }
    let tokens = message.get("tokens")?;
    let count = |key: &str| tokens.get(key).and_then(Value::as_u64).unwrap_or(0);
    let timestamp = DateTime::parse_from_rfc3339(message.get("timestamp")?.as_str()?)
        .ok()?
        .with_timezone(&Local);

    // `input` is the API's promptTokenCount, which includes the cached part.
    let cached = count("cached");
    let input_tokens = count("input").saturating_sub(cached) + count("tool");
    let output_tokens = count("output") + count("thoughts");
    if input_tokens + output_tokens + cached == 0 {
        return None;
    }

    Some(ParsedEntry {
        timestamp,
        model: message
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
            .unwrap_or("gemini-unknown")
            .to_string(),
        input_tokens,
        output_tokens,
        cache_creation_5m_tokens: 0,
        cache_creation_1h_tokens: 0,
        cache_read_tokens: cached,
        web_search_requests: 0,
        billed_cost: None,
        unique_hash: None,
        session_key: session_key.to_string(),
        agent_scope: crate::stats::subagent::AgentScope::Main,
        // ponytail: no project attribution; the chat log only has a project
        // hash. Read `~/.gemini/projects.json` if per-project stats matter.
        session_meta: None,
    })
}

/// Entries plus the number of records read, from either log shape.
fn parse_gemini_session_text(text: &str, session_key: &str) -> (Vec<ParsedEntry>, usize) {
    let records: Vec<Value> = match serde_json::from_str::<Value>(text) {
        Ok(mut document) => match document.get_mut("messages").map(Value::take) {
            Some(Value::Array(messages)) => messages,
            _ => vec![document],
        },
        Err(_) => text
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect(),
    };

    let mut entries: Vec<ParsedEntry> = Vec::new();
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    for record in &records {
        let Some(entry) = message_entry(record, session_key) else {
            continue;
        };
        match record.get("id").and_then(Value::as_str) {
            Some(id) => match by_id.get(id) {
                Some(&index) => entries[index] = entry,
                None => {
                    by_id.insert(id, entries.len());
                    entries.push(entry);
                }
            },
            None => entries.push(entry),
        }
    }
    entries.sort_by_key(|entry| entry.timestamp);
    (entries, records.len())
}

pub(crate) fn parse_gemini_session_file(path: &Path) -> SessionParseResult {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (Vec::new(), Vec::new(), 0, true);
    };
    let session_key = format!("gemini-file:{}", path_to_string(path));
    let (entries, records) = parse_gemini_session_text(&text, &session_key);
    (entries, Vec::new(), records, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = r#"{"id":"m2","timestamp":"2026-01-02T03:04:05.000Z","type":"gemini","model":"gemini-2.5-pro","tokens":{"input":1200,"output":80,"cached":1000,"thoughts":20,"tool":5,"total":1305}}"#;

    #[test]
    fn json_document_splits_cached_input_and_adds_thoughts() {
        let text = format!(
            r#"{{"sessionId":"s","messages":[{{"id":"m1","timestamp":"2026-01-02T03:04:00.000Z","type":"user","content":"hi"}},{REPLY}]}}"#
        );
        let (entries, records) = parse_gemini_session_text(&text, "k");
        assert_eq!(records, 2);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.model, "gemini-2.5-pro");
        assert_eq!(entry.input_tokens, 205);
        assert_eq!(entry.output_tokens, 100);
        assert_eq!(entry.cache_read_tokens, 1000);
    }

    #[test]
    fn jsonl_keeps_the_last_record_per_message_id() {
        let rewritten = REPLY.replace("\"output\":80", "\"output\":90");
        let text = format!("{{\"sessionId\":\"s\"}}\n{REPLY}\n{rewritten}\nnot json\n");
        let (entries, _) = parse_gemini_session_text(&text, "k");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].output_tokens, 110);
    }
}
