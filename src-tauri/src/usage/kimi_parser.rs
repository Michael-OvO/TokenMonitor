//! Parser for Kimi Code CLI usage logs.
//!
//! The Kimi CLI (`kimi-code`, and the legacy `kimi-cli`) records every request
//! it makes to a per-session `wire.jsonl` file under its data home, e.g.
//! `~/.kimi-code/sessions/<workspace>/<session>/agents/<agent>/wire.jsonl`.
//! Token accounting rides on `usage.record` lines:
//!
//! ```json
//! {"type":"usage.record","model":"kimi-code/kimi-for-coding",
//!  "usage":{"inputOther":1163,"output":352,"inputCacheRead":22272,"inputCacheCreation":0},
//!  "usageScope":"turn","time":1780410897480}
//! ```
//!
//! Only turn-scoped records are counted — session-scoped records repeat the
//! session's cumulative totals and would double-count if summed. Field names are
//! accepted in both the wire camelCase form (`inputOther`) and the snake_case
//! form the CLI's `StatusUpdate` events use (`input_other`), so both the current
//! and legacy log shapes parse.

use chrono::{DateTime, Local, TimeZone, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::parser::{path_to_string, ParsedEntry, SessionParseResult};

/// Read a `u64` from the first present key, tolerating stringified numbers.
fn read_u64(usage: &serde_json::Map<String, Value>, keys: &[&str]) -> u64 {
    for key in keys {
        if let Some(value) = usage.get(*key) {
            if let Some(n) = value.as_u64() {
                return n;
            }
            if let Some(n) = value.as_f64() {
                if n >= 0.0 {
                    return n as u64;
                }
            }
            if let Some(n) = value.as_str().and_then(|s| s.trim().parse::<u64>().ok()) {
                return n;
            }
        }
    }
    0
}

/// Strip a provider prefix so `kimi-code/kimi-for-coding` becomes
/// `kimi-for-coding`, which classifies as `ModelFamily::Moonshot` and prices
/// off the `kimi-for-coding` table entry. Empty/absent models fall back to the
/// canonical coding-model name.
fn normalize_kimi_model(raw: Option<&str>) -> String {
    let trimmed = raw.map(str::trim).unwrap_or("");
    let stripped = trimmed.rsplit('/').next().unwrap_or(trimmed).trim();
    if stripped.is_empty() {
        "kimi-for-coding".to_string()
    } else {
        stripped.to_string()
    }
}

/// Turn a millisecond epoch (`time`) or an RFC 3339 string (`timestamp`) into a
/// local `DateTime`. Kimi Code emits epoch-millis; the legacy CLI emits RFC 3339.
fn parse_kimi_timestamp(entry: &Value) -> Option<DateTime<Local>> {
    if let Some(ms) = entry.get("time").and_then(Value::as_i64) {
        return match Utc.timestamp_millis_opt(ms) {
            chrono::LocalResult::Single(dt) => Some(dt.with_timezone(&Local)),
            _ => None,
        };
    }
    if let Some(ts) = entry.get("timestamp").and_then(Value::as_str) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
            return Some(dt.with_timezone(&Local));
        }
    }
    None
}

// ── Model display names (from the Kimi CLI's own config) ────────────────────
//
// Usage records carry the selected config alias (`kimi-code/kimi-for-coding`,
// `kimi-code/k3`, …); the real product name is declared in
// `<kimi-home>/config.toml`:
//
// ```toml
// [models."kimi-code/kimi-for-coding"]
// model = "kimi-for-coding"
// display_name = "K2.7 Coding"
// ```
//
// We read that mapping and register it as a display-name override so the tab
// shows "K2.7 Coding" while the model key stays `kimi-for-coding`
// (which pricing and the chart palette rely on).

/// The Kimi CLI data-home directories (parents of `sessions/`), where
/// `config.toml` lives. Honors `KIMI_DATA_DIR` (comma-separated) and falls back
/// to `~/.kimi` and `~/.kimi-code`.
fn kimi_home_dirs() -> Vec<PathBuf> {
    if let Ok(raw) = env::var("KIMI_DATA_DIR") {
        let homes: Vec<PathBuf> = raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .collect();
        if !homes.is_empty() {
            return homes;
        }
    }
    let mut homes = Vec::new();
    if let Some(home) = dirs::home_dir() {
        homes.push(home.join(".kimi"));
        homes.push(home.join(".kimi-code"));
    }
    homes
}

