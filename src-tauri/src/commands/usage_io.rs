// ─────────────────────────────────────────────────────────────────────────────
// Usage import / export — back up the usage archive to a portable JSON file and
// merge it back in with idempotent dedup. See docs/ecl/usage-import-export.yaml.
//
// Operates on the ArchivedHourly aggregate layer (the durable, provider-agnostic,
// cost-free record), NOT raw provider JSONL. Import deduplicates by bucket
// identity (source, date, hour, model_key, provider) with field-wise max, so
// re-importing the same file is a no-op.
// ─────────────────────────────────────────────────────────────────────────────

use super::AppState;
use crate::usage::archive::{ArchivedHourly, ImportSourceStats};
use chrono::Timelike;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

const EXPORT_FORMAT: &str = "tokenmonitor-usage-export";
const EXPORT_FORMAT_VERSION: u32 = 1;

/// One source's worth of archived records inside an export document.
#[derive(Serialize, Deserialize)]
struct SourceBlock {
    source_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frontier: Option<String>,
    records: Vec<ArchivedHourly>,
}

/// The on-disk export file. Snake_case keys are part of the file format.
#[derive(Serialize, Deserialize)]
struct ExportDocument {
    format: String,
    format_version: u32,
    #[serde(default)]
    exported_at: String,
    #[serde(default)]
    app_version: String,
    sources: Vec<SourceBlock>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub source_count: usize,
    pub record_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub sources: Vec<ImportSourceStats>,
    pub total_seen: usize,
    pub total_new: usize,
    pub total_deduped: usize,
}

/// Validate a source key before any filesystem op (prevents path traversal via
/// the archive's `source_dir` resolver). Only the known local providers and
/// safe device aliases are accepted.
fn is_valid_source_key(key: &str) -> bool {
    match key.split_once(':') {
        Some(("local", "claude")) | Some(("local", "codex")) | Some(("local", "cursor")) => true,
        Some(("device", alias)) => {
            !alias.is_empty()
                && alias.len() <= 64
                // Reject the special path components "." / ".." even though the
                // char filter would otherwise allow them (dot is permitted in
                // aliases like "laptop.local"), so source_dir().join(alias)
                // can never resolve to a parent directory.
                && alias != "."
                && alias != ".."
                && alias
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        }
        _ => false,
    }
}

/// Export every archived source to a single JSON file at `path`.
/// The frontend obtains `path` from a native Save dialog. Returns the written
/// path plus a count summary.
#[tauri::command]
pub async fn export_usage_data(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ExportResult, String> {
    let archive = state
        .parser
        .archive()
        .ok_or_else(|| "Usage archive is not available".to_string())?;

    // Flush completed local + SSH-device hours so the export reflects the latest
    // data. SSH devices are otherwise only archived on the background tick, so a
    // just-synced host could lag without this explicit flush.
    crate::archive_local_usage(&state);
    crate::archive_ssh_device_usage(&state).await;

    let sources: Vec<SourceBlock> = archive
        .list_sources()
        .into_iter()
        .map(|source_key| {
            let frontier = archive.frontier_string(&source_key);
            let records = archive.read_raw(&source_key);
            SourceBlock {
                source_key,
                frontier,
                records,
            }
        })
        .filter(|block| !block.records.is_empty())
        .collect();

    let source_count = sources.len();
    let record_count: usize = sources.iter().map(|b| b.records.len()).sum();

    let doc = ExportDocument {
        format: EXPORT_FORMAT.to_string(),
        format_version: EXPORT_FORMAT_VERSION,
        exported_at: chrono::Local::now().to_rfc3339(),
        app_version: app.package_info().version.to_string(),
        sources,
    };

    let json = serde_json::to_string_pretty(&doc)
        .map_err(|e| format!("Failed to serialize export: {e}"))?;

    let target = std::path::PathBuf::from(&path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create export dir: {e}"))?;
    }
    std::fs::write(&target, json.as_bytes()).map_err(|e| format!("Failed to write export: {e}"))?;

    Ok(ExportResult {
        path,
        source_count,
        record_count,
    })
}

/// Import a previously exported JSON document, merging each source into the
/// archive with idempotent dedup. The caller (frontend) reads the file contents
/// with the native file picker and passes the text here.
#[tauri::command]
pub async fn import_usage_data(
    app: AppHandle,
    state: State<'_, AppState>,
    json: String,
) -> Result<ImportResult, String> {
    let archive = state
        .parser
        .archive()
        .ok_or_else(|| "Usage archive is not available".to_string())?;

    let doc: ExportDocument =
        serde_json::from_str(&json).map_err(|e| format!("Not a valid TokenMonitor export: {e}"))?;
    if doc.format != EXPORT_FORMAT {
        return Err(format!(
            "Unrecognized file format '{}' (expected '{EXPORT_FORMAT}')",
            doc.format
        ));
    }
    if doc.format_version > EXPORT_FORMAT_VERSION {
        return Err(format!(
            "This file was created by a newer TokenMonitor (format {} > {EXPORT_FORMAT_VERSION}). Please update.",
            doc.format_version
        ));
    }

    // Flush local completed hours first so advancing a frontier on import never
    // hides un-archived live data (see ECL DEC-004).
    crate::archive_local_usage(&state);

    let now = chrono::Local::now();
    let current_date = now.date_naive();
    let current_hour = now.hour() as u8;

    let mut result = ImportResult {
        sources: Vec::new(),
        total_seen: 0,
        total_new: 0,
        total_deduped: 0,
    };

    for block in &doc.sources {
        if !is_valid_source_key(&block.source_key) {
            tracing::warn!(
                source = block.source_key.as_str(),
                "Skipping import block with invalid source key"
            );
            continue;
        }
        let stats = archive.import_source(
            &block.source_key,
            &block.records,
            current_date,
            current_hour,
        );
        result.total_seen += stats.seen;
        result.total_new += stats.new_buckets;
        result.total_deduped += stats.deduped;
        result.sources.push(stats);
    }

    // The archive changed but no source JSONL did, so invalidate caches manually
    // (mirrors the clear_payload_cache command) and notify the UI to refetch.
    // NOTE: never call parser.clear_cache() here — it resets the archive.
    state.parser.clear_payload_cache();
    if let Some(ref disk_cache) = *state.payload_disk_cache.read().await {
        disk_cache.clear_all();
    }
    let _ = app.emit("data-updated", 0u64);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_known_source_keys() {
        assert!(is_valid_source_key("local:claude"));
        assert!(is_valid_source_key("local:codex"));
        assert!(is_valid_source_key("local:cursor"));
        assert!(is_valid_source_key("device:my-server"));
        assert!(is_valid_source_key("device:laptop_2.local"));
    }

    #[test]
    fn rejects_unsafe_source_keys() {
        assert!(!is_valid_source_key("local:unknown"));
        assert!(!is_valid_source_key("local:../../etc"));
        assert!(!is_valid_source_key("device:../escape"));
        assert!(!is_valid_source_key("device:a/b"));
        assert!(!is_valid_source_key("device:a\\b"));
        assert!(!is_valid_source_key("device:.."));
        assert!(!is_valid_source_key("device:."));
        assert!(!is_valid_source_key("device:"));
        assert!(!is_valid_source_key("other:x"));
        assert!(!is_valid_source_key("garbage"));
    }
}
