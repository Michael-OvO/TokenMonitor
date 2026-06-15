// ─────────────────────────────────────────────────────────────────────────────
// Usage import / export — back up the usage archive and merge it back in with
// idempotent dedup. See docs/ecl/usage-import-export.yaml.
//
// Operates on the ArchivedHourly aggregate layer (the durable, provider-agnostic
// record). The ARCHIVE dedups by bucket identity (source, d, h, mk, p) with
// field-wise max, so re-importing the same data is a no-op.
//
// Export shape (both formats below) is human-report-oriented: it drops the raw
// archive `p` tag and instead carries a resolved `provider` (always one of
// claude/codex/cursor, never "all") plus a computed USD `cost`. On IMPORT the
// bucket `p` is reconstructed (legacy record.p > new record.provider > JSONL
// line provider > "all") and `cost` is ignored (the archive recomputes cost with
// live pricing). Old files that still carry `p` import unchanged.
//
// Two on-disk shapes:
//   • Manual Export button → ONE pretty-printed JSON document ("snapshot").
//   • Background auto-export → a line-delimited JSONL log APPENDED to on the
//     refresh cadence (completed hours are immutable and only grow).
// Import transparently reads BOTH shapes (and their pre-`provider`/`cost`
// predecessors).
// ─────────────────────────────────────────────────────────────────────────────

use super::AppState;
use crate::usage::archive::{ArchiveFrontier, ArchiveManager, ArchivedHourly, ImportSourceStats};
use chrono::{NaiveDate, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, State};

const EXPORT_FORMAT: &str = "tokenmonitor-usage-export";
const EXPORT_FORMAT_VERSION: u32 = 1;

/// JSONL auto-export format tag + version (independent of the snapshot format).
const EXPORT_FORMAT_JSONL: &str = "tokenmonitor-usage-export-jsonl";
const EXPORT_FORMAT_JSONL_VERSION: u32 = 1;

/// The background auto-export writes ONE file PER DEVICE:
/// `TokenMonitor-Usage-<deviceSlug>.jsonl`. A single writer per file means a
/// cloud-sync folder (OneDrive/Dropbox/…) never produces a "conflicted copy" —
/// the classic failure mode of many machines writing one shared file. Peers'
/// files (other devices) are read but never written by this machine.
const AUTO_EXPORT_FILE_PREFIX: &str = "TokenMonitor-Usage-";
const AUTO_EXPORT_FILE_SUFFIX: &str = ".jsonl";

/// This machine's auto-export file name, `TokenMonitor-Usage-<slug>.jsonl`.
fn auto_export_file_name() -> String {
    format!("{AUTO_EXPORT_FILE_PREFIX}{}{AUTO_EXPORT_FILE_SUFFIX}", device_slug())
}

/// Defensive per-line cap when importing JSONL. A record line is ~150 bytes;
/// anything past this is treated as corrupt and skipped rather than allocated,
/// bounding memory on a malformed/hostile file.
const MAX_JSONL_LINE_BYTES: usize = 1_048_576;

/// Background auto-export preferences, mirrored from the frontend settings via
/// `set_auto_export_config`. When `enabled` and a `folder` is set, the refresh
/// loop appends new archive records into that folder's JSONL file.
#[derive(Clone, Default)]
pub struct AutoExportConfig {
    pub enabled: bool,
    pub folder: Option<String>,
}

/// Runtime (non-persisted) state for the JSONL auto-export. Defaults on launch
/// so the first enabled tick performs a full reconciliation ("rewrite once");
/// thereafter only new completed-hour records are appended.
#[derive(Default)]
pub struct AutoExportRuntime {
    /// `false` until the once-per-session full reconciliation has written the
    /// complete archive to the current file. Reset (together with `cursors`) on
    /// folder change and after an import so a fresh full rewrite lands.
    pub synced: bool,
    /// Per-source high-water frontier already written to the file. Drives both
    /// the cheap "nothing changed" skip and the incremental append window.
    pub cursors: HashMap<String, ArchiveFrontier>,
    /// Last-seen mtime of each PEER file (other devices') we've merged from the
    /// sync folder, keyed by file name. Gates re-merging so an unchanged peer
    /// file isn't re-parsed/re-imported every tick.
    pub peer_mtimes: HashMap<String, Option<SystemTime>>,
}

// ── Export wire types (serialize-only) ───────────────────────────────────────

/// One archived bucket as written to an export file: the raw `p` tag is dropped
/// in favor of a resolved `provider` and a computed USD `cost`. `provider` is
/// omitted on JSONL record lines (the line itself carries it) and present on
/// snapshot records (which have no per-line wrapper).
#[derive(Serialize)]
struct ExportRecordRef<'a> {
    d: &'a str,
    h: u8,
    mk: &'a str,
    mn: &'a str,
    #[serde(rename = "in")]
    input_tokens: u64,
    out: u64,
    c5: u64,
    c1: u64,
    cr: u64,
    ws: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<&'a str>,
    cost: f64,
}

/// JSONL header (line 1 of the auto-export file).
#[derive(Serialize)]
struct JsonlHeader<'a> {
    format: &'a str,
    format_version: u32,
    exported_at: String,
    app_version: String,
    /// The machine that produced this file: computer name + OS, e.g.
    /// "Zijia's MacBook Pro (macOS)". Each record line also carries its own
    /// `device` (a remote SSH device's lines carry that device's alias).
    device: &'a str,
}

/// One JSONL record line. `device` is which machine the row came from (this
/// machine for `local:*`, the SSH alias for `device:*`); `provider` is the
/// resolved source tool (claude/codex/cursor, never "all").
#[derive(Serialize)]
struct JsonlLineRef<'a> {
    source_key: &'a str,
    device: &'a str,
    provider: &'a str,
    record: ExportRecordRef<'a>,
}

/// One source's records inside a snapshot document.
#[derive(Serialize)]
struct ExportSourceRef<'a> {
    source_key: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    frontier: Option<&'a str>,
    records: Vec<ExportRecordRef<'a>>,
}

/// The single-document ("snapshot") export file.
#[derive(Serialize)]
struct ExportDocRef<'a> {
    format: &'a str,
    format_version: u32,
    exported_at: String,
    app_version: String,
    /// The machine that produced this file (origin marker).
    device: &'a str,
    sources: Vec<ExportSourceRef<'a>>,
}

