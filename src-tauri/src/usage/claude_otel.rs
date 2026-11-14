//! Local OpenTelemetry receiver for Claude Code API requests.
//!
//! Headless `claude -p --no-session-persistence` runs write no session log, so
//! the JSONL parser never sees them, yet they draw down the plan's limits.
//! Claude Code can export an `api_request` event per API call over OTLP. This
//! module listens on `127.0.0.1:OTEL_PORT` for OTLP/HTTP **JSON** posts, keeps
//! only the usage fields of each `api_request` event (model, token counts,
//! request id, session id, timestamp — no prompt, account or e-mail data), and
//! appends them as Claude-Code-shaped `assistant` lines to
//! `paths::claude_otel_ledger_dir()`. That directory is an extra Claude log
//! root, so pricing, caching, charts and plan budgets all reuse the normal
//! pipeline. A request that a session log also records is counted once
//! (`claude_parser::upsert_claude_entry` dedupes on the request id).
//!
//! Enable in the shell that starts Claude Code (settings-file `env` is skipped
//! by `--setting-sources ""`):
//!
//! ```sh
//! export CLAUDE_CODE_ENABLE_TELEMETRY=1
//! export OTEL_LOGS_EXPORTER=otlp
//! export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
//! export OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:53218
//! ```
//!
//! The server is a deliberately small HTTP/1.1 reader on `std::net` (no new
//! dependencies): Content-Length or chunked bodies, keep-alive, bounded sizes
//! and timeouts. Anything that is not an OTLP logs post is answered and
//! dropped. Requests arriving while the app is closed are lost.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::{json, Value};

use super::claude_parser::OTEL_MESSAGE_ID;

/// Next to the single-instance lock port (53217).
pub const OTEL_PORT: u16 = 53218;

const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REQUESTS_PER_CONN: usize = 1_000;

static LISTENING: AtomicBool = AtomicBool::new(false);
static BIND_ERROR: OnceLock<String> = OnceLock::new();
static EVENTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static LAST_EVENT_MS: AtomicI64 = AtomicI64::new(0);
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeOtelStatus {
    pub port: u16,
    pub listening: bool,
    pub bind_error: Option<String>,
    /// API requests recorded since launch.
    pub events_received: u64,
    /// RFC 3339 time of the newest recorded request since launch.
    pub last_event_at: Option<String>,
    pub ledger_dir: Option<String>,
}

pub fn status() -> ClaudeOtelStatus {
    let last = LAST_EVENT_MS.load(Ordering::Relaxed);
    ClaudeOtelStatus {
        port: OTEL_PORT,
        listening: LISTENING.load(Ordering::Relaxed),
        bind_error: BIND_ERROR.get().cloned(),
        events_received: EVENTS_TOTAL.load(Ordering::Relaxed),
        last_event_at: (last > 0)
            .then(|| Utc.timestamp_millis_opt(last).single())
            .flatten()
            .map(|t| t.to_rfc3339()),
        ledger_dir: crate::paths::claude_otel_ledger_dir().map(|p| p.display().to_string()),
    }
}

#[tauri::command]
pub fn get_claude_otel_status() -> ClaudeOtelStatus {
    status()
}

/// Bind and serve on a background thread. Call once, from the owning instance.
pub fn spawn() {
    let Some(dir) = crate::paths::claude_otel_ledger_dir() else {
        let _ = BIND_ERROR.set("no data directory".into());
        return;
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, OTEL_PORT));
    let listener = match TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("Claude OTel receiver could not bind {addr}: {e}");
            let _ = BIND_ERROR.set(e.to_string());
            return;
        }
    };
    LISTENING.store(true, Ordering::Relaxed);
    tracing::info!("Claude OTel receiver listening on {addr}");
    let spawned = std::thread::Builder::new()
        .name("claude-otel-accept".into())
        .spawn(move || {
            for incoming in listener.incoming() {
                match incoming {
                    Ok(stream) => {
                        let dir = dir.clone();
                        let _ = std::thread::Builder::new()
                            .name("claude-otel-conn".into())
                            .spawn(move || serve_connection(stream, &dir));
                    }
                    Err(e) => tracing::debug!("Claude OTel accept error: {e}"),
                }
            }
        });
    if let Err(e) = spawned {
        LISTENING.store(false, Ordering::Relaxed);
        let _ = BIND_ERROR.set(e.to_string());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// HTTP
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    content_type: String,
    content_encoding: String,
    keep_alive: bool,
    body: Vec<u8>,
}

