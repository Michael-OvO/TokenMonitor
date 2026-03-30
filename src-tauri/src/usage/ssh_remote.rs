use std::path::{Path, PathBuf};

use tokio::process::Command;

/// Per-host sync state tracked by the cache manager.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHostStatus {
    pub alias: String,
    pub enabled: bool,
    pub last_sync: Option<String>,
    pub last_error: Option<String>,
    pub entry_count: u32,
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
}

/// Test SSH connectivity to a host using `ssh <host> echo ok`.
pub async fn test_connection(alias: &str) -> SshTestResult {
    let start = std::time::Instant::now();

    let result = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "LogLevel=ERROR",
            alias,
            "echo",
            "ok",
        ])
        .output()
        .await;

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
fn build_extraction_script(since_epoch: Option<u64>) -> String {
    let newer_filter = since_epoch
        .map(|ts| format!(" -newer /tmp/.tm-marker-{ts}"))
        .unwrap_or_default();

    let touch_cmd = since_epoch
        .map(|ts| format!("touch -d @{ts} /tmp/.tm-marker-{ts} 2>/dev/null; "))
        .unwrap_or_default();

    // Claude: search both config locations.
    let claude_find = format!(
        "for d in ~/.claude/projects ~/.config/claude/projects; do \
           [ -d \"$d\" ] && find \"$d\" -name '*.jsonl'{newer_filter} -type f 2>/dev/null; \
         done"
    );

    // Codex: search session directory.
    let codex_find = format!(
        "for d in ~/.codex/sessions; do \
           [ -d \"$d\" ] && find \"$d\" -name '*.jsonl'{newer_filter} -type f 2>/dev/null; \
         done"
    );

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
        "     if u:print(json.dumps({'ts':d.get('timestamp',''),",
        "'m':d.get('message',{}).get('model',''),",
        "'in':u.get('input_tokens',0),",
        "'out':u.get('output_tokens',0),",
        "'c5':u.get('cache_creation_input_tokens',0),",
        "'cr':u.get('cache_read_input_tokens',0)}))\n",
        "   except:pass\n",
        " except:pass\n",
    );

    // Codex python extraction — stateful: tracks model from turn_context,
    // extracts token deltas from last_token_usage / total_token_usage.
    // Normalizes cached_input out of input_tokens (Codex includes cached in input).
    let codex_py = concat!(
        "import json,sys\n",
        "for f in sys.argv[1:]:\n",
        " try:\n",
        "  m='';pi=0;po=0;pc=0\n",
        "  for line in open(f):\n",
        "   try:\n",
        "    d=json.loads(line)\n",
        "    t=d.get('type','')\n",
        "    if t=='turn_context':m=d.get('payload',{}).get('model','')\n",
        "    elif t=='event_msg':\n",
        "     p=d.get('payload',{})\n",
        "     if p.get('type')=='token_count':\n",
        "      nf=p.get('info') or {}\n",
        "      tu=nf.get('total_token_usage')\n",
        "      lu=nf.get('last_token_usage')\n",
        "      if tu:\n",
        "       i=tu.get('input_tokens',0);o=tu.get('output_tokens',0);c=tu.get('cached_input_tokens',0)\n",
        "       di=max(0,i-pi);do2=max(0,o-po);dc=max(0,c-pc)\n",
        "       pi=i;po=o;pc=c\n",
        "       if di>0 or do2>0:print(json.dumps({'ts':d.get('timestamp',''),'m':m,'in':max(0,di-dc),'out':do2,'c5':0,'cr':dc}))\n",
        "      elif lu:\n",
        "       i=lu.get('input_tokens',0);o=lu.get('output_tokens',0);c=lu.get('cached_input_tokens',0)\n",
        "       print(json.dumps({'ts':d.get('timestamp',''),'m':m,'in':max(0,i-c),'out':o,'c5':0,'cr':c}))\n",
        "   except:pass\n",
        " except:pass\n",
    );

    format!(
        "{touch_cmd}\
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
                 cr:(.message.usage.cache_read_input_tokens // 0)}}' 2>/dev/null; \
           elif command -v python3 >/dev/null 2>&1; then \
             echo \"$CLAUDE_FILES\" | tr '\\n' '\\0' | xargs -0 python3 -c \"{claude_py}\" 2>/dev/null; \
           else \
             echo \"$CLAUDE_FILES\" | xargs grep -lh '\"usage\"' 2>/dev/null | xargs grep -h '\"assistant\"' 2>/dev/null; \
           fi; \
         fi; \
         if [ -n \"$CODEX_FILES\" ]; then \
           if command -v python3 >/dev/null 2>&1; then \
             echo \"$CODEX_FILES\" | tr '\\n' '\\0' | xargs -0 python3 -c \"{codex_py}\" 2>/dev/null; \
           elif command -v python >/dev/null 2>&1; then \
             echo \"$CODEX_FILES\" | tr '\\n' '\\0' | xargs -0 python -c \"{codex_py}\" 2>/dev/null; \
           fi; \
         fi; \
         true"
    )
}