// ── Import wire types (deserialize-only, tolerant of old + new shapes) ────────

/// Lenient probe used to detect the JSONL format / version from a single line.
#[derive(Deserialize)]
struct JsonlHeaderProbe {
    #[serde(default)]
    format: String,
    #[serde(default)]
    format_version: u32,
    /// Origin machine label from the header line. Used to recognize a file this
    /// machine wrote under a PREVIOUS file name (after a naming-scheme change) so
    /// it isn't merged back in as a phantom "peer device".
    #[serde(default)]
    device: String,
}

/// A record read from either export shape. Accepts both the legacy `p` tag and
/// the new `provider` field (and ignores `cost`), so old and new files import.
#[derive(Deserialize)]
struct ImportRecord {
    d: String,
    #[serde(default)]
    h: u8,
    #[serde(default)]
    mk: String,
    #[serde(default)]
    mn: String,
    #[serde(rename = "in", default)]
    input_tokens: u64,
    #[serde(default)]
    out: u64,
    #[serde(default)]
    c5: u64,
    #[serde(default)]
    c1: u64,
    #[serde(default)]
    cr: u64,
    #[serde(default)]
    ws: u64,
    /// Legacy bucket tag (claude/codex/cursor/all). Present in old exports.
    #[serde(default)]
    p: Option<String>,
    /// New resolved provider (snapshot record-level). Present in new snapshots.
    #[serde(default)]
    provider: Option<String>,
}

impl ImportRecord {
    /// Reconstruct the archive bucket, resolving `p` from (in precedence order):
    /// the legacy `record.p`, the new `record.provider`, the JSONL line-level
    /// provider, then "all" as a last resort. This keeps old files byte-faithful
    /// while letting new files (no `p`) round-trip via their resolved provider.
    fn into_archived(self, line_provider: Option<&str>) -> ArchivedHourly {
        let p = self
            .p
            .or(self.provider)
            .or_else(|| line_provider.map(str::to_string))
            .unwrap_or_else(|| "all".to_string());
        ArchivedHourly {
            d: self.d,
            h: self.h,
            mk: self.mk,
            mn: self.mn,
            input_tokens: self.input_tokens,
            out: self.out,
            c5: self.c5,
            c1: self.c1,
            cr: self.cr,
            ws: self.ws,
            p,
        }
    }
}

/// One source's records inside a snapshot document, on import.
#[derive(Deserialize)]
struct ImportSourceBlock {
    source_key: String,
    #[serde(default)]
    records: Vec<ImportRecord>,
}

/// The single-document snapshot, on import.
#[derive(Deserialize)]
struct ImportDoc {
    format: String,
    #[serde(default)]
    format_version: u32,
    #[serde(default)]
    sources: Vec<ImportSourceBlock>,
}

/// One JSONL record line, on import. `device` (if present) is ignored; the
/// line-level `provider` feeds bucket reconstruction when the record lacks `p`.
#[derive(Deserialize)]
struct ImportJsonlLine {
    source_key: String,
    #[serde(default)]
    provider: Option<String>,
    record: ImportRecord,
}

/// `(source_key, reconstructed records)` groups produced by parsing an import
/// payload, ready to feed the shared `import_source` merge loop.
type ImportGroups = Vec<(String, Vec<ArchivedHourly>)>;

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
    /// Malformed/oversized lines skipped while parsing a JSONL file (e.g. a torn
    /// final append). Surfaced so a partial import is never silently mistaken
    /// for a complete one.
    pub skipped: usize,
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

// ── Cost + provider/device derivation (export side) ──────────────────────────

/// USD cost for one bucket, matching the query path's formula
/// (`calculate_cost_for_key * provider_multiplier`). The archive stores the
/// normalized model key (no Bedrock region prefix), so the multiplier is 1.0 in
/// practice. Rounded to micro-dollars to avoid float noise; 0.0 when pricing for
/// the model is unknown (same as the dashboard).
fn bucket_cost_usd(r: &ArchivedHourly) -> f64 {
    use crate::usage::pricing::{calculate_cost_for_key, provider_multiplier};
    let raw = calculate_cost_for_key(
        &r.mk,
        r.input_tokens,
        r.out,
        r.c5,
        r.c1,
        r.cr,
        r.ws,
    ) * provider_multiplier(&r.mk);
    (raw * 1_000_000.0).round() / 1_000_000.0
}

/// Build the export view of one record (drops `p`, adds `cost`). `provider` is
/// `Some` for snapshot records (no line wrapper) and `None` for JSONL record
/// lines (the line carries it).
fn export_record<'a>(r: &'a ArchivedHourly, provider: Option<&'a str>) -> ExportRecordRef<'a> {
    ExportRecordRef {
        d: &r.d,
        h: r.h,
        mk: &r.mk,
        mn: &r.mn,
        input_tokens: r.input_tokens,
        out: r.out,
        c5: r.c5,
        c1: r.c1,
        cr: r.cr,
        ws: r.ws,
        provider,
        cost: bucket_cost_usd(r),
    }
}

/// The source tool for a record — always one of "claude" / "codex" / "cursor",
/// never "all". Local rows carry the authoritative integration tag in `p` (and
/// Cursor proxies many model families, so its `p` must win over the model).
/// Aggregated SSH-device rows store `p = "all"`, so for those the tool is
/// recovered from the model family (devices only ever sync Claude Code / Codex
/// data); a third-party/unknown model falls back to "claude".
fn provider_label(record: &ArchivedHourly) -> &str {
    match record.p.as_str() {
        p @ ("claude" | "codex" | "cursor") => p,
        _ => {
            use crate::models::{detect_model_family, ModelFamily};
            match detect_model_family(&record.mk) {
                ModelFamily::OpenAI => "codex",
                ModelFamily::Cursor => "cursor",
                _ => "claude",
            }
        }
    }
}

/// Resolve the device label for a source: a `device:<alias>` source reports its
/// alias (the remote machine); everything else (`local:*`) reports this machine.
fn source_device(source_key: &str, local_device: &str) -> String {
    match source_key.split_once(':') {
        Some(("device", alias)) => alias.to_string(),
        _ => local_device.to_string(),
    }
}

