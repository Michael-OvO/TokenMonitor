use std::path::{Path, PathBuf};

use tokio::process::Command;

/// Windows: CREATE_NO_WINDOW flag prevents a console window from flashing.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Per-host sync state tracked by the cache manager.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHostStatus {
    pub alias: String,
    pub enabled: bool,
    pub last_sync: Option<String>,
    pub last_error: Option<String>,
    pub entry_count: u32,
    /// Remote server UTC offset, e.g. `"+0800"`, detected via `date +%z`.
    pub remote_tz: Option<String>,
}

/// Configuration for a user-managed SSH host (persisted in settings).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SshHostConfig {
    pub alias: String,
    pub enabled: bool,
    #[serde(default)]
    pub include_in_stats: bool,
}

/// Result of a test_connection call.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SshTestResult {
    pub success: bool,
    pub message: String,
    pub duration_ms: u64,
}

/// Combined result of a sync operation (with pre-test).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshSyncResult {
    /// Whether the connection test passed.
    pub test_success: bool,
    /// Human-readable test message (e.g., "Connected in 42ms").
    pub test_message: String,
    /// Connection test latency in milliseconds.
    pub test_duration_ms: u64,
    /// Number of new records synced (0 if test failed or no new data).
    pub records_synced: u32,
    /// Diagnostic hint when records_synced is 0 (helps the user understand why).
    pub diagnostic: Option<String>,
}

/// A compact usage record extracted from a remote JSONL file.
/// This is the ONLY data we transfer over SSH — not the full conversation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompactUsageRecord {
    pub ts: String,
    #[serde(rename = "m")]
    pub model: String,
    #[serde(rename = "in")]
    pub input_tokens: u64,
    #[serde(rename = "out")]
    pub output_tokens: u64,
    /// cache_creation_input_tokens (5-min tier)
    #[serde(rename = "c5", default)]
    pub cache_5m: u64,
    /// cache_creation_input_tokens (1-hour tier — currently mapped to same field)
    #[serde(rename = "c1", default)]
    pub cache_1h: u64,
    /// cache_read_input_tokens
    #[serde(rename = "cr", default)]
    pub cache_read: u64,
    #[serde(rename = "sp", default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
}

/// Test SSH connectivity to a host using `ssh <host> echo ok`.
pub async fn test_connection(alias: &str) -> SshTestResult {
    let start = std::time::Instant::now();

    let mut cmd = Command::new("ssh");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "LogLevel=ERROR",
        alias,
        "echo",
        "ok",
    ]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let result = cmd.output().await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().contains("ok") {
                SshTestResult {
                    success: true,
                    message: format!("Connected in {duration_ms}ms"),
                    duration_ms,
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let msg = if stderr.trim().is_empty() {
                    format!("Exit code {}", output.status.code().unwrap_or(-1))
                } else {
                    stderr.trim().to_string()
                };
                SshTestResult {
                    success: false,
                    message: msg,
                    duration_ms,
                }
            }
        }
        Err(e) => SshTestResult {
            success: false,
            message: format!("Failed to execute ssh: {e}"),
            duration_ms,
        },
    }
}

// ── Remote extraction script ────────────────────────────────────────────────
//
// Runs on the remote machine to extract ONLY usage metadata from JSONL files.
// Tries jq first (fastest), then python3/python fallback.
// Output: one compact JSON per line, ~120 bytes each (vs ~5KB for full entries).
//
// Estimated transfer: ~500MB raw → ~2-5MB compact.