/// Fetch compact usage records from a remote host.
///
/// Runs the extraction script via SSH and parses the output.
/// Returns compact records (jq/python path) or raw JSONL lines (grep fallback).
pub async fn fetch_remote_usage(
    alias: &str,
    since_epoch: Option<u64>,
) -> Result<Vec<CompactUsageRecord>, String> {
    let script = build_extraction_script(since_epoch);
    let output = ssh_command(alias, &script).await?;

    if output.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
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

    Ok(records)
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
    })
}

/// Run a command on a remote host via SSH.
///
/// On Windows, passing complex shell scripts as a command-line argument to SSH
/// causes quoting issues (Windows `CreateProcess` double-escapes inner quotes).
/// To avoid this, we pass the script via stdin with `bash -s` on the remote.
async fn ssh_command(alias: &str, script: &str) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;

    let mut child = Command::new("ssh")
        .args([
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
        .stderr(std::process::Stdio::piped())
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

    if output.status.success() || !stdout.trim().is_empty() {
        return Ok(stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
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
//     .last-sync          — epoch timestamp
//     usage.jsonl          — compact records (appended on each sync)

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
    fn host_cache_dir(&self, alias: &str) -> PathBuf {
        let dir = self.base_dir.join(alias);
        // SAFETY: Ensure resolved path is under base_dir to prevent path traversal.
        assert!(
            dir.starts_with(&self.base_dir),
            "host_cache_dir resolved outside base_dir: {dir:?}"
        );
        dir
    }

    /// Path to the compact usage cache file for a host.
    fn usage_cache_path(&self, alias: &str) -> PathBuf {
        self.host_cache_dir(alias).join("usage.jsonl")
    }

    /// Read the last-sync timestamp for a host.
    pub fn last_sync_epoch(&self, alias: &str) -> Option<u64> {
        let marker = self.host_cache_dir(alias).join(".last-sync");
        std::fs::read_to_string(marker)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
    }

    /// Write the last-sync timestamp for a host.
    fn set_last_sync(&self, alias: &str) -> Result<(), String> {
        let dir = self.host_cache_dir(alias);
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;

        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let marker = dir.join(".last-sync");
        std::fs::write(marker, epoch.to_string()).map_err(|e| format!("write: {e}"))
    }

    /// Get status for all configured hosts.
    pub fn host_statuses(&self, configs: &[SshHostConfig]) -> Vec<SshHostStatus> {
        configs
            .iter()
            .map(|cfg| {
                let last_sync = self.last_sync_epoch(&cfg.alias).map(|ts| {
                    chrono::DateTime::from_timestamp(ts as i64, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default()
                });

                let entry_count = self.load_cached_records(&cfg.alias).len() as u32;

                SshHostStatus {
                    alias: cfg.alias.clone(),
                    enabled: cfg.enabled,
                    last_sync,
                    last_error: None,
                    entry_count,
                }
            })
            .collect()
    }

    /// Sync a single host: fetch compact usage data, append to cache.
    ///
    /// Returns the number of new records synced.
    pub async fn sync_host(&self, alias: &str) -> Result<u32, String> {
        let since = self.last_sync_epoch(alias);

        // Fetch compact usage records from remote.
        let records = fetch_remote_usage(alias, since).await?;

        if records.is_empty() {
            // Don't update last-sync when no records found — the remote
            // may have data that failed to extract.  Only advance the
            // timestamp after a successful non-empty fetch so future
            // syncs can retry with the full file scan.
            return Ok(0);
        }

        // Append new records to the cache file.
        let cache_path = self.usage_cache_path(alias);
        let dir = self.host_cache_dir(alias);
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;

        let mut lines = String::new();
        for record in &records {
            if let Ok(json) = serde_json::to_string(record) {
                lines.push_str(&json);
                lines.push('\n');
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

        let count = records.len() as u32;
        self.set_last_sync(alias)?;

        Ok(count)
    }

    /// Load all cached compact records for a host.
    pub fn load_cached_records(&self, alias: &str) -> Vec<CompactUsageRecord> {
        let cache_path = self.usage_cache_path(alias);
        let content = match std::fs::read_to_string(&cache_path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        content
            .lines()
            .filter_map(|line| serde_json::from_str::<CompactUsageRecord>(line).ok())
            .collect()
    }
}