/// This machine's label: "<computer name> (<OS>)", e.g. "Zijia's MacBook Pro
/// (macOS)". Computed once and cached — the value is stable for the process.
fn device_label() -> &'static str {
    static LABEL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LABEL
        .get_or_init(|| format!("{} ({})", computer_name(), os_label()))
        .as_str()
}

/// This machine's filename-safe slug, e.g. "Zijia-s-MacBook-Pro-macOS". Used in
/// the per-device file name AND, when a peer reads this file, as the `device:`
/// source alias — so it must satisfy `is_valid_source_key`'s device rules
/// (`[A-Za-z0-9._-]`, ≤64, not "."/".."). Cached for the process.
fn device_slug() -> &'static str {
    static SLUG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SLUG.get_or_init(|| {
        // Human-readable part (capped) + a stable per-machine disambiguator hash.
        // The hash (over the FULL label + home dir) guarantees two machines never
        // share a file name / device alias even if their names truncate to the
        // same 64-char prefix or are identical — which would otherwise reintroduce
        // the cloud-sync conflicted-copy problem. DefaultHasher uses fixed keys, so
        // the value is stable for a given machine across runs.
        let base: String = slugify(device_label()).chars().take(48).collect();
        let base = base.trim_matches('-');
        let seed = format!(
            "{}|{}",
            device_label(),
            dirs::home_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&seed, &mut hasher);
        let id = std::hash::Hasher::finish(&hasher) as u32;
        if base.is_empty() {
            format!("device-{id:08x}")
        } else {
            format!("{base}-{id:08x}")
        }
    })
    .as_str()
}

/// Collapse arbitrary text into a `[A-Za-z0-9._-]` slug (runs of other chars
/// become a single '-', trimmed, capped at 64). Never empty / "." / "..".
fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let mut slug: String = out.trim_matches('-').chars().take(64).collect();
    if slug.is_empty() || slug == "." || slug == ".." {
        slug = "device".to_string();
    }
    slug
}

/// Friendly OS name.
fn os_label() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

/// Best-effort computer name, no extra dependency. macOS uses the user-facing
/// ComputerName; Windows reads COMPUTERNAME; other Unix shells out to `hostname`.
/// Falls back through env vars to "unknown".
fn computer_name() -> String {
    #[cfg(target_os = "windows")]
    if let Ok(n) = std::env::var("COMPUTERNAME") {
        let n = n.trim();
        if !n.is_empty() {
            return n.to_string();
        }
    }

    #[cfg(target_os = "macos")]
    if let Some(n) = run_trimmed("scutil", &["--get", "ComputerName"]) {
        return n;
    }

    #[cfg(unix)]
    if let Some(n) = run_trimmed("hostname", &[]) {
        // Trim a trailing domain (e.g. "host.local" → "host") for readability.
        let short = n.split('.').next().unwrap_or(&n).trim();
        if !short.is_empty() {
            return short.to_string();
        }
    }

    for var in ["HOSTNAME", "COMPUTERNAME", "HOST"] {
        if let Ok(n) = std::env::var(var) {
            let n = n.trim();
            if !n.is_empty() {
                return n.to_string();
            }
        }
    }
    "unknown".to_string()
}