/// Build the remote extraction script.
///
/// The script searches `~/.claude/projects/` and `~/.config/claude/projects/`
/// for .jsonl files, extracts assistant entries with usage data, and outputs
/// compact JSON records.
fn build_extraction_script(claude_since: Option<u64>, codex_since: Option<u64>) -> String {
    let newer_filter = claude_since
        .map(|ts| format!(" -newer /tmp/.tm-marker-{ts}"))
        .unwrap_or_default();

    let touch_cmd = claude_since
        .map(|ts| format!("touch -d @{ts} /tmp/.tm-marker-{ts} 2>/dev/null; "))
        .unwrap_or_default();

    // Claude: search both config locations.
    let claude_find = format!(
        "for d in ~/.claude/projects ~/.config/claude/projects; do \
           [ -d \"$d\" ] && find \"$d\" -name '*.jsonl'{newer_filter} -type f 2>/dev/null; \
         done"
    );

    // Codex: search session directory.
    // NOTE: Codex session files are written once and their mtime is never
    // updated, so the `-newer` filter (based on last-sync time) would
    // permanently skip them once Claude data advances the sync timestamp.
    // We always find ALL Codex files and filter by timestamp in the Python
    // extraction script instead.
    let codex_find = "for d in ~/.codex/sessions; do \
           [ -d \"$d\" ] && find \"$d\" -name '*.jsonl' -type f 2>/dev/null; \
         done"
        .to_string();

    // Claude python extraction — uses explicit \n + indentation to avoid
    // Rust's trailing-backslash line continuation eating the leading spaces.
    let claude_py = concat!(
        "import json,sys\n",
        "for f in sys.argv[1:]:\n",
        " try:\n",
        "  for line in open(f):\n",
        "   try:\n",
        "    d=json.loads(line)\n",
        "    if d.get('type')=='assistant':\n",
        "     u=d.get('message',{}).get('usage')\n",
        "     if u:\n",
        "      r={'ts':d.get('timestamp',''),",
        "'m':d.get('message',{}).get('model',''),",
        "'in':u.get('input_tokens',0),",
        "'out':u.get('output_tokens',0),",
        "'c5':u.get('cache_creation_input_tokens',0),",
        "'cr':u.get('cache_read_input_tokens',0)}\n",
        "      if u.get('speed')=='fast':r['sp']='fast'\n",
        "      print(json.dumps(r))\n",
        "   except Exception:pass\n",
        " except Exception as e:import sys;print('PARSE_ERR:'+str(e),file=sys.stderr)\n",
    );

    // Codex python extraction — stateful: tracks model from turn_context,
    // extracts token deltas from last_token_usage / total_token_usage.
    // Normalizes cached_input out of input_tokens (Codex includes cached in input).
    //
    // Because Codex files are not filtered by -newer (mtime never changes),
    // we pass `since_epoch` into the script and skip records whose timestamp
    // is at or before the cutoff.  This avoids duplicating already-cached data.
    let since_iso_filter = codex_since
        .and_then(|ts| {
            // Convert epoch to ISO-8601 string with UTC offset for Python comparison.
            // Including the timezone offset (+0000) ensures correct lexicographic
            // comparison even when remote timestamps carry a different offset.
            chrono::DateTime::from_timestamp(ts as i64, 0).map(|dt| {
                let formatted = dt.format("%Y-%m-%dT%H:%M:%S+0000").to_string();
                format!("S='{formatted}'\n")
            })
        })
        .unwrap_or_else(|| "S=''\n".to_string());

    // The script must still track cumulative state (pi/po/pc) for ALL records
    // to compute correct deltas, but only *print* records whose timestamp is
    // after the cutoff.  Skipping records entirely would corrupt the running
    // totals used for delta calculation.
    let codex_py = format!(
        "import json,sys\n\
         {since_iso_filter}\
         for f in sys.argv[1:]:\n\
         {S1}try:\n\
         {S2}m='';pi=0;po=0;pc=0\n\
         {S2}for line in open(f):\n\
         {S3}try:\n\
         {S4}d=json.loads(line)\n\
         {S4}t=d.get('type','')\n\
         {S4}if t=='turn_context':m=d.get('payload',{{}}).get('model','')\n\
         {S4}elif t=='event_msg':\n\
         {S5}p=d.get('payload',{{}})\n\
         {S5}if p.get('type')=='token_count':\n\
         {S6}nf=p.get('info') or {{}}\n\
         {S6}tu=nf.get('total_token_usage')\n\
         {S6}lu=nf.get('last_token_usage')\n\
         {S6}ts=d.get('timestamp','')\n\
         {S6}if tu:\n\
         {S7}i=tu.get('input_tokens',0);o=tu.get('output_tokens',0);c=tu.get('cached_input_tokens',0)\n\
         {S7}di=max(0,i-pi);do2=max(0,o-po);dc=max(0,c-pc)\n\
         {S7}pi=i;po=o;pc=c\n\
         {S7}if (not S or ts>S) and (di>0 or do2>0):print(json.dumps({{'ts':ts,'m':m,'in':max(0,di-dc),'out':do2,'c5':0,'cr':dc}}))\n\
         {S6}elif lu:\n\
         {S7}i=lu.get('input_tokens',0);o=lu.get('output_tokens',0);c=lu.get('cached_input_tokens',0)\n\
         {S7}if not S or ts>S:print(json.dumps({{'ts':ts,'m':m,'in':max(0,i-c),'out':o,'c5':0,'cr':c}}))\n\
         {S3}except Exception:pass\n\
         {S1}except Exception as e:import sys;print('PARSE_ERR:'+str(e),file=sys.stderr)\n",
        since_iso_filter = since_iso_filter,
        S1 = " ",
        S2 = "  ",
        S3 = "   ",
        S4 = "    ",
        S5 = "     ",
        S6 = "      ",
        S7 = "       ",
    );

    format!(
        "echo \"TZ_OFFSET:$(date +%z)\" 2>/dev/null; \
         {touch_cmd}\
         CLAUDE_FILES=$({claude_find}); \
         CODEX_FILES=$({codex_find}); \
         [ -z \"$CLAUDE_FILES\" ] && [ -z \"$CODEX_FILES\" ] && exit 0; \
         if [ -n \"$CLAUDE_FILES\" ]; then \
           if command -v jq >/dev/null 2>&1; then \
             echo \"$CLAUDE_FILES\" | xargs jq -c 'select(.type==\"assistant\" and .message.usage) | \
               {{ts:.timestamp, m:.message.model, \
                 \"in\":.message.usage.input_tokens, \
                 out:.message.usage.output_tokens, \
                 c5:(.message.usage.cache_creation_input_tokens // 0), \
                 cr:(.message.usage.cache_read_input_tokens // 0)}} + \
                 (if .message.usage.speed == \"fast\" then {{sp:\"fast\"}} else {{}} end)'; \
           elif command -v python3 >/dev/null 2>&1; then \
             echo \"$CLAUDE_FILES\" | tr '\\n' '\\0' | xargs -0 python3 -c \"{claude_py}\"; \
           else \
             echo \"$CLAUDE_FILES\" | xargs grep -lh '\"usage\"' 2>/dev/null | xargs grep -h '\"assistant\"' 2>/dev/null; \
           fi; \
         fi; \
         if [ -n \"$CODEX_FILES\" ]; then \
           if command -v python3 >/dev/null 2>&1; then \
             echo \"$CODEX_FILES\" | tr '\\n' '\\0' | xargs -0 python3 -c \"{codex_py}\"; \
           elif command -v python >/dev/null 2>&1; then \
             echo \"$CODEX_FILES\" | tr '\\n' '\\0' | xargs -0 python -c \"{codex_py}\"; \
           fi; \
         fi"
    )
}