/// Extract a quoted string value for `key` from a `key = "value"` TOML line,
/// requiring the `=` so `model` does not match `model_foo`.
fn toml_string_value(line: &str, key: &str) -> Option<String> {
    let rest = line.trim().strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Pull `(model, display_name)` pairs from every `[models."…"]` table in a Kimi
/// `config.toml`. Deliberately minimal — the config is a flat key=value file, so
/// a full TOML parser would be overkill (and a new dependency).
fn parse_kimi_model_display_names(config_text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_models_table = false;
    let mut current_model: Option<String> = None;
    let mut current_display: Option<String> = None;

    let mut flush = |model: &mut Option<String>, display: &mut Option<String>| {
        if let (Some(m), Some(d)) = (model.take(), display.take()) {
            out.push((m, d));
        } else {
            *model = None;
            *display = None;
        }
    };

    for line in config_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            flush(&mut current_model, &mut current_display);
            in_models_table = trimmed.starts_with("[models.");
            continue;
        }
        if !in_models_table {
            continue;
        }
        if let Some(model) = toml_string_value(trimmed, "model") {
            current_model = Some(model);
        } else if let Some(display) = toml_string_value(trimmed, "display_name") {
            current_display = Some(display);
        }
    }
    flush(&mut current_model, &mut current_display);
    out
}

/// Build the map of normalized model key → display name for Kimi models, read
/// from the CLI's config files. A baseline entry keeps the tab readable even
/// when no config is present; a real config upgrades it to the live model name.
pub fn kimi_model_display_names() -> HashMap<String, String> {
    let mut out = HashMap::new();
    out.insert("kimi-for-coding".to_string(), "Kimi Code".to_string());

    for home in kimi_home_dirs() {
        let config_path = home.join("config.toml");
        let Ok(text) = fs::read_to_string(&config_path) else {
            continue;
        };
        for (model, display) in parse_kimi_model_display_names(&text) {
            let display = display.trim();
            if display.is_empty() {
                continue;
            }
            out.insert(
                crate::models::normalized_model_key(&model),
                display.to_string(),
            );
        }
    }
    out
}