/// Run a command and return its trimmed stdout, or None on any failure.
#[cfg(unix)]
fn run_trimmed(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// ── Manual snapshot export (JSON document) ───────────────────────────────────

/// Export every archived source to a single JSON snapshot at `path`.
/// The frontend obtains `path` from a native Save dialog. Returns the written
/// path plus a count summary.
#[tauri::command]
pub async fn export_usage_data(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ExportResult, String> {
    run_export(&app, &state, &path).await
}

/// Core snapshot routine for the manual Export button. Flushes completed local +
/// SSH-device hours, then writes every archived source to one pretty JSON file.
pub(crate) async fn run_export(
    app: &AppHandle,
    state: &AppState,
    path: &str,
) -> Result<ExportResult, String> {
    let archive = state
        .parser
        .archive()
        .ok_or_else(|| "Usage archive is not available".to_string())?;

    // Flush completed local + SSH-device hours so the export reflects the latest
    // data. SSH devices are otherwise only archived on the background tick, so a
    // just-synced host could lag without this explicit flush.
    crate::archive_local_usage(state);
    crate::archive_ssh_device_usage(state).await;

    // Collect owned records first so the borrowed export views can reference them.
    let collected: Vec<(String, Option<String>, Vec<ArchivedHourly>)> = archive
        .list_sources()
        .into_iter()
        .map(|source_key| {
            let frontier = archive.frontier_string(&source_key);
            let records = archive.read_raw(&source_key);
            (source_key, frontier, records)
        })
        .filter(|(_, _, records)| !records.is_empty())
        .collect();

    let source_count = collected.len();
    let record_count: usize = collected.iter().map(|(_, _, r)| r.len()).sum();

    let sources: Vec<ExportSourceRef> = collected
        .iter()
        .map(|(source_key, frontier, records)| ExportSourceRef {
            source_key,
            frontier: frontier.as_deref(),
            records: records
                .iter()
                .map(|r| export_record(r, Some(provider_label(r))))
                .collect(),
        })
        .collect();

    let doc = ExportDocRef {
        format: EXPORT_FORMAT,
        format_version: EXPORT_FORMAT_VERSION,
        exported_at: chrono::Local::now().to_rfc3339(),
        app_version: app.package_info().version.to_string(),
        device: device_label(),
        sources,
    };

    let json = serde_json::to_string_pretty(&doc)
        .map_err(|e| format!("Failed to serialize export: {e}"))?;

    let target = std::path::PathBuf::from(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create export dir: {e}"))?;
    }
    std::fs::write(&target, json.as_bytes()).map_err(|e| format!("Failed to write export: {e}"))?;

    Ok(ExportResult {
        path: path.to_string(),
        source_count,
        record_count,
    })
}

// ── Background JSONL auto-export ─────────────────────────────────────────────

/// Run one auto-export pass if it's enabled and a destination folder is set.
/// Called from the refresh loop on every tick (and once synchronously after an
/// import). Best-effort: failures are logged and swallowed.
///
/// Concurrency: the heavy archive flush (with its `.await`) runs BEFORE the
/// runtime lock is taken, so the lock is never held across an await. All fs work
/// in `do_auto_export` is synchronous; holding the lock across it both serializes
/// file access against import's one-shot call and keeps `synced`/`cursors`
/// consistent. The cheap frontier pre-check returns before any read_raw/write on
/// the common (nothing-changed) tick, so command handlers contending for the
/// lock normally wait only microseconds.
pub(crate) async fn run_auto_export(app: &AppHandle, state: &AppState) {
    let (enabled, folder) = {
        let cfg = state.auto_export.read().await;
        (cfg.enabled, cfg.folder.clone())
    };
    if !enabled {
        return;
    }
    let Some(folder) = folder else {
        return;
    };
    let Some(archive) = state.parser.archive() else {
        return;
    };

    // Self-sufficient flush (mirrors run_export) so frontiers/records are current
    // even when periodic refresh is Off and we're driven from the idle branch.
    crate::archive_local_usage(state);
    crate::archive_ssh_device_usage(state).await;

    let folder_path = Path::new(&folder);
    let own_file = auto_export_file_name();
    // This machine's label (computer name + OS), computed before the lock. Cached
    // in a OnceLock so the underlying lookup runs at most once per process.
    let local_device = device_label();

    // Devices THIS machine owns besides local:* = ALL its configured SSH hosts,
    // ENABLED OR NOT. The export file is a backup of everything this machine
    // knows, so a host we've synced before is still ours to back up even after
    // it's disabled ("disabled = hidden in the UI", not "dropped from backup").
    // What this intentionally excludes is a file-imported PEER device (present in
    // the archive but never in our SSH config) — that belongs in the peer's own
    // file, not ours.
    let ssh_aliases: HashSet<String> = {
        let hosts = state.ssh_hosts.read().await;
        hosts.iter().map(|h| h.alias.clone()).collect()
    };

    // ── 1. Pull in peers: merge every OTHER device's file from this folder ──
    let merged = {
        let mut rt = state.auto_export_runtime.write().await;
        merge_peer_files(&archive, folder_path, &own_file, &mut rt)
    };
    if merged {
        // Peer data landed in the archive — refresh caches + notify the UI,
        // mirroring import_usage_data so the combined view appears live.
        state.parser.clear_payload_cache();
        if let Some(ref disk_cache) = *state.payload_disk_cache.read().await {
            disk_cache.clear_all();
        }
        let _ = app.emit("data-updated", 0u64);
    }

    // ── 2. Write THIS machine's own file (owned sources only) ──
    let owned: Vec<String> = archive
        .list_sources()
        .into_iter()
        .filter(|s| is_own_source(s, &ssh_aliases))
        .collect();
    let path = folder_path.join(&own_file);
    let mut rt = state.auto_export_runtime.write().await;
    if let Err(e) = do_auto_export(&archive, &path, app, local_device, &owned, &mut rt) {
        tracing::warn!(error = %e, folder = folder.as_str(), "Auto-export failed");
    }
}

/// True if a source belongs to THIS machine (so it's ours to export): the local
/// providers, or a device we sync via SSH. File-imported peer devices (present
/// in the archive but not one of our SSH hosts) are NOT re-exported — otherwise
/// our file would accumulate every peer's data and the files would cross-pollute.
fn is_own_source(source_key: &str, ssh_aliases: &HashSet<String>) -> bool {
    match source_key.split_once(':') {
        Some(("local", _)) => true,
        Some(("device", alias)) => ssh_aliases.contains(alias),
        _ => false,
    }
}

/// Remap a peer file's source into THIS archive: a peer's own `local:*` becomes
/// a `device:<peerSlug>` source (so it stays attributed to that machine and its
/// totals sum rather than colliding with our local); a peer's `device:*` (a
/// machine IT syncs) is kept as-is and merges with ours if shared. Returns None
/// for anything unrecognized.
fn remap_peer_source(source_key: &str, peer_slug: &str) -> Option<String> {
    match source_key.split_once(':') {
        Some(("local", _)) => Some(format!("device:{peer_slug}")),
        Some(("device", _)) => Some(source_key.to_string()),
        _ => None,
    }
}

/// Scan the sync folder for OTHER devices' export files and merge them into the
/// archive (idempotent field-wise-max dedup). Skips our own file and any file
/// whose mtime is unchanged since the last merge. Returns true if anything was
/// imported (so the caller can refresh caches + notify the UI).
fn merge_peer_files(
    archive: &ArchiveManager,
    folder: &Path,
    own_file: &str,
    rt: &mut AutoExportRuntime,
) -> bool {
    let read_dir = match std::fs::read_dir(folder) {
        Ok(r) => r,
        Err(_) => return false,
    };

    let now = chrono::Local::now();
    let current_date = now.date_naive();
    let current_hour = now.hour() as u8;

    let mut changed = false;
    let mut seen_files: HashSet<String> = HashSet::new();

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == own_file {
            continue;
        }
        // Must match our per-device naming and yield a safe device alias.
        let Some(slug) = name
            .strip_prefix(AUTO_EXPORT_FILE_PREFIX)
            .and_then(|s| s.strip_suffix(AUTO_EXPORT_FILE_SUFFIX))
        else {
            continue;
        };
        if !is_valid_source_key(&format!("device:{slug}")) {
            continue;
        }
        seen_files.insert(name.clone());

        // mtime gate: skip a peer file we've already merged at this version. When
        // the filesystem can't report an mtime (None), never gate on it — always
        // re-merge (idempotent dedup makes that safe) so changes aren't missed.
        let mtime = entry.metadata().ok().and_then(|m| m.modified().ok());
        if mtime.is_some() && rt.peer_mtimes.get(&name) == Some(&mtime) {
            continue;
        }

        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(file = name.as_str(), error = %e, "Auto-export peer read failed");
                continue;
            }
        };

        // Skip a file THIS machine wrote under a previous name (the filename
        // scheme changed across versions). Identify it by the header's `device`
        // label == ours; merging it would re-import our own data as a phantom
        // peer device and double-count it. Gate it (record mtime) so we don't
        // re-read it every tick.
        let header_device = content
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .and_then(|first| serde_json::from_str::<JsonlHeaderProbe>(first).ok())
            .map(|h| h.device)
            .unwrap_or_default();
        if !header_device.is_empty() && header_device == device_label() {
            tracing::debug!(file = name.as_str(), "Skipping our own stale export file");
            rt.peer_mtimes.insert(name, mtime);
            continue;
        }

        let (groups, _skipped) = match parse_import_payload(&content) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(file = name.as_str(), error = %e, "Auto-export peer parse failed");
                continue;
            }
        };

        for (source_key, records) in groups {
            let Some(target) = remap_peer_source(&source_key, slug) else {
                continue;
            };
            // remap_peer_source only yields device:* keys; re-validate defensively.
            if !is_valid_source_key(&target) {
                continue;
            }
            if records.is_empty() {
                continue;
            }
            archive.import_source(&target, &records, current_date, current_hour);
            changed = true;
        }

        rt.peer_mtimes.insert(name, mtime);
    }

    // Forget peers whose files disappeared, so a returning file re-merges.
    rt.peer_mtimes.retain(|k, _| seen_files.contains(k));

    changed
}