fn serve_connection(stream: TcpStream, dir: &Path) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut writer = stream;
    for _ in 0..MAX_REQUESTS_PER_CONN {
        let req = match read_request(&mut reader) {
            Ok(Some(req)) => req,
            Ok(None) => return,
            Err(status) => {
                let _ = write_response(&mut writer, status, "{}", false);
                return;
            }
        };
        let (status, body) = handle(&req, dir);
        if write_response(&mut writer, status, &body, req.keep_alive).is_err() || !req.keep_alive {
            return;
        }
    }
}

/// `Ok(None)` on a clean close between requests; `Err(status)` on a bad request.
fn read_request(reader: &mut impl BufRead) -> Result<Option<Request>, u16> {
    let mut head_bytes = 0usize;
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => return Ok(None),
        Ok(n) => head_bytes += n,
        Err(_) => return Ok(None),
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or(400u16)?.to_string();
    let path = parts.next().ok_or(400u16)?.to_string();
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();

    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    let mut content_type = String::new();
    let mut content_encoding = String::new();
    let mut keep_alive = version != "HTTP/1.0";
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        head_bytes += n;
        if head_bytes > MAX_HEADER_BYTES {
            return Err(431);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        let Some((name, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => content_length = Some(value.parse().map_err(|_| 400u16)?),
            "transfer-encoding" => chunked = value.to_ascii_lowercase().contains("chunked"),
            "content-type" => content_type = value.to_ascii_lowercase(),
            "content-encoding" => content_encoding = value.to_ascii_lowercase(),
            "connection" => {
                let v = value.to_ascii_lowercase();
                if v.contains("close") {
                    keep_alive = false;
                } else if v.contains("keep-alive") {
                    keep_alive = true;
                }
            }
            _ => {}
        }
    }

    let body = if chunked {
        read_chunked(reader)?
    } else {
        let len = content_length.unwrap_or(0);
        if len > MAX_BODY_BYTES {
            return Err(413);
        }
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body).map_err(|_| 400u16)?;
        body
    };
    Ok(Some(Request {
        method,
        path,
        content_type,
        content_encoding,
        keep_alive,
        body,
    }))
}

fn read_chunked(reader: &mut impl BufRead) -> Result<Vec<u8>, u16> {
    let mut body = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line).map_err(|_| 400u16)?;
        let size_hex = line.trim().split(';').next().unwrap_or("");
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| 400u16)?;
        if size == 0 {
            // Trailers until the blank line.
            loop {
                line.clear();
                let n = reader.read_line(&mut line).map_err(|_| 400u16)?;
                if n == 0 || line.trim().is_empty() {
                    return Ok(body);
                }
            }
        }
        if body.len() + size > MAX_BODY_BYTES {
            return Err(413);
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..]).map_err(|_| 400u16)?;
        line.clear();
        reader.read_line(&mut line).map_err(|_| 400u16)?; // CRLF after the chunk
    }
}

fn write_response(
    w: &mut impl Write,
    status: u16,
    body: &str,
    keep_alive: bool,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        _ => "Internal Server Error",
    };
    let connection = if keep_alive { "keep-alive" } else { "close" };
    write!(
        w,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {connection}\r\n\r\n{body}",
        body.len()
    )?;
    w.flush()
}