/// Result of fetching remote usage: records + optional timezone offset.
pub struct FetchRemoteResult {
    pub records: Vec<CompactUsageRecord>,
    /// The remote server's UTC offset, e.g. `"+0800"`, `"-0500"`, `"+0000"`.
    /// Detected via `date +%z` on the remote host.
    pub remote_tz: Option<String>,
}

/// Fetch compact usage records from a remote host.
///
/// Runs the extraction script via SSH and parses the output.
/// Returns compact records (jq/python path) or raw JSONL lines (grep fallback),
/// plus the detected remote timezone offset.
pub async fn fetch_remote_usage(
    alias: &str,
    claude_since: Option<u64>,
    codex_since: Option<u64>,
) -> Result<FetchRemoteResult, String> {
    let script = build_extraction_script(claude_since, codex_since);
    let output = ssh_command(alias, &script).await?;

    if output.trim().is_empty() {
        return Ok(FetchRemoteResult {
            records: Vec::new(),
            remote_tz: None,
        });
    }

    let mut records = Vec::new();
    let mut remote_tz: Option<String> = None;

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse the TZ_OFFSET marker emitted by `date +%z` at script start.
        if let Some(tz) = line.strip_prefix("TZ_OFFSET:") {
            let tz = tz.trim();
            if !tz.is_empty() {
                remote_tz = Some(tz.to_string());
            }
            continue;
        }

        // Try compact format first (jq/python output).
        if let Ok(record) = serde_json::from_str::<CompactUsageRecord>(line) {
            // Skip synthetic/internal models.
            if !record.model.starts_with('<') {
                records.push(record);
            }
            continue;
        }

        // Fallback: try parsing as full Claude JSONL entry (grep output).
        if let Some(record) = parse_full_entry_to_compact(line) {
            records.push(record);
        }
    }

    Ok(FetchRemoteResult { records, remote_tz })
}