/// Synchronous core of the auto-export. Either does a full reconciliation (first
/// run this session, after an import/folder-change, or a missing file) or appends
/// only records past each source's cursor.
fn do_auto_export(
    archive: &ArchiveManager,
    path: &Path,
    app: &AppHandle,
    local_device: &str,
    owned: &[String],
    rt: &mut AutoExportRuntime,
) -> Result<(), String> {
    // Once-per-session (or post-import / folder-change / missing-file) full
    // reconciliation guarantees the file holds everything archived so far.
    if !rt.synced || !path.exists() {
        let cursors = full_rewrite(archive, path, app, local_device, owned)?;
        rt.cursors = cursors;
        rt.synced = true;
        tracing::debug!(path = %path.display(), "Auto-export full sync complete");
        return Ok(());
    }

    // Incremental: append only completed-hour records past each source's cursor.
    // Iterate OWNED sources only (never re-export a file-imported peer device).
    let mut batch = String::new();
    let mut count = 0usize;
    let mut updates: Vec<(String, ArchiveFrontier)> = Vec::new();
    for source_key in owned {
        let source_key = source_key.as_str();
        let cur = match archive.frontier(source_key) {
            Some(f) => f,
            None => continue, // no completed hours archived yet
        };
        let prior = rt.cursors.get(source_key).copied();
        if prior == Some(cur) {
            continue; // frontier unchanged → nothing new for this source
        }
        let device = source_device(source_key, local_device);
        for record in archive.read_raw(source_key) {
            let Ok(date) = NaiveDate::parse_from_str(&record.d, "%Y-%m-%d") else {
                // A well-formed archive never hits this; a bad date means the
                // record is unaddressable (no (date,hour) key), so log it as a
                // data-integrity signal instead of dropping it silently.
                tracing::warn!(
                    source = source_key,
                    date = record.d.as_str(),
                    "Auto-export skipping archive record with unparseable date"
                );
                continue;
            };
            let is_new = prior.is_none_or(|f| !f.covers(date, record.h));
            if !is_new {
                continue;
            }
            batch.push_str(&record_line(source_key, &device, &record)?);
            batch.push('\n');
            count += 1;
        }
        updates.push((source_key.to_string(), cur));
    }

    if !batch.is_empty() {
        if let Err(e) = append_lines(path, &batch) {
            // Leave synced=false so the next tick rewrites cleanly rather than
            // appending onto a possibly-partial file; don't advance cursors.
            rt.synced = false;
            return Err(e);
        }
        tracing::debug!(records = count, "Auto-export appended new records");
    }
    // Advance cursors after a successful (or empty) append. Cursors track each
    // source's completed-hour FRONTIER, not individual record writes, so an
    // empty batch on an advanced frontier (a genuinely empty completed hour) is
    // correct to commit — nothing was lost. Anchoring on the frontier (rather
    // than the max written record) avoids re-emitting/duplicating records when
    // the newest completed hour happens to be empty.
    for (k, v) in updates {
        rt.cursors.insert(k, v);
    }
    // Drop cursors for sources no longer owned (e.g. an SSH host was disabled),
    // keeping the cursor map aligned with what we actually export.
    rt.cursors.retain(|k, _| owned.iter().any(|s| s == k));
    Ok(())
}

/// Serialize one record as a JSONL line body (no trailing newline). `device` is
/// the originating machine; `provider` is resolved by `provider_label`; the
/// record body drops `p` and carries the computed USD `cost`.
fn record_line(source_key: &str, device: &str, record: &ArchivedHourly) -> Result<String, String> {
    serde_json::to_string(&JsonlLineRef {
        source_key,
        device,
        provider: provider_label(record),
        record: export_record(record, None),
    })
    .map_err(|e| format!("Failed to serialize record: {e}"))
}

/// Rewrite the entire JSONL file from scratch (header + every archived record)
/// and return the per-source cursors it represents. Published atomically via a
/// sibling temp file + rename so a crash mid-write never leaves a torn mirror —
/// the previous good file survives until the rename succeeds.
fn full_rewrite(
    archive: &ArchiveManager,
    path: &Path,
    app: &AppHandle,
    local_device: &str,
    owned: &[String],
) -> Result<HashMap<String, ArchiveFrontier>, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create auto-export dir: {e}"))?;
    }

    let mut out = String::new();
    let header = JsonlHeader {
        format: EXPORT_FORMAT_JSONL,
        format_version: EXPORT_FORMAT_JSONL_VERSION,
        exported_at: chrono::Local::now().to_rfc3339(),
        app_version: app.package_info().version.to_string(),
        device: local_device,
    };
    out.push_str(&serde_json::to_string(&header).map_err(|e| format!("Failed header: {e}"))?);
    out.push('\n');

    let mut cursors = HashMap::new();
    for source_key in owned {
        let source_key = source_key.as_str();
        let records = archive.read_raw(source_key);
        if records.is_empty() {
            continue;
        }
        let device = source_device(source_key, local_device);
        for record in &records {
            out.push_str(&record_line(source_key, &device, record)?);
            out.push('\n');
        }
        if let Some(f) = archive.frontier(source_key) {
            cursors.insert(source_key.to_string(), f);
        }
    }

    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, out.as_bytes())
        .map_err(|e| format!("Failed to write auto-export: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Failed to publish auto-export: {e}"))?;
    Ok(cursors)
}