fn handle(req: &Request, dir: &Path) -> (u16, String) {
    if req.method != "POST" {
        return (405, "{}".into());
    }
    let path = req.path.split('?').next().unwrap_or("");
    if !path.starts_with("/v1/") {
        return (404, "{}".into());
    }
    if !req.content_encoding.is_empty() && req.content_encoding != "identity" {
        return (
            415,
            json!({"message": "TokenMonitor accepts uncompressed OTLP JSON only"}).to_string(),
        );
    }
    if !req.content_type.starts_with("application/json") {
        return (
            415,
            json!({"message": "TokenMonitor accepts OTLP/HTTP JSON only; set OTEL_EXPORTER_OTLP_PROTOCOL=http/json"})
                .to_string(),
        );
    }
    // Metrics and traces are accepted and dropped: api_request events carry everything.
    if path != "/v1/logs" {
        return (200, "{}".into());
    }
    let Ok(payload) = serde_json::from_slice::<Value>(&req.body) else {
        return (400, "{}".into());
    };
    let records = api_request_records(&payload);
    if !records.is_empty() {
        if let Err(e) = append_records(dir, &records) {
            tracing::warn!("Claude OTel ledger write failed: {e}");
            return (500, "{}".into());
        }
        EVENTS_TOTAL.fetch_add(records.len() as u64, Ordering::Relaxed);
        if let Some(newest) = records.iter().map(|r| r.timestamp).max() {
            LAST_EVENT_MS.fetch_max(newest.timestamp_millis(), Ordering::Relaxed);
        }
    }
    (200, "{}".into())
}

// ─────────────────────────────────────────────────────────────────────────────
// OTLP → ledger
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LedgerRecord {
    pub timestamp: DateTime<Utc>,
    pub line: String,
}

fn attr_value(v: &Value) -> Option<&Value> {
    let obj = v.as_object()?;
    ["stringValue", "intValue", "doubleValue", "boolValue"]
        .iter()
        .find_map(|k| obj.get(*k))
}

fn attrs_of(record: &Value) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    if let Some(list) = record.get("attributes").and_then(Value::as_array) {
        for kv in list {
            if let (Some(k), Some(v)) = (
                kv.get("key").and_then(Value::as_str),
                kv.get("value").and_then(attr_value),
            ) {
                out.insert(k.to_string(), v.clone());
            }
        }
    }
    out
}

