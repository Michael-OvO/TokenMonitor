use chrono::{DateTime, Local, NaiveDate};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::parser::{
    glob_jsonl_files, modified_since, path_to_string, push_sample_path, ParsedEntry,
    ProviderReadDebug, SessionParseResult,
};

// ─────────────────────────────────────────────────────────────────────────────
// Kimi wire JSONL serde types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct KimiJsonlEntry {
    #[serde(rename = "type", default)]
    entry_type: String,
    #[serde(default)]
    time: Option<i64>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<KimiTokenUsage>,
    #[serde(default)]
    event: Option<KimiLoopEvent>,
}

#[derive(Deserialize)]
pub(crate) struct KimiLoopEvent {
    #[serde(rename = "type", default)]
    event_type: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<KimiTokenUsage>,
}

#[derive(Deserialize, Default)]
pub(crate) struct KimiTokenUsage {
    #[serde(alias = "inputOther", default)]
    input_other: Option<u64>,
    #[serde(alias = "inputCacheRead", default)]
    input_cache_read: Option<u64>,
    #[serde(default)]
    output: Option<u64>,
    #[serde(alias = "inputCacheCreation", default)]
    input_cache_creation: Option<u64>,
}

impl KimiTokenUsage {
    fn input_other_tokens(&self) -> u64 {
        self.input_other.unwrap_or(0)
    }

    fn output_tokens(&self) -> u64 {
        self.output.unwrap_or(0)
    }

    fn cache_read_tokens(&self) -> u64 {
        self.input_cache_read.unwrap_or(0)
    }

    fn cache_creation_tokens(&self) -> u64 {
        self.input_cache_creation.unwrap_or(0)
    }