/// Append a batch of complete (newline-terminated) lines, guarding against a
/// torn previous write: if the file doesn't already end in '\n', emit a
/// separating newline first so a partial trailing line becomes its own
/// (import-skippable) line instead of fusing with the first new record.
fn append_lines(path: &Path, batch: &str) -> Result<(), String> {
    // Best-effort: does the file already end in '\n'? Swallow any IO error here
    // (e.g. a concurrent truncation by a cloud-sync daemon between stat and
    // seek) and just skip the separator rather than failing the whole append —
    // a torn tail is recovered by the launch-time full rewrite and tolerated by
    // the importer. Only a genuine write failure below should abort the tick.
    let needs_sep = last_byte(path).is_some_and(|b| b != b'\n');
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("Failed to open auto-export for append: {e}"))?;
    if needs_sep {
        out.write_all(b"\n")
            .map_err(|e| format!("Failed to append separator: {e}"))?;
    }
    out.write_all(batch.as_bytes())
        .map_err(|e| format!("Failed to append auto-export: {e}"))?;
    Ok(())
}

/// Last byte of `path`, or None if the file is empty, missing, or unreadable.
/// Intentionally infallible (errors → None) so a concurrent truncation can't
/// turn the trailing-newline probe into a hard append failure.
fn last_byte(path: &Path) -> Option<u8> {
    let mut f = std::fs::File::open(path).ok()?;
    if f.metadata().ok()?.len() == 0 {
        return None;
    }
    f.seek(SeekFrom::End(-1)).ok()?;
    let mut b = [0u8; 1];
    f.read_exact(&mut b).ok()?;
    Some(b[0])
}

// ── Import (reads both the JSON snapshot and JSONL log formats) ──────────────

/// Import a previously exported document, merging each source into the archive
/// with idempotent dedup. Reads BOTH the single-document JSON snapshot and the
/// line-delimited JSONL auto-export log (including their pre-`provider`/`cost`
/// predecessors). The caller (frontend) reads the file contents with the native
/// file picker and passes the text here.
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

    let (sources, skipped) = parse_import_payload(&json)?;

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
        skipped,
    };

    for (source_key, records) in &sources {
        if !is_valid_source_key(source_key) {
            tracing::warn!(
                source = source_key.as_str(),
                "Skipping import block with invalid source key"
            );
            continue;
        }
        let stats = archive.import_source(source_key, records, current_date, current_hour);
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

    // Imports can field-wise-max-bump OLD, already-exported buckets that sit
    // behind the auto-export cursor, so the incremental path would miss them.
    // Force a full reconciliation AND run it now (not just on the next tick),
    // so the mirror reflects the merge immediately regardless of the refresh
    // cadence — including when refresh is Off. This reset is the LAST state
    // mutation before the export, so the export observes the merged archive.
    {
        let mut rt = state.auto_export_runtime.write().await;
        rt.synced = false;
        rt.cursors.clear();
    }
    run_auto_export(&app, &state).await;

    Ok(result)
}

/// Parse an import payload in either supported shape into `(source_key, records)`
/// groups plus the number of malformed JSONL lines skipped.
fn parse_import_payload(input: &str) -> Result<(ImportGroups, usize), String> {
    // Strip a leading UTF-8 BOM (also fixes the pre-existing snapshot BOM case).
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);

    let Some(first) = input.lines().map(str::trim).find(|l| !l.is_empty()) else {
        // Empty / whitespace-only — nothing to import. The frontend renders a
        // zero-record result as "No usage records found in that file".
        return Ok((Vec::new(), 0));
    };

    // Detect the JSONL log from its first line: either the header object or, if
    // the header is missing/torn, a bare {source_key, record} line.
    let is_jsonl = serde_json::from_str::<JsonlHeaderProbe>(first)
        .map(|p| p.format == EXPORT_FORMAT_JSONL)
        .unwrap_or(false)
        || serde_json::from_str::<ImportJsonlLine>(first).is_ok();
    if is_jsonl {
        return parse_jsonl_payload(input);
    }

    // Otherwise the single-document JSON snapshot.
    let doc: ImportDoc =
        serde_json::from_str(input).map_err(|e| format!("Not a valid TokenMonitor export: {e}"))?;
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
    let groups = doc
        .sources
        .into_iter()
        .map(|block| {
            let records = block
                .records
                .into_iter()
                .map(|r| r.into_archived(None))
                .collect();
            (block.source_key, records)
        })
        .collect();
    Ok((groups, 0))
}