/// Parse a full Claude JSONL line into a compact record.
/// Used for the grep fallback path.
fn parse_full_entry_to_compact(line: &str) -> Option<CompactUsageRecord> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;

    if value.get("type")?.as_str()? != "assistant" {
        return None;
    }

    let message = value.get("message")?;
    let usage = message.get("usage")?;

    let model = message.get("model")?.as_str().unwrap_or("unknown");
    // Skip synthetic/internal models (same filter as parser.rs).
    if model.starts_with('<') {
        return None;
    }

    let speed = usage
        .get("speed")
        .and_then(|v| v.as_str())
        .filter(|s| *s == "fast")
        .map(String::from);

    Some(CompactUsageRecord {
        ts: value.get("timestamp")?.as_str()?.to_string(),
        model: model.to_string(),
        input_tokens: usage.get("input_tokens")?.as_u64().unwrap_or(0),
        output_tokens: usage.get("output_tokens")?.as_u64().unwrap_or(0),
        cache_5m: usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cache_1h: 0,
        cache_read: usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        speed,
    })
}

/// Run a command on a remote host via SSH.
///
/// On Windows, passing complex shell scripts as a command-line argument to SSH
/// causes quoting issues (Windows `CreateProcess` double-escapes inner quotes).
/// To avoid this, we pass the script via stdin with `bash -s` on the remote.
async fn ssh_command(alias: &str, script: &str) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;

    let mut cmd = Command::new("ssh");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "LogLevel=ERROR",
        alias,
        "bash -s",
    ])
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn ssh: {e}"))?;

    // Write script to stdin and close it so the remote bash exits.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to ssh stdin: {e}"))?;
        // stdin is dropped here, closing the pipe.
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Failed to wait for ssh: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Log stderr diagnostics even on success (e.g. Python PARSE_ERR lines).
    if !stderr.trim().is_empty() && output.status.success() {
        tracing::debug!("SSH stderr for {alias} (exit 0): {}", stderr.trim());
    }

    if output.status.success() {
        return Ok(stdout);
    }

    // Log stderr when exit code is non-zero.
    if !stderr.trim().is_empty() {
        tracing::warn!("SSH stderr for {alias}: {}", stderr.trim());
    }

    let msg = if stderr.trim().is_empty() {
        format!(
            "SSH exited with code {}",
            output.status.code().unwrap_or(-1)
        )
    } else {
        stderr.trim().to_string()
    };
    Err(format!("SSH command failed: {msg}"))
}

// ── Cache manager ───────────────────────────────────────────────────────────
//
// Stores compact usage records per host, not raw JSONL files.
// Cache structure:
//   {app_data_dir}/remote-cache/{hostname}/
//     .last-sync-claude    — epoch timestamp for Claude provider
//     .last-sync-codex     — epoch timestamp for Codex provider
//     usage.jsonl           — compact records (appended on each sync)

#[derive(Clone)]
pub struct SshCacheManager {
    base_dir: PathBuf,
}