/// Parse a single Kimi `wire.jsonl` session file into per-turn token entries.
pub(crate) fn parse_kimi_session_file(path: &Path) -> SessionParseResult {
    tracing::debug!(path = %path.display(), "opening file (kimi session)");
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
    let mut lines_read = 0;
    let mut parse_failures = 0_usize;
    let session_key = format!("kimi-file:{}", path_to_string(path));

    for line in reader.lines() {
        lines_read += 1;
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        let entry: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                parse_failures += 1;
                continue;
            }
        };

        // Only usage.record lines carry token accounting.
        if entry.get("type").and_then(Value::as_str) != Some("usage.record") {
            continue;
        }

        // Session-scoped records are cumulative totals — skip them so we don't
        // double-count. Absent scope is treated as turn-scoped (the common case).
        if entry.get("usageScope").and_then(Value::as_str) == Some("session")
            || entry.get("usage_scope").and_then(Value::as_str) == Some("session")
        {
            continue;
        }

        let Some(usage) = entry.get("usage").and_then(Value::as_object) else {
            continue;
        };

        let input_tokens = read_u64(usage, &["inputOther", "input_other"]);
        let output_tokens = read_u64(usage, &["output", "outputTokens", "output_tokens"]);
        let cache_read_tokens = read_u64(usage, &["inputCacheRead", "input_cache_read"]);
        let cache_creation_tokens =
            read_u64(usage, &["inputCacheCreation", "input_cache_creation"]);

        if input_tokens == 0
            && output_tokens == 0
            && cache_read_tokens == 0
            && cache_creation_tokens == 0
        {
            continue;
        }

        let Some(timestamp) = parse_kimi_timestamp(&entry) else {
            continue;
        };

        let model = normalize_kimi_model(entry.get("model").and_then(Value::as_str));

        entries.push(ParsedEntry {
            timestamp,
            model,
            input_tokens,
            output_tokens,
            // Kimi reports a single cache-creation bucket; the app models 5m/1h
            // tiers, so account all creation at the 5m tier (matching how the
            // pricing table prices Kimi cache writes).
            cache_creation_5m_tokens: cache_creation_tokens,
            cache_creation_1h_tokens: 0,
            cache_read_tokens,
            web_search_requests: 0,
            unique_hash: None,
            session_key: session_key.clone(),
            agent_scope: crate::stats::subagent::AgentScope::Main,
        });
    }

    entries.sort_by_key(|entry| entry.timestamp);

    if parse_failures > 0 && entries.is_empty() && lines_read > 10 {
        tracing::warn!(
            "All {} lines failed to parse in {}; Kimi wire schema may have changed",
            parse_failures,
            path.display()
        );
    }

    (entries, Vec::new(), lines_read, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_session(lines: &[&str]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wire.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        (dir, path)
    }

    #[test]
    fn parses_turn_scoped_usage_record() {
        let (_dir, path) = write_session(&[
            r#"{"type":"session.start","model":"kimi-code/kimi-for-coding"}"#,
            r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":1163,"output":352,"inputCacheRead":22272,"inputCacheCreation":10},"usageScope":"turn","time":1780410897480}"#,
        ]);
        let (entries, _changes, _lines, opened) = parse_kimi_session_file(&path);
        assert!(opened);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.model, "kimi-for-coding");
        assert_eq!(e.input_tokens, 1163);
        assert_eq!(e.output_tokens, 352);
        assert_eq!(e.cache_read_tokens, 22272);
        assert_eq!(e.cache_creation_5m_tokens, 10);
    }

    #[test]
    fn skips_session_scoped_cumulative_records() {
        let (_dir, path) = write_session(&[
            r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":100,"output":50,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":1780410897480}"#,
            r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":100,"output":50,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"session","time":1780410897999}"#,
        ]);
        let (entries, _changes, _lines, _opened) = parse_kimi_session_file(&path);
        assert_eq!(entries.len(), 1, "session-scoped record must be skipped");
        assert_eq!(entries[0].input_tokens, 100);
    }

    #[test]
    fn skips_zero_usage_records() {
        let (_dir, path) = write_session(&[
            r#"{"type":"usage.record","model":"kimi-for-coding","usage":{"inputOther":0,"output":0,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":1780410897480}"#,
        ]);
        let (entries, _changes, _lines, _opened) = parse_kimi_session_file(&path);
        assert!(entries.is_empty());
    }

    #[test]
    fn extracts_display_name_from_config_toml() {
        let config = r#"
default_model = "kimi-code/kimi-for-coding"

[thinking]
enabled = true

[providers."managed:kimi-code"]
type = "kimi"

[models."kimi-code/kimi-for-coding"]
provider = "managed:kimi-code"
model = "kimi-for-coding"
max_context_size = 262144
display_name = "K2.7 Code High Speed"
"#;
        let pairs = parse_kimi_model_display_names(config);
        assert_eq!(
            pairs,
            vec![(
                "kimi-for-coding".to_string(),
                "K2.7 Code High Speed".to_string()
            )]
        );
        // Keying is by normalized model key, matching what the parser emits.
        assert_eq!(
            crate::models::normalized_model_key("kimi-for-coding"),
            "kimi-for-coding"
        );
    }

    #[test]
    fn config_parser_ignores_non_model_tables() {
        let config = r#"
[services.moonshot_search]
model = "should-be-ignored"
display_name = "Nope"
"#;
        assert!(parse_kimi_model_display_names(config).is_empty());
    }

    #[test]
    fn accepts_legacy_snake_case_and_rfc3339() {
        let (_dir, path) = write_session(&[
            r#"{"type":"usage.record","model":"kimi-for-coding","usage":{"input_other":10,"output":5,"input_cache_read":3,"input_cache_creation":0},"timestamp":"2026-06-01T12:00:00Z"}"#,
        ]);
        let (entries, _changes, _lines, _opened) = parse_kimi_session_file(&path);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].input_tokens, 10);
        assert_eq!(entries[0].cache_read_tokens, 3);
    }
}