/// Parse the line-delimited JSONL log into `(source_key, records)` groups. Blank
/// lines and the header line are skipped; a line that fails to parse (e.g. a
/// torn final append, or an oversized line) is skipped and counted rather than
/// aborting the whole import.
fn parse_jsonl_payload(input: &str) -> Result<(ImportGroups, usize), String> {
    let mut by_source: HashMap<String, Vec<ArchivedHourly>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut skipped = 0usize;

    for raw in input.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_JSONL_LINE_BYTES {
            tracing::warn!(bytes = line.len(), "Skipping oversized JSONL import line");
            skipped += 1;
            continue;
        }
        // Try the record shape FIRST. A record line ({source_key, record}) is
        // unambiguous, and trying it first means a record that happens to carry
        // a stray top-level "format" field can never be mistaken for a header
        // and silently dropped. A real header lacks source_key/record, so it
        // falls through to the header branch below.
        match serde_json::from_str::<ImportJsonlLine>(line) {
            Ok(line) => {
                let ImportJsonlLine {
                    source_key,
                    provider,
                    record,
                } = line;
                let archived = record.into_archived(provider.as_deref());
                if !by_source.contains_key(&source_key) {
                    order.push(source_key.clone());
                }
                by_source.entry(source_key).or_default().push(archived);
                continue;
            }
            Err(record_err) => {
                // Not a record: a header line (skip after a version check) or
                // genuinely corrupt (skip + count).
                if let Ok(probe) = serde_json::from_str::<JsonlHeaderProbe>(line) {
                    if probe.format == EXPORT_FORMAT_JSONL {
                        if probe.format_version > EXPORT_FORMAT_JSONL_VERSION {
                            return Err(format!(
                                "This file was created by a newer TokenMonitor (format {} > {EXPORT_FORMAT_JSONL_VERSION}). Please update.",
                                probe.format_version
                            ));
                        }
                        continue;
                    }
                }
                // Tolerate a corrupt/partial line instead of failing the import.
                tracing::warn!(error = %record_err, "Skipping malformed JSONL import line");
                skipped += 1;
            }
        }
    }

    let groups = order
        .into_iter()
        .map(|k| {
            let records = by_source.remove(&k).unwrap_or_default();
            (k, records)
        })
        .collect();
    Ok((groups, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(provider: &str, date: &str, hour: u8) -> ArchivedHourly {
        ArchivedHourly {
            d: date.to_string(),
            h: hour,
            mk: "sonnet-4-6".to_string(),
            mn: "Sonnet 4.6".to_string(),
            input_tokens: 100,
            out: 200,
            c5: 0,
            c1: 0,
            cr: 0,
            ws: 0,
            p: provider.to_string(),
        }
    }

    fn jsonl_line(source: &str, rec: &ArchivedHourly) -> String {
        record_line(source, "Test-Machine (macOS)", rec).unwrap()
    }

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

    #[test]
    fn export_line_drops_p_and_adds_cost_and_provider() {
        let rec = sample_record("claude", "2026-06-15", 10);
        let line = record_line("local:claude", "Zijia's MacBook Pro (macOS)", &rec).unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["source_key"], "local:claude");
        assert_eq!(v["device"], "Zijia's MacBook Pro (macOS)");
        assert_eq!(v["provider"], "claude");
        assert_eq!(v["record"]["d"], "2026-06-15");
        // p is gone; cost is present (a number, ≥ 0 even when pricing is absent).
        assert!(v["record"]["p"].is_null(), "record.p must not be serialized");
        assert!(v["record"]["cost"].is_number(), "record.cost must be present");
        assert!(v["record"].get("provider").is_none(), "JSONL line carries provider, not the record");
    }

    #[test]
    fn device_all_record_resolves_real_provider_never_all() {
        // SSH-device rows are archived with p="all"; the export must surface a
        // concrete provider derived from the model, never "all".
        let mut rec = sample_record("all", "2026-06-15", 10);
        rec.mk = "gpt-5".to_string();
        rec.mn = "GPT-5".to_string();
        let v: serde_json::Value =
            serde_json::from_str(&record_line("device:my-laptop", "my-laptop", &rec).unwrap())
                .unwrap();
        assert_eq!(v["provider"], "codex");
        assert!(v["record"]["p"].is_null());

        rec.mk = "claude-sonnet-4-6".to_string();
        let v2: serde_json::Value =
            serde_json::from_str(&record_line("device:my-laptop", "my-laptop", &rec).unwrap())
                .unwrap();
        assert_eq!(v2["provider"], "claude");
    }

    #[test]
    fn cursor_provider_wins_over_proxied_model_family() {
        // Cursor proxies gpt/claude/etc.; a local:cursor row must stay "cursor"
        // even when its model looks like another vendor.
        let mut rec = sample_record("cursor", "2026-06-15", 10);
        rec.mk = "gpt-4o".to_string();
        let v: serde_json::Value =
            serde_json::from_str(&record_line("local:cursor", "Mac (macOS)", &rec).unwrap())
                .unwrap();
        assert_eq!(v["provider"], "cursor");
    }

    #[test]
    fn own_source_excludes_foreign_peer_devices() {
        let ssh: HashSet<String> = ["DukeServer".to_string()].into_iter().collect();
        assert!(is_own_source("local:claude", &ssh));
        assert!(is_own_source("local:codex", &ssh));
        assert!(is_own_source("local:cursor", &ssh));
        assert!(is_own_source("device:DukeServer", &ssh)); // a device WE sync via SSH
        assert!(!is_own_source("device:PeerMac", &ssh)); // file-imported peer → not re-exported
        assert!(!is_own_source("garbage", &ssh));
    }

    #[test]
    fn peer_source_remaps_local_to_device() {
        // A peer's own local:* becomes device:<peerSlug>; a device the peer syncs
        // stays as-is (merges with ours if shared); anything else is dropped.
        assert_eq!(
            remap_peer_source("local:claude", "PeerMac").as_deref(),
            Some("device:PeerMac")
        );
        assert_eq!(
            remap_peer_source("local:codex", "PeerMac").as_deref(),
            Some("device:PeerMac")
        );
        assert_eq!(
            remap_peer_source("device:DukeServer", "PeerMac").as_deref(),
            Some("device:DukeServer")
        );
        assert_eq!(remap_peer_source("weird", "PeerMac"), None);
    }

    #[test]
    fn slugify_produces_valid_source_alias() {
        let slug = slugify("Zijia's MacBook Pro (macOS)");
        assert_eq!(slug, "Zijia-s-MacBook-Pro-macOS");
        // The slug must be usable as a device:<alias> source key (peers remap to it).
        assert!(is_valid_source_key(&format!("device:{slug}")));
        // Degenerate inputs never yield an empty / path-traversal slug.
        assert_eq!(slugify(""), "device");
        assert_eq!(slugify("///"), "device");
        assert_eq!(slugify(".."), "device");
        assert!(is_valid_source_key(&format!("device:{}", slugify("a/b\\c:d"))));
    }

    #[test]
    fn source_device_resolves_alias_vs_local() {
        let local = "ThisMac (macOS)";
        assert_eq!(source_device("device:my-laptop", local), "my-laptop");
        assert_eq!(source_device("device:laptop.local", local), "laptop.local");
        assert_eq!(source_device("local:claude", local), local);
        assert_eq!(source_device("local:codex", local), local);
        assert_eq!(source_device("local:cursor", local), local);
    }

    #[test]
    fn parses_new_snapshot_with_provider_no_p() {
        let snapshot = r#"{
            "format":"tokenmonitor-usage-export","format_version":1,
            "device":"Mac (macOS)",
            "sources":[{"source_key":"local:claude","records":[
              {"d":"2026-06-15","h":10,"mk":"opus-4-6","mn":"Opus 4.6","in":1,"out":2,"c5":0,"c1":0,"cr":0,"ws":0,"provider":"claude","cost":1.23}
            ]}]
        }"#;
        let (groups, skipped) = parse_import_payload(snapshot).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "local:claude");
        assert_eq!(groups[0].1.len(), 1);
        // provider reconstructed into the bucket p (cost ignored).
        assert_eq!(groups[0].1[0].p, "claude");
    }

    #[test]
    fn parses_legacy_snapshot_with_p() {
        // Old snapshot: records carry `p`, no `provider`/`cost`. Must still import
        // with p preserved byte-faithfully.
        let legacy = r#"{
            "format":"tokenmonitor-usage-export","format_version":1,
            "sources":[{"source_key":"device:srv","records":[
              {"d":"2026-06-15","h":10,"mk":"opus-4-6","mn":"Opus 4.6","in":1,"out":2,"c5":0,"c1":0,"cr":0,"ws":0,"p":"all"}
            ]}]
        }"#;
        let (groups, _) = parse_import_payload(legacy).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1[0].p, "all");
    }

    #[test]
    fn rejects_unknown_snapshot_format() {
        let json = r#"{"format":"something-else","format_version":1,"sources":[]}"#;
        assert!(parse_import_payload(json).is_err());
    }

    #[test]
    fn import_reconstructs_provider_from_jsonl_line() {
        // New JSONL: record has no p; the bucket p is rebuilt from the line-level
        // provider (here derived from the gpt model → codex).
        let mut rec = sample_record("all", "2026-06-15", 10);
        rec.mk = "gpt-5".to_string();
        let body = format!("{}\n", record_line("device:srv", "srv", &rec).unwrap());
        let (groups, skipped) = parse_import_payload(&body).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(groups[0].0, "device:srv");
        assert_eq!(groups[0].1[0].p, "codex");
    }

    #[test]
    fn parses_jsonl_with_header() {
        let header =
            r#"{"format":"tokenmonitor-usage-export-jsonl","format_version":1,"device":"Mac (macOS)"}"#;
        let r1 = sample_record("claude", "2026-06-15", 10);
        let r2 = sample_record("codex", "2026-06-15", 11);
        let body = format!(
            "{header}\n{}\n{}\n",
            jsonl_line("local:claude", &r1),
            jsonl_line("local:codex", &r2)
        );
        let (groups, skipped) = parse_import_payload(&body).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(groups.len(), 2);
        let total: usize = groups.iter().map(|(_, r)| r.len()).sum();
        assert_eq!(total, 2);
    }

    #[test]
    fn parses_headerless_jsonl() {
        let r1 = sample_record("claude", "2026-06-15", 10);
        let body = format!("{}\n", jsonl_line("local:claude", &r1));
        let (groups, skipped) = parse_import_payload(&body).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1.len(), 1);
    }

    #[test]
    fn tolerates_corrupt_trailing_line_in_jsonl() {
        let header = r#"{"format":"tokenmonitor-usage-export-jsonl","format_version":1}"#;
        let good = jsonl_line("local:claude", &sample_record("claude", "2026-06-15", 10));
        // A torn final append: valid prefix, no closing brace.
        let torn = r#"{"source_key":"local:claude","record":{"d":"2026-06-15","h":11"#;
        let body = format!("{header}\n{good}\n{torn}");
        let (groups, skipped) = parse_import_payload(&body).unwrap();
        assert_eq!(skipped, 1, "the torn line should be skipped and counted");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1.len(), 1, "the good record still imports");
    }

    #[test]
    fn empty_input_yields_empty_result() {
        let (groups, skipped) = parse_import_payload("   \n  \n").unwrap();
        assert!(groups.is_empty());
        assert_eq!(skipped, 0);
    }

    #[test]
    fn strips_leading_bom_before_parsing() {
        let r1 = sample_record("claude", "2026-06-15", 10);
        let body = format!("\u{feff}{}\n", jsonl_line("local:claude", &r1));
        let (groups, skipped) = parse_import_payload(&body).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn oversized_jsonl_line_is_skipped() {
        let header = r#"{"format":"tokenmonitor-usage-export-jsonl","format_version":1}"#;
        let huge = format!(
            r#"{{"source_key":"local:claude","record":"{}"}}"#,
            "x".repeat(MAX_JSONL_LINE_BYTES + 10)
        );
        let body = format!("{header}\n{huge}");
        let (groups, skipped) = parse_import_payload(&body).unwrap();
        assert_eq!(skipped, 1);
        assert!(groups.is_empty());
    }

    #[test]
    fn rejects_newer_jsonl_version() {
        let header = r#"{"format":"tokenmonitor-usage-export-jsonl","format_version":999}"#;
        // Headerless detection fails (it's a header), so it's treated as JSONL and
        // the version guard fires.
        let body = format!("{header}\n");
        assert!(parse_import_payload(&body).is_err());
    }

    #[test]
    fn record_line_with_stray_format_field_is_not_dropped_as_header() {
        // A record line that happens to carry a top-level `format` matching the
        // JSONL tag must still be imported (record shape is tried first).
        let header = r#"{"format":"tokenmonitor-usage-export-jsonl","format_version":1}"#;
        let rec = r#"{"format":"tokenmonitor-usage-export-jsonl","source_key":"local:claude","record":{"d":"2026-06-15","h":10,"mk":"opus-4-6","mn":"Opus 4.6","in":100,"out":200,"c5":0,"c1":0,"cr":0,"ws":0}}"#;
        let body = format!("{header}\n{rec}\n");
        let (groups, skipped) = parse_import_payload(&body).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1.len(), 1);
    }

    #[test]
    fn jsonl_preserves_invalid_source_key_for_caller_to_reject() {
        // parse_* does not validate source keys — the shared import loop does via
        // is_valid_source_key. Confirm a hostile key survives parsing unchanged.
        let r = sample_record("claude", "2026-06-15", 10);
        let body = format!("{}\n", jsonl_line("device:../escape", &r));
        let (groups, _) = parse_import_payload(&body).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "device:../escape");
        assert!(!is_valid_source_key(&groups[0].0));
    }
}