impl SshCacheManager {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            base_dir: app_data_dir.join("remote-cache"),
        }
    }

    /// Get the cache directory for a specific host.
    ///
    /// Validates that the resolved path stays within `base_dir` to prevent
    /// path traversal attacks via crafted alias values.
    fn host_cache_dir(&self, alias: &str) -> Result<PathBuf, String> {
        let dir = self.base_dir.join(alias);
        if !dir.starts_with(&self.base_dir) {
            return Err(format!("host_cache_dir resolved outside base_dir: {dir:?}"));
        }
        Ok(dir)
    }

    /// Path to the compact usage cache file for a host.
    fn usage_cache_path(&self, alias: &str) -> Result<PathBuf, String> {
        Ok(self.host_cache_dir(alias)?.join("usage.jsonl"))
    }

    /// Migrate legacy `.last-sync` to per-provider files if needed.
    ///
    /// Copies the old unified timestamp to both `.last-sync-claude` and
    /// `.last-sync-codex` (only when neither exists yet), then removes
    /// the old file.
    pub fn migrate_legacy_sync_marker(&self, alias: &str) {
        let dir = match self.host_cache_dir(alias) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("migrate_legacy_sync_marker: {e}");
                return;
            }
        };
        let legacy = dir.join(".last-sync");
        if !legacy.exists() {
            return;
        }
        let claude_marker = dir.join(".last-sync-claude");
        let codex_marker = dir.join(".last-sync-codex");
        if claude_marker.exists() || codex_marker.exists() {
            // Already migrated — clean up the legacy file.
            if let Err(e) = std::fs::remove_file(&legacy) {
                tracing::warn!("Failed to remove legacy sync marker: {e}");
            }
            return;
        }
        if let Ok(content) = std::fs::read_to_string(&legacy) {
            // Only migrate Claude's timestamp; leave Codex empty so it
            // does a full first sync (Codex data was never synced before).
            if let Err(e) = std::fs::write(&claude_marker, content.trim()) {
                tracing::warn!("Failed to write claude sync marker: {e}");
            }
        }
        if let Err(e) = std::fs::remove_file(&legacy) {
            tracing::warn!("Failed to remove legacy sync marker: {e}");
        }
    }

    /// Validate that a provider name is safe for use in filenames.
    fn validate_provider(provider: &str) -> Result<(), String> {
        if !matches!(provider, "claude" | "codex") {
            return Err(format!("Invalid provider: {provider}"));
        }
        Ok(())
    }

    /// Read the persisted remote timezone offset for a host.
    pub fn host_timezone(&self, alias: &str) -> Option<String> {
        let dir = self.host_cache_dir(alias).ok()?;
        let path = dir.join(".timezone");
        std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Persist the remote timezone offset for a host.
    fn set_host_timezone(&self, alias: &str, tz: &str) -> Result<(), String> {
        let dir = self.host_cache_dir(alias)?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
        let path = dir.join(".timezone");
        std::fs::write(path, tz).map_err(|e| format!("write timezone: {e}"))
    }

    /// Read the last-sync timestamp for a host and provider.
    pub fn last_sync_epoch(&self, alias: &str, provider: &str) -> Option<u64> {
        Self::validate_provider(provider).ok()?;
        let dir = self.host_cache_dir(alias).ok()?;
        let marker = dir.join(format!(".last-sync-{provider}"));
        std::fs::read_to_string(marker)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
    }

    /// Write the last-sync timestamp for a host and provider.
    fn set_last_sync(&self, alias: &str, provider: &str) -> Result<(), String> {
        Self::validate_provider(provider)?;
        let dir = self.host_cache_dir(alias)?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;

        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let marker = dir.join(format!(".last-sync-{provider}"));
        std::fs::write(marker, epoch.to_string()).map_err(|e| format!("write: {e}"))
    }

    /// Get status for all configured hosts.
    pub fn host_statuses(&self, configs: &[SshHostConfig]) -> Vec<SshHostStatus> {
        configs
            .iter()
            .map(|cfg| {
                // Display the most recent sync time across providers.
                let claude_ts = self.last_sync_epoch(&cfg.alias, "claude");
                let codex_ts = self.last_sync_epoch(&cfg.alias, "codex");
                let latest_ts = claude_ts.max(codex_ts);
                let last_sync = latest_ts.map(|ts| {
                    chrono::DateTime::from_timestamp(ts as i64, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default()
                });

                let entry_count = match self.count_cached_records(&cfg.alias) {
                    Ok(count) => count,
                    Err(e) => {
                        tracing::warn!("Failed to count cached records for {}: {e}", cfg.alias);
                        0
                    }
                };

                let remote_tz = self.host_timezone(&cfg.alias);

                SshHostStatus {
                    alias: cfg.alias.clone(),
                    enabled: cfg.enabled,
                    last_sync,
                    last_error: None,
                    entry_count,
                    remote_tz,
                }
            })
            .collect()
    }

    /// Sync a single host: fetch compact usage data, append to cache.
    ///
    /// Returns the number of new records synced.
    pub async fn sync_host(&self, alias: &str) -> Result<u32, String> {
        self.migrate_legacy_sync_marker(alias);

        let claude_since = self.last_sync_epoch(alias, "claude");
        let codex_since = self.last_sync_epoch(alias, "codex");

        // Fetch compact usage records from remote.
        let result = fetch_remote_usage(alias, claude_since, codex_since).await?;

        // Persist remote timezone regardless of whether records were found.
        if let Some(tz) = &result.remote_tz {
            if let Err(e) = self.set_host_timezone(alias, tz) {
                tracing::warn!("Failed to persist timezone for {alias}: {e}");
            }
        }

        let records = result.records;

        if records.is_empty() {
            // Don't update last-sync when no records found — the remote
            // may have data that failed to extract.  Only advance the
            // timestamp after a successful non-empty fetch so future
            // syncs can retry with the full file scan.
            return Ok(0);
        }

        // Append new records to the cache file.
        let cache_path = self.usage_cache_path(alias)?;
        let dir = self.host_cache_dir(alias)?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;

        let mut lines = String::new();
        let mut has_claude = false;
        let mut has_codex = false;
        for record in &records {
            if let Ok(json) = serde_json::to_string(record) {
                lines.push_str(&json);
                lines.push('\n');
            }
            // Detect provider by model family.
            use crate::models::{detect_model_family, ModelFamily};
            match detect_model_family(&record.model) {
                ModelFamily::Anthropic => has_claude = true,
                ModelFamily::OpenAI => has_codex = true,
                _ => {} // Other models don't affect provider timestamps.
            }
        }

        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&cache_path)
            .map_err(|e| format!("open cache: {e}"))?;
        let mut writer = std::io::BufWriter::new(file);
        writer
            .write_all(lines.as_bytes())
            .map_err(|e| format!("write cache: {e}"))?;

        // Only advance a provider's timestamp when that provider had data.
        if has_claude {
            self.set_last_sync(alias, "claude")?;
        }
        if has_codex {
            self.set_last_sync(alias, "codex")?;
        }

        Ok(records.len() as u32)
    }

    /// Count cached records without parsing JSON (line count).
    pub fn count_cached_records(&self, alias: &str) -> Result<u32, String> {
        let cache_path = self.usage_cache_path(alias)?;
        let content = match std::fs::read_to_string(&cache_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(format!("Failed to read cache for {alias}: {e}")),
        };
        Ok(content.lines().filter(|l| !l.trim().is_empty()).count() as u32)
    }

    /// Load all cached compact records for a host.
    pub fn load_cached_records(&self, alias: &str) -> Result<Vec<CompactUsageRecord>, String> {
        let cache_path = self.usage_cache_path(alias)?;
        let content = match std::fs::read_to_string(&cache_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("Failed to read cache for {alias}: {e}")),
        };

        Ok(content
            .lines()
            .filter_map(|line| serde_json::from_str::<CompactUsageRecord>(line).ok())
            .collect())
    }
}