    fn is_zero(&self) -> bool {
        self.input_other_tokens() == 0
            && self.output_tokens() == 0
            && self.cache_read_tokens() == 0
            && self.cache_creation_tokens() == 0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Kimi-specific helper functions
// ─────────────────────────────────────────────────────────────────────────────

fn extract_kimi_timestamp(entry: &KimiJsonlEntry) -> Option<DateTime<Local>> {
    entry
        .time
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|dt| dt.with_timezone(&Local))
}

fn extract_kimi_model(entry: &KimiJsonlEntry) -> Option<String> {
    if let Some(model) = entry.model.as_ref() {
        if !model.is_empty() {
            return Some(model.clone());
        }
    }
    if let Some(event) = entry.event.as_ref() {
        if let Some(model) = event.model.as_ref() {
            if !model.is_empty() {
                return Some(model.clone());
            }
        }
    }
    None
}

fn extract_kimi_usage(entry: &KimiJsonlEntry) -> Option<&KimiTokenUsage> {
    if let Some(usage) = entry.usage.as_ref() {
        return Some(usage);
    }
    entry.event.as_ref()?.usage.as_ref()
}

fn is_kimi_usage_event(entry: &KimiJsonlEntry) -> bool {
    if entry.entry_type == "usage.record" {
        return entry.usage.is_some();
    }
    if entry.entry_type == "context.append_loop_event" {
        if let Some(event) = entry.event.as_ref() {
            if event.event_type == "step.end" {
                return event.usage.is_some();
            }
        }
    }
    false
}

// ─────────────────────────────────────────────────────────────────────────────
// Kimi config.toml display-name lookup
// ─────────────────────────────────────────────────────────────────────────────

/// Locate `~/.kimi-code/config.toml` given a sessions directory.
///
/// The default layout is `~/.kimi-code/sessions/<workdir-hash>/<session>/...`,
/// so the config lives in the parent of the sessions dir. If the env override
/// `KIMI_DATA_DIR` points at a `sessions` folder inside an alternate data root,
/// this still finds the paired config.
fn kimi_config_path_from_sessions_dir(sessions_dir: &Path) -> Option<PathBuf> {
    sessions_dir.parent().map(|p| p.join("config.toml"))
}

/// Locate the paired `config.toml` by walking up from a `wire.jsonl` path.
///
/// A typical path is
/// `<root>/sessions/<workdir-hash>/<session-id>/agents/<agent>/wire.jsonl`,
/// so we search upward until we find `config.toml`.
fn kimi_config_path_from_wire_path(wire_path: &Path) -> Option<PathBuf> {
    wire_path.ancestors().find_map(|dir| {
        let candidate = dir.join("config.toml");
        candidate.exists().then_some(candidate)
    })
}

/// Load the `[models."<alias>"]` → `display_name` map from Kimi's config.toml.
fn load_kimi_display_names(config_path: &Path) -> HashMap<String, String> {
    let contents = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(path = %config_path.display(), error = %e, "failed to read Kimi config.toml");
            return HashMap::new();
        }
    };

    let config: toml::Value = match toml::from_str(&contents) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(path = %config_path.display(), error = %e, "failed to parse Kimi config.toml");
            return HashMap::new();
        }
    };

    let mut out = HashMap::new();
    let Some(models) = config.get("models").and_then(|m| m.as_table()) else {
        return out;
    };

    for (alias, model_table) in models {
        let Some(display) = model_table.get("display_name").and_then(|v| v.as_str()) else {
            continue;
        };
        if display.is_empty() {
            continue;
        }
        out.insert(alias.clone(), display.to_string());
    }

    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Kimi session file parser
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) fn parse_kimi_session_file(
    path: &Path,
    display_names: Option<&HashMap<String, String>>,
) -> SessionParseResult {
    // Resolve display names from the caller-provided map, or fall back to the
    // paired config.toml next to the wire.jsonl file.
    let owned_display_names;
    let display_names: &HashMap<String, String> = match display_names {
        Some(map) => map,
        None => {
            owned_display_names = kimi_config_path_from_wire_path(path)
                .map(|p| load_kimi_display_names(&p))
                .unwrap_or_default();
            &owned_display_names
        }
    };

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
    let mut lines_read = 0_usize;
    let mut parse_failures = 0_usize;
    let file_session_key = format!("kimi:file:{}", path_to_string(path));

    for line in reader.lines() {
        lines_read += 1;
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };

        let entry: KimiJsonlEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => {
                parse_failures += 1;
                continue;
            }
        };

        if !is_kimi_usage_event(&entry) {
            continue;
        }

        let token_usage = match extract_kimi_usage(&entry) {
            Some(usage) => usage,
            None => continue,
        };

        if token_usage.is_zero() {
            continue;
        }

        let ts = match extract_kimi_timestamp(&entry) {
            Some(ts) => ts,
            None => continue,
        };

        let model =
            extract_kimi_model(&entry).unwrap_or_else(|| String::from("kimi-code/kimi-for-coding"));
        let model_display_name = display_names.get(&model).cloned();

        entries.push(ParsedEntry {
            timestamp: ts,
            model,
            model_display_name,
            input_tokens: token_usage.input_other_tokens(),
            output_tokens: token_usage.output_tokens(),
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: token_usage.cache_creation_tokens(),
            cache_read_tokens: token_usage.cache_read_tokens(),
            web_search_requests: 0,
            unique_hash: None,
            session_key: file_session_key.clone(),
            agent_scope: crate::stats::subagent::AgentScope::Main,
        });
    }

    entries.sort_by_key(|a| a.timestamp);

    if parse_failures > 0 && entries.is_empty() && lines_read > 10 {
        tracing::warn!(
            "All {} candidate lines failed to parse in {}; JSONL schema may have changed",
            parse_failures,
            path.display()
        );
    }

    (entries, Vec::new(), lines_read, true)
}