fn as_str<'a>(attrs: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    attrs
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// OTLP JSON encodes 64-bit ints as numbers or strings; accept both.
fn as_u64(attrs: &serde_json::Map<String, Value>, key: &str) -> u64 {
    match attrs.get(key) {
        Some(Value::Number(n)) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
            .unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn record_time(record: &Value, attrs: &serde_json::Map<String, Value>) -> Option<DateTime<Utc>> {
    if let Some(ts) = as_str(attrs, "event.timestamp") {
        if let Ok(t) = DateTime::parse_from_rfc3339(ts) {
            return Some(t.with_timezone(&Utc));
        }
    }
    let nanos = match record.get("timeUnixNano") {
        Some(Value::String(s)) => s.parse::<i64>().ok(),
        Some(Value::Number(n)) => n.as_i64(),
        _ => None,
    }?;
    (nanos > 0).then(|| Utc.timestamp_nanos(nanos))
}

/// Ledger lines for every Claude Code `api_request` log record in an OTLP
/// logs payload. Other events, and records without a model, are skipped.
pub(crate) fn api_request_records(payload: &Value) -> Vec<LedgerRecord> {
    let mut out = Vec::new();
    let resource_logs = payload
        .get("resourceLogs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    for rl in resource_logs {
        let scope_logs = rl.get("scopeLogs").and_then(Value::as_array);
        for sl in scope_logs.into_iter().flatten() {
            let records = sl.get("logRecords").and_then(Value::as_array);
            for record in records.into_iter().flatten() {
                let attrs = attrs_of(record);
                let body_name = record
                    .get("body")
                    .and_then(|b| b.get("stringValue"))
                    .and_then(Value::as_str);
                let is_api_request = as_str(&attrs, "event.name") == Some("api_request")
                    || body_name == Some("claude_code.api_request");
                if !is_api_request {
                    continue;
                }
                if let Some(rec) = ledger_record(record, &attrs) {
                    out.push(rec);
                }
            }
        }
    }
    out
}

fn ledger_record(record: &Value, attrs: &serde_json::Map<String, Value>) -> Option<LedgerRecord> {
    let model = as_str(attrs, "model")?;
    let timestamp = record_time(record, attrs)?;
    let request_id = as_str(attrs, "request_id")
        .or_else(|| as_str(attrs, "client_request_id"))?
        .replace(':', "_");
    let cache_creation = as_u64(attrs, "cache_creation_tokens");
    let five_m = as_u64(attrs, "cache_creation_5m_tokens");
    let one_h = as_u64(attrs, "cache_creation_1h_tokens");
    let mut usage = json!({
        "input_tokens": as_u64(attrs, "input_tokens"),
        "output_tokens": as_u64(attrs, "output_tokens"),
        "cache_read_input_tokens": as_u64(attrs, "cache_read_tokens"),
        "cache_creation_input_tokens": cache_creation.max(five_m + one_h),
    });
    // Without a 5m/1h split the parser files all cache writes under 1h,
    // matching how it reads session logs that lack the breakdown.
    if five_m + one_h > 0 {
        usage["cache_creation"] = json!({
            "ephemeral_5m_input_tokens": five_m,
            "ephemeral_1h_input_tokens": one_h,
        });
    }
    if as_str(attrs, "speed") == Some("fast") {
        usage["speed"] = json!("fast");
    }
    let line = json!({
        "type": "assistant",
        "timestamp": timestamp.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "sessionId": as_str(attrs, "session.id").unwrap_or("otel"),
        "requestId": request_id,
        "isSidechain": false,
        "message": {
            "id": OTEL_MESSAGE_ID,
            "model": model,
            "usage": usage,
        },
    })
    .to_string();
    Some(LedgerRecord { timestamp, line })
}

fn ledger_file(dir: &Path, timestamp: DateTime<Utc>) -> PathBuf {
    dir.join(format!(
        "claude-otel-{}.jsonl",
        timestamp.format("%Y-%m-%d")
    ))
}

/// Append records to one file per UTC day. One `write_all` per file under a
/// process-wide lock, so concurrent connections never interleave lines.
pub(crate) fn append_records(dir: &Path, records: &[LedgerRecord]) -> std::io::Result<()> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    fs::create_dir_all(dir)?;
    let mut by_file: std::collections::BTreeMap<PathBuf, String> = Default::default();
    for r in records {
        let buf = by_file.entry(ledger_file(dir, r.timestamp)).or_default();
        buf.push_str(&r.line);
        buf.push('\n');
    }
    for (path, buf) in by_file {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(buf.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    fn kv(key: &str, value: Value) -> Value {
        json!({"key": key, "value": value})
    }

    /// Shape captured from Claude Code 2.1.325 (`claude -p --no-session-persistence`).
    fn api_request_payload() -> Value {
        json!({"resourceLogs": [{
            "resource": {"attributes": [kv("service.name", json!({"stringValue": "claude-code"}))]},
            "scopeLogs": [{
                "scope": {"name": "com.anthropic.claude_code.events"},
                "logRecords": [
                    {
                        "timeUnixNano": "1794641533762000000",
                        "body": {"stringValue": "claude_code.user_prompt"},
                        "attributes": [kv("event.name", json!({"stringValue": "user_prompt"}))]
                    },
                    {
                        "timeUnixNano": "1794641535124000000",
                        "body": {"stringValue": "claude_code.api_request"},
                        "attributes": [
                            kv("user.email", json!({"stringValue": "someone@example.com"})),
                            kv("session.id", json!({"stringValue": "sess-1"})),
                            kv("event.name", json!({"stringValue": "api_request"})),
                            kv("event.timestamp", json!({"stringValue": "2026-11-14T07:32:15.124Z"})),
                            kv("model", json!({"stringValue": "claude-opus-5-5"})),
                            kv("input_tokens", json!({"intValue": 2})),
                            kv("output_tokens", json!({"intValue": "14"})),
                            kv("cache_read_tokens", json!({"intValue": 3660})),
                            kv("cache_creation_tokens", json!({"intValue": 500})),
                            kv("cache_creation_5m_tokens", json!({"intValue": 200})),
                            kv("cache_creation_1h_tokens", json!({"intValue": 300})),
                            kv("cost_usd", json!({"doubleValue": 0.0000438})),
                            kv("request_id", json!({"stringValue": "req_011Ch2RxaHP5P6PrLCNV9WXs"})),
                            kv("speed", json!({"stringValue": "normal"}))
                        ]
                    }
                ]
            }]
        }]})
    }

    #[test]
    fn api_request_becomes_claude_assistant_line_without_identity_fields() {
        let records = api_request_records(&api_request_payload());
        assert_eq!(records.len(), 1, "only the api_request event is kept");
        let line: Value = serde_json::from_str(&records[0].line).unwrap();
        assert_eq!(line["type"], "assistant");
        assert_eq!(line["timestamp"], "2026-11-14T07:32:15.124Z");
        assert_eq!(line["sessionId"], "sess-1");
        assert_eq!(line["requestId"], "req_011Ch2RxaHP5P6PrLCNV9WXs");
        assert_eq!(line["message"]["id"], OTEL_MESSAGE_ID);
        assert_eq!(line["message"]["model"], "claude-opus-5-5");
        assert_eq!(
            line["message"]["usage"],
            json!({
                "input_tokens": 2, "output_tokens": 14, "cache_read_input_tokens": 3660,
                "cache_creation_input_tokens": 500,
                "cache_creation": {"ephemeral_5m_input_tokens": 200, "ephemeral_1h_input_tokens": 300}
            })
        );
        assert!(!records[0].line.contains("example.com"));
    }

    #[test]
    fn falls_back_to_time_unix_nano_and_marks_fast_mode() {
        let payload = json!({"resourceLogs": [{"scopeLogs": [{"logRecords": [{
            "timeUnixNano": "1794641535124000000",
            "attributes": [
                kv("event.name", json!({"stringValue": "api_request"})),
                kv("model", json!({"stringValue": "claude-opus-5-5"})),
                kv("output_tokens", json!({"intValue": 9})),
                kv("speed", json!({"stringValue": "fast"})),
                kv("client_request_id", json!({"stringValue": "c-1"}))
            ]
        }]}]}]});
        let records = api_request_records(&payload);
        let line: Value = serde_json::from_str(&records[0].line).unwrap();
        assert_eq!(records[0].timestamp.timestamp_millis(), 1_794_641_535_124);
        assert_eq!(line["requestId"], "c-1");
        assert_eq!(line["message"]["usage"]["speed"], "fast");
        assert!(line["message"]["usage"].get("cache_creation").is_none());
    }

    #[test]
    fn skips_records_without_model_time_or_request_id() {
        let base = |extra: Vec<Value>| {
            let mut attrs = vec![kv("event.name", json!({"stringValue": "api_request"}))];
            attrs.extend(extra);
            json!({"resourceLogs": [{"scopeLogs": [{"logRecords": [{"attributes": attrs}]}]}]})
        };
        let model = kv("model", json!({"stringValue": "m"}));
        let ts = kv(
            "event.timestamp",
            json!({"stringValue": "2026-11-14T07:32:15Z"}),
        );
        let req = kv("request_id", json!({"stringValue": "r"}));
        assert!(api_request_records(&base(vec![ts.clone(), req.clone()])).is_empty());
        assert!(api_request_records(&base(vec![model.clone(), req])).is_empty());
        assert!(api_request_records(&base(vec![model.clone(), ts.clone()])).is_empty());
        assert!(api_request_records(&json!({"nope": 1})).is_empty());
    }

    fn http(req: &str) -> Request {
        read_request(&mut Cursor::new(req.as_bytes().to_vec()))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn reads_content_length_and_chunked_bodies() {
        let r = http("POST /v1/logs HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}");
        assert_eq!(
            (r.method.as_str(), r.path.as_str(), r.body.as_slice()),
            ("POST", "/v1/logs", &b"{}"[..])
        );
        assert!(r.keep_alive);
        let r = http("POST /v1/logs HTTP/1.1\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n");
        assert_eq!(r.body, b"abcde");
        assert!(!r.keep_alive);
    }

    #[test]
    fn rejects_oversized_and_malformed_requests() {
        let big = format!(
            "POST /v1/logs HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY_BYTES + 1
        );
        assert_eq!(
            read_request(&mut Cursor::new(big.into_bytes())).unwrap_err(),
            413
        );
        assert_eq!(
            read_request(&mut Cursor::new(
                b"POST /v1/logs HTTP/1.1\r\nContent-Length: x\r\n\r\n".to_vec()
            ))
            .unwrap_err(),
            400
        );
        assert!(read_request(&mut Cursor::new(Vec::new()))
            .unwrap()
            .is_none());
    }

    #[test]
    fn handle_routes_and_writes_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let body = api_request_payload().to_string();
        let req = |path: &str, ct: &str, enc: &str, body: &str| Request {
            method: "POST".into(),
            path: path.into(),
            content_type: ct.into(),
            content_encoding: enc.into(),
            keep_alive: true,
            body: body.as_bytes().to_vec(),
        };
        assert_eq!(
            handle(
                &req("/v1/logs", "application/x-protobuf", "", &body),
                dir.path()
            )
            .0,
            415
        );
        assert_eq!(
            handle(
                &req("/v1/logs", "application/json", "gzip", &body),
                dir.path()
            )
            .0,
            415
        );
        assert_eq!(
            handle(&req("/other", "application/json", "", &body), dir.path()).0,
            404
        );
        assert_eq!(
            handle(
                &req("/v1/metrics", "application/json", "", "{}"),
                dir.path()
            )
            .0,
            200
        );
        assert_eq!(
            handle(
                &req("/v1/logs", "application/json", "", "not json"),
                dir.path()
            )
            .0,
            400
        );
        assert_eq!(
            handle(
                &req("/v1/logs", "application/json; charset=utf-8", "", &body),
                dir.path()
            )
            .0,
            200
        );
        let mut get = req("/v1/logs", "application/json", "", "");
        get.method = "GET".into();
        assert_eq!(handle(&get, dir.path()).0, 405);

        let written = fs::read_to_string(dir.path().join("claude-otel-2026-11-14.jsonl")).unwrap();
        assert_eq!(written.lines().count(), 1);
        assert!(status().events_received >= 1);
    }

    #[test]
    fn serves_keep_alive_requests_over_a_real_socket() {
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let path = dir.path().to_path_buf();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, &path);
        });
        let body = api_request_payload().to_string();
        let mut client = TcpStream::connect(addr).unwrap();
        let mut reader = BufReader::new(client.try_clone().unwrap());
        for close in [false, true] {
            let conn = if close { "close" } else { "keep-alive" };
            write!(
                client,
                "POST /v1/logs HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nConnection: {conn}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            let mut status_line = String::new();
            reader.read_line(&mut status_line).unwrap();
            assert!(status_line.starts_with("HTTP/1.1 200"), "{status_line}");
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut resp = vec![0u8; len];
            reader.read_exact(&mut resp).unwrap();
        }
        server.join().unwrap();
        let written = fs::read_to_string(dir.path().join("claude-otel-2026-11-14.jsonl")).unwrap();
        assert_eq!(written.lines().count(), 2);
    }
}