// ─────────────────────────────────────────────────────────────────────────────
// Kimi directory reader
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) fn read_kimi_entries_with_debug(
    sessions_dir: &Path,
    since: Option<NaiveDate>,
) -> (Vec<ParsedEntry>, ProviderReadDebug) {
    let mut entries = Vec::new();
    let display_names = kimi_config_path_from_sessions_dir(sessions_dir)
        .map(|p| load_kimi_display_names(&p))
        .unwrap_or_default();
    let files = glob_jsonl_files(sessions_dir);
    let mut report = ProviderReadDebug {
        provider: String::from("kimi"),
        root_dir: path_to_string(sessions_dir),
        root_exists: sessions_dir.exists(),
        since: since.map(|date| date.format("%Y-%m-%d").to_string()),
        strategy: String::from("recursive-jsonl-glob+token-usage"),
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
        let (parsed_entries, _change_events, lines_read, opened) =
            parse_kimi_session_file(&path, Some(&display_names));
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
pub fn read_kimi_entries(sessions_dir: &Path, since: Option<NaiveDate>) -> Vec<ParsedEntry> {
    read_kimi_entries_with_debug(sessions_dir, since).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kimi_usage_record_line() {
        let line = r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usageScope":"turn","usage":{"inputOther":1200,"output":450,"inputCacheRead":100,"inputCacheCreation":50},"time":1783033533063}"#;
        let entry: KimiJsonlEntry = serde_json::from_str(line).unwrap();
        assert!(is_kimi_usage_event(&entry));
        let usage = extract_kimi_usage(&entry).unwrap();
        assert_eq!(usage.input_other_tokens(), 1200);
        assert_eq!(usage.output_tokens(), 450);
        assert_eq!(usage.cache_read_tokens(), 100);
        assert_eq!(usage.cache_creation_tokens(), 50);
    }

    #[test]
    fn parse_kimi_step_end_event_line() {
        let line = r#"{"type":"context.append_loop_event","event":{"type":"step.end","uuid":"u","turnId":"t","step":1,"usage":{"inputOther":100,"output":50},"finishReason":"stop"},"time":1783033533063}"#;
        let entry: KimiJsonlEntry = serde_json::from_str(line).unwrap();
        assert!(is_kimi_usage_event(&entry));
        let usage = extract_kimi_usage(&entry).unwrap();
        assert_eq!(usage.input_other_tokens(), 100);
        assert_eq!(usage.output_tokens(), 50);
    }

    #[test]
    fn parse_kimi_session_file_extracts_entries() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wire.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"usage.record","model":"kimi-code/kimi-for-coding","usageScope":"turn","usage":{{"inputOther":100,"output":50}},"time":1783033533063}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"usage.record","model":"kimi-code/kimi-for-coding","usageScope":"turn","usage":{{"inputOther":200,"output":100,"inputCacheRead":10}},"time":1783033534000}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"context.append_loop_event","event":{{"type":"tool.result","result":{{"output":"ok"}}}}}}"#
        )
        .unwrap();

        let display_names = HashMap::new();
        let (entries, _, lines_read, opened) = parse_kimi_session_file(&path, Some(&display_names));
        assert!(opened);
        assert_eq!(lines_read, 3);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].input_tokens, 100);
        assert_eq!(entries[0].output_tokens, 50);
        assert_eq!(entries[1].cache_read_tokens, 10);
    }

    #[test]
    fn parse_kimi_session_file_uses_config_display_name() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wire.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"usage.record","model":"kimi-code/kimi-for-coding","usageScope":"turn","usage":{{"inputOther":100,"output":50}},"time":1783033533063}}"#
        )
        .unwrap();

        let mut display_names = HashMap::new();
        display_names.insert(
            String::from("kimi-code/kimi-for-coding"),
            String::from("K2.7 Code"),
        );

        let (entries, _, _, opened) = parse_kimi_session_file(&path, Some(&display_names));
        assert!(opened);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].model, "kimi-code/kimi-for-coding");
        assert_eq!(entries[0].model_display_name.as_deref(), Some("K2.7 Code"));
    }

    #[test]
    fn load_kimi_display_names_reads_config_toml() {
        let dir = tempfile::tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions");
        fs::create_dir(&sessions_dir).unwrap();
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
[models."kimi-code/kimi-for-coding"]
provider = "managed:kimi-code"
model = "kimi-for-coding"
display_name = "K2.7 Code"
"#,
        )
        .unwrap();

        let config_path = kimi_config_path_from_sessions_dir(&sessions_dir).unwrap();
        let names = load_kimi_display_names(&config_path);
        assert_eq!(
            names.get("kimi-code/kimi-for-coding"),
            Some(&String::from("K2.7 Code"))
        );
    }
}
