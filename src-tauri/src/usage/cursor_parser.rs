use chrono::{DateTime, Local, NaiveDate, TimeZone};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use super::parser::{
    modified_since, path_to_string, push_sample_path, ParsedEntry, ProviderReadDebug,
    SessionParseResult,
};

/// Windows: CREATE_NO_WINDOW flag prevents a console window from flashing.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ─────────────────────────────────────────────────────────────────────────────
// Cursor helpers (local probe + remote API)
//
// Three remote auth paths are supported, dispatched from the active credential:
//
//   • Admin API key (`key_…` prefix) → Basic auth against
//     `https://api.cursor.com/teams/filtered-usage-events`. Enterprise admins
//     only.
//   • Dashboard session token (`WorkosCursorSessionToken` cookie value
//     manually pasted by the user) → cookie auth against
//     `https://cursor.com/api/dashboard/get-filtered-usage-events`.
//     Works for individual Pro/Pro+/Ultra users.
//   • IDE bearer token (auto-detected from `cursorAuth/accessToken` in
//     Cursor IDE's `state.vscdb`) → Bearer auth against
//     `https://api2.cursor.sh/aiserver.v1.DashboardService/GetFilteredUsageEvents`
//     (Connect-Web protocol). This is the **zero-config** path: as long as
//     the user is signed into Cursor IDE on the same machine, no manual
//     paste is required. Cursor IDE refreshes the access token on its own
//     schedule; we just re-read state.vscdb before each remote call.
// ─────────────────────────────────────────────────────────────────────────────

const CURSOR_API_MAX_PAGES: usize = 20;
const CURSOR_API_PAGE_SIZE: usize = 100;
const CURSOR_OFFICIAL_API_BASE_URL: &str = "https://api.cursor.com";
const CURSOR_DASHBOARD_API_BASE_URL: &str = "https://cursor.com";
const CURSOR_IDE_API_BASE_URL: &str = "https://api2.cursor.sh";
const CURSOR_API_KEY_ENV: &str = "CURSOR_API_KEY";
const CURSOR_SESSION_TOKEN_ENV: &str = "CURSOR_SESSION_TOKEN";
const CURSOR_USER_DIR_ENV: &str = "CURSOR_USER_DIR";
const CURSOR_IDE_ACCESS_TOKEN_KEY: &str = "cursorAuth/accessToken";

static CURSOR_LAST_WARNING: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static CURSOR_SECRET_OVERRIDE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static CURSOR_IDE_TOKEN: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static CURSOR_STORAGE_BACKEND: OnceLock<Mutex<crate::secrets::StorageBackend>> = OnceLock::new();

fn cursor_warning_cell() -> &'static Mutex<Option<String>> {
    CURSOR_LAST_WARNING.get_or_init(|| Mutex::new(None))
}

fn cursor_secret_override_cell() -> &'static Mutex<Option<String>> {
    CURSOR_SECRET_OVERRIDE.get_or_init(|| Mutex::new(None))
}

fn cursor_ide_token_cell() -> &'static Mutex<Option<String>> {
    CURSOR_IDE_TOKEN.get_or_init(|| Mutex::new(None))
}

fn cursor_storage_backend_cell() -> &'static Mutex<crate::secrets::StorageBackend> {
    CURSOR_STORAGE_BACKEND.get_or_init(|| Mutex::new(crate::secrets::StorageBackend::None))
}

fn normalize_optional_secret(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn set_cursor_auth_config(
    api_key: Option<String>,
    backend: crate::secrets::StorageBackend,
) -> CursorAuthStatus {
    if let Ok(mut guard) = cursor_secret_override_cell().lock() {
        *guard = normalize_optional_secret(api_key);
    }
    if let Ok(mut guard) = cursor_storage_backend_cell().lock() {
        *guard = backend;
    }
    set_cursor_warning(None);
    cursor_auth_status()
}

pub(crate) fn set_cursor_warning(message: Option<String>) {
    if let Ok(mut guard) = cursor_warning_cell().lock() {
        *guard = message;
    }
}

pub(crate) fn cursor_last_warning() -> Option<String> {
    cursor_warning_cell()
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorAuthStatus {
    pub source: String,
    pub configured: bool,
    pub message: String,
    pub last_warning: Option<String>,
    pub storage_backend: crate::secrets::StorageBackend,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CursorAuth {
    Admin(String),
    Dashboard(String),
    IdeBearer(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorAuthKind {
    Admin,
    Dashboard,
    IdeBearer,
}

impl CursorAuth {
    pub(crate) fn kind(&self) -> CursorAuthKind {
        match self {
            CursorAuth::Admin(_) => CursorAuthKind::Admin,
            CursorAuth::Dashboard(_) => CursorAuthKind::Dashboard,
            CursorAuth::IdeBearer(_) => CursorAuthKind::IdeBearer,
        }
    }
}

pub(crate) fn classify_cursor_secret(secret: &str) -> Option<CursorAuth> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("key_") {
        Some(CursorAuth::Admin(trimmed.to_string()))
    } else {
        Some(CursorAuth::Dashboard(trimmed.to_string()))
    }
}

pub(crate) fn choose_cursor_auth(
    api_key_env: Option<&str>,
    session_token_env: Option<&str>,
    secret_override: Option<&str>,
    ide_token: Option<&str>,
) -> Option<CursorAuth> {
    secret_override
        .and_then(classify_cursor_secret)
        .or_else(|| session_token_env.and_then(classify_cursor_secret))
        .or_else(|| api_key_env.and_then(classify_cursor_secret))
        .or_else(|| {
            let trimmed = ide_token?.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(CursorAuth::IdeBearer(trimmed.to_string()))
            }
        })
}

pub(crate) fn cursor_auth_status() -> CursorAuthStatus {
    let last_warning = cursor_last_warning();
    let storage_backend = cursor_storage_backend_cell()
        .lock()
        .map(|guard| *guard)
        .unwrap_or_default();
    match resolve_cursor_auth() {
        Some(CursorAuth::Admin(_)) => CursorAuthStatus {
            source: String::from("admin_api_key"),
            configured: true,
            message: String::from(
                "Cursor Enterprise admin API key is configured. Detailed events come from api.cursor.com.",
            ),
            last_warning,
            storage_backend,
        },
        Some(CursorAuth::Dashboard(_)) => CursorAuthStatus {
            source: String::from("dashboard_session"),
            configured: true,
            message: String::from(
                "Cursor dashboard session token is configured. Detailed events come from cursor.com/api/dashboard.",
            ),
            last_warning,
            storage_backend,
        },
        Some(CursorAuth::IdeBearer(_)) => CursorAuthStatus {
            source: String::from("ide_bearer"),
            configured: true,
            message: String::from(
                "Auto-detected from your Cursor IDE login. Detailed events come from api2.cursor.sh; the access token refreshes silently as long as Cursor IDE stays signed in.",
            ),
            last_warning,
            storage_backend,
        },
        None => CursorAuthStatus {
            source: String::from("missing"),
            configured: false,
            message: String::from(
                "No Cursor credentials available. Sign into Cursor IDE on this machine for zero-config access, or paste a dashboard session token / Enterprise admin API key.",
            ),
            last_warning,
            storage_backend,
        },
    }
}

fn parse_u64_value(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(num)) => num.as_u64().unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse::<u64>().unwrap_or(0),
        _ => 0,
    }
}

fn parse_cursor_timestamp(value: Option<&Value>) -> Option<DateTime<Local>> {
    match value {
        Some(Value::Number(num)) => {
            let raw = num.as_i64()?;
            let (seconds, nanos) = if raw > 10_000_000_000 {
                (raw / 1_000, ((raw % 1_000) * 1_000_000) as u32)
            } else {
                (raw, 0)
            };
            let utc = chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, nanos)?;
            Some(utc.with_timezone(&Local))
        }
        Some(Value::String(text)) => {
            if let Ok(raw) = text.trim().parse::<i64>() {
                return parse_cursor_timestamp(Some(&Value::Number(raw.into())));
            }
            chrono::DateTime::parse_from_rfc3339(text)
                .map(|dt| dt.with_timezone(&Local))
                .ok()
        }
        _ => None,
    }
}

fn parse_cursor_usage_from_object(
    map: &serde_json::Map<String, Value>,
) -> Option<(u64, u64, u64, u64)> {
    let input = parse_u64_value(
        map.get("inputTokens")
            .or_else(|| map.get("input_tokens"))
            .or_else(|| map.get("promptTokens"))
            .or_else(|| map.get("prompt_tokens")),
    );
    let output = parse_u64_value(
        map.get("outputTokens")
            .or_else(|| map.get("output_tokens"))
            .or_else(|| map.get("completionTokens"))
            .or_else(|| map.get("completion_tokens")),
    );
    let cache_read = parse_u64_value(
        map.get("cacheReadTokens")
            .or_else(|| map.get("cache_read_tokens"))
            .or_else(|| map.get("cached_input_tokens")),
    );
    let cache_write = parse_u64_value(
        map.get("cacheWriteTokens")
            .or_else(|| map.get("cache_write_tokens"))
            .or_else(|| map.get("cache_creation_input_tokens")),
    );

    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        None
    } else {
        Some((input, output, cache_read, cache_write))
    }
}

fn collect_cursor_entries_from_value(
    value: &Value,
    session_key: &str,
    entries: &mut Vec<ParsedEntry>,
) {
    match value {
        Value::Object(map) => {
            let usage_map = map
                .get("tokenUsage")
                .and_then(Value::as_object)
                .or_else(|| map.get("usage").and_then(Value::as_object))
                .or(Some(map));

            if let Some(tokens_obj) = usage_map {
                if let Some((input, output, cache_read, cache_write)) =
                    parse_cursor_usage_from_object(tokens_obj)
                {
                    let timestamp = parse_cursor_timestamp(
                        map.get("timestamp")
                            .or_else(|| map.get("time"))
                            .or_else(|| map.get("createdAt"))
                            .or_else(|| map.get("created_at")),
                    )
                    .unwrap_or_else(Local::now);
                    let model = map
                        .get("model")
                        .or_else(|| map.get("modelName"))
                        .or_else(|| map.get("model_name"))
                        .and_then(Value::as_str)
                        .unwrap_or("cursor-unknown")
                        .to_string();
                    let unique_hash = map
                        .get("id")
                        .or_else(|| map.get("eventId"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string);
                    entries.push(ParsedEntry {
                        timestamp,
                        model,
                        input_tokens: input,
                        output_tokens: output,
                        cache_creation_5m_tokens: 0,
                        cache_creation_1h_tokens: cache_write,
                        cache_read_tokens: cache_read,
                        web_search_requests: 0,
                        unique_hash,
                        session_key: session_key.to_string(),
                        agent_scope: crate::stats::subagent::AgentScope::Main,
                    });
                }
            }

            for (key, nested) in map {
                if key == "tokenUsage" || key == "usage" {
                    continue;
                }
                collect_cursor_entries_from_value(nested, session_key, entries);
            }
        }
        Value::Array(values) => {
            for nested in values {
                collect_cursor_entries_from_value(nested, session_key, entries);
            }
        }
        _ => {}
    }
}

pub(crate) fn parse_cursor_session_file(path: &Path) -> SessionParseResult {
    tracing::debug!(path = %path.display(), "opening file (cursor session)");
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(
                path = %path_to_string(path),
                error = %error,
                "Failed to open Cursor chat session file"
            );
            return (Vec::new(), Vec::new(), 0, false);
        }
    };
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    let mut lines_read = 0usize;
    for line in reader.lines() {
        lines_read += 1;
        match line {
            Ok(content) => lines.push(content),
            Err(error) => {
                tracing::warn!(
                    path = %path_to_string(path),
                    error = %error,
                    "Failed to read a line from Cursor chat session file"
                );
            }
        }
    }
    let content = lines.join("\n");
    let session_key = format!("cursor-file:{}", path_to_string(path));

    let mut entries = Vec::new();
    if let Ok(value) = serde_json::from_str::<Value>(&content) {
        collect_cursor_entries_from_value(&value, &session_key, &mut entries);
    } else {
        let mut parsed_lines = 0usize;
        for line in lines {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                parsed_lines += 1;
                collect_cursor_entries_from_value(&value, &session_key, &mut entries);
            }
        }
        if parsed_lines == 0 && lines_read > 0 {
            tracing::warn!(
                path = %path_to_string(path),
                lines_read,
                "Cursor chat session file could not be parsed as JSON"
            );
        }
    }

    (entries, Vec::new(), lines_read, true)
}

pub(crate) fn glob_cursor_chat_session_files(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if !dir.exists() {
        return results;
    }
    tracing::debug!(path = %dir.display(), "read_dir (glob_cursor_chat_session_files)");
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(error) => {
            tracing::warn!(
                path = %path_to_string(dir),
                error = %error,
                "Failed to read Cursor workspace storage directory"
            );
            return results;
        }
    };
    for entry in rd.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            tracing::debug!(path = %entry.path().display(), "skipping symlink");
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            if path.file_name().is_some_and(|name| name == "chatSessions") {
                let chat_rd = match fs::read_dir(&path) {
                    Ok(chat_rd) => chat_rd,
                    Err(error) => {
                        tracing::warn!(
                            path = %path_to_string(&path),
                            error = %error,
                            "Failed to read Cursor chatSessions directory"
                        );
                        continue;
                    }
                };
                for chat_entry in chat_rd.flatten() {
                    let Ok(chat_ft) = chat_entry.file_type() else {
                        continue;
                    };
                    if chat_ft.is_symlink() || !chat_ft.is_file() {
                        continue;
                    }
                    let chat_path = chat_entry.path();
                    if chat_path.extension().is_some_and(|ext| ext == "json") {
                        results.push(chat_path);
                    }
                }
            } else {
                results.extend(glob_cursor_chat_session_files(&path));
            }
        }
    }
    results.sort();
    results
}

pub(crate) fn cursor_global_state_path_from_env() -> Option<PathBuf> {
    let raw = std::env::var(CURSOR_USER_DIR_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if path.file_name().is_some_and(|name| name == "state.vscdb") {
        return Some(path);
    }
    if path.file_name().is_some_and(|name| name == "globalStorage") {
        return Some(path.join("state.vscdb"));
    }
    if path.file_name().is_some_and(|name| name == "User") {
        return Some(path.join("globalStorage").join("state.vscdb"));
    }
    Some(path.join("User").join("globalStorage").join("state.vscdb"))
}

pub(crate) fn read_cursor_state_value_from_sqlite3(
    db_path: &Path,
    key: &str,
) -> Result<Option<String>, String> {
    if !db_path.is_file() {
        return Err(format!(
            "Cursor state DB not found at {}",
            path_to_string(db_path)
        ));
    }
    let query = format!("SELECT value FROM ItemTable WHERE key = '{key}' LIMIT 1;");
    let mut cmd = Command::new("sqlite3");
    cmd.arg(db_path).arg(&query);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().map_err(|e| {
        format!(
            "Failed to run sqlite3 for Cursor state DB {}: {e}",
            path_to_string(db_path)
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "sqlite3 failed reading Cursor state DB {} with status {}{}",
            path_to_string(db_path),
            output.status,
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", stderr.trim())
            }
        ));
    }
    let text = String::from_utf8(output.stdout).map_err(|e| {
        format!(
            "Cursor state DB token output was not valid UTF-8 at {}: {e}",
            path_to_string(db_path)
        )
    })?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

pub(crate) fn read_cursor_cached_email() -> Option<String> {
    let path = cursor_global_state_path_from_env()
        .or_else(crate::paths::cursor_global_state_vscdb_default)?;
    read_cursor_state_value_from_sqlite3(&path, "cursorAuth/cachedEmail")
        .ok()
        .flatten()
        .map(|email| email.trim().to_string())
        .filter(|email| email.contains('@'))
}

pub(crate) fn resolve_cursor_auth() -> Option<CursorAuth> {
    let api_key_env = std::env::var(CURSOR_API_KEY_ENV).ok();
    let session_token_env = std::env::var(CURSOR_SESSION_TOKEN_ENV).ok();
    let secret_override = cursor_secret_override_cell()
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    let ide_token = cursor_ide_token_cell()
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    choose_cursor_auth(
        api_key_env.as_deref(),
        session_token_env.as_deref(),
        secret_override.as_deref(),
        ide_token.as_deref(),
    )
}

pub(crate) fn read_cursor_ide_access_token() -> Option<String> {
    let path = cursor_global_state_path_from_env()
        .or_else(crate::paths::cursor_global_state_vscdb_default)?;
    let raw = read_cursor_state_value_from_sqlite3(&path, CURSOR_IDE_ACCESS_TOKEN_KEY)
        .ok()
        .flatten()?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn refresh_cursor_ide_token() {
    let was_primed = cursor_ide_token_cell()
        .lock()
        .ok()
        .map(|g| g.is_some())
        .unwrap_or(false);
    if !was_primed {
        return;
    }
    if let Some(token) = read_cursor_ide_access_token() {
        if let Ok(mut guard) = cursor_ide_token_cell().lock() {
            *guard = Some(token);
        }
    }
}

pub(crate) fn prime_ide_access_token() -> bool {
    if let Some(token) = read_cursor_ide_access_token() {
        if let Ok(mut guard) = cursor_ide_token_cell().lock() {
            *guard = Some(token);
            return true;
        }
    }
    false
}

fn cursor_api_time_range_ms(since: Option<NaiveDate>) -> (String, String) {
    let now_local = Local::now();
    let start_local = since
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(|dt| Local.from_local_datetime(&dt).single())
        .unwrap_or_else(|| now_local - chrono::Duration::hours(24));
    (
        start_local.timestamp_millis().to_string(),
        now_local.timestamp_millis().to_string(),
    )
}

fn parsed_entry_from_cursor_event(
    map: &serde_json::Map<String, Value>,
    session_key: &str,
    fallback_hash: Option<String>,
) -> Option<ParsedEntry> {
    let usage = map.get("tokenUsage").and_then(Value::as_object);
    let input = parse_u64_value(usage.and_then(|u| u.get("inputTokens")));
    let output = parse_u64_value(usage.and_then(|u| u.get("outputTokens")));
    let cache_read = parse_u64_value(usage.and_then(|u| u.get("cacheReadTokens")));
    let cache_write = parse_u64_value(usage.and_then(|u| u.get("cacheWriteTokens")));
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        return None;
    }

    let timestamp = parse_cursor_timestamp(map.get("timestamp")).unwrap_or_else(Local::now);
    let model = map
        .get("model")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("cursor-unknown")
        .to_string();
    let unique_hash = map
        .get("id")
        .or_else(|| map.get("eventId"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or(fallback_hash);
    Some(ParsedEntry {
        timestamp,
        model,
        input_tokens: input,
        output_tokens: output,
        cache_creation_5m_tokens: 0,
        cache_creation_1h_tokens: cache_write,
        cache_read_tokens: cache_read,
        web_search_requests: 0,
        unique_hash,
        session_key: session_key.to_string(),
        agent_scope: crate::stats::subagent::AgentScope::Main,
    })
}

pub(crate) fn parse_cursor_official_usage_events(
    data: &Value,
    since: Option<NaiveDate>,
    session_key: &str,
) -> Result<Vec<ParsedEntry>, String> {
    let rows = data
        .get("usageEvents")
        .or_else(|| data.get("usageEventsDisplay"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!(
                "Cursor API payload missing usageEvents/usageEventsDisplay array (session_key={session_key})"
            )
        })?;
    let mut entries = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        let Some(map) = row.as_object() else {
            continue;
        };
        let fallback_hash = map
            .get("timestamp")
            .and_then(|value| match value {
                Value::String(text) => Some(text.clone()),
                Value::Number(num) => Some(num.to_string()),
                _ => None,
            })
            .map(|timestamp| format!("{session_key}:{timestamp}:{idx}"));
        let Some(entry) = parsed_entry_from_cursor_event(map, session_key, fallback_hash) else {
            continue;
        };
        if since.is_some_and(|since_date| entry.timestamp.date_naive() < since_date) {
            continue;
        }
        entries.push(entry);
    }
    Ok(entries)
}

pub(crate) fn cursor_response_has_next_page(data: &Value, page: usize, page_size: usize) -> bool {
    if let Some(has_next) = data
        .get("pagination")
        .and_then(|p| p.get("hasNextPage"))
        .and_then(Value::as_bool)
    {
        return has_next;
    }
    let total = data.get("totalUsageEventsCount").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
    });
    match total {
        Some(total) => (page as u64).saturating_mul(page_size as u64) < total,
        None => false,
    }
}

pub(crate) fn cursor_request_url(auth: &CursorAuth) -> String {
    match auth {
        CursorAuth::Admin(_) => {
            format!("{CURSOR_OFFICIAL_API_BASE_URL}/teams/filtered-usage-events")
        }
        CursorAuth::Dashboard(_) => {
            format!("{CURSOR_DASHBOARD_API_BASE_URL}/api/dashboard/get-filtered-usage-events")
        }
        CursorAuth::IdeBearer(_) => {
            format!("{CURSOR_IDE_API_BASE_URL}/aiserver.v1.DashboardService/GetFilteredUsageEvents")
        }
    }
}

fn apply_cursor_auth(
    request: reqwest::blocking::RequestBuilder,
    auth: &CursorAuth,
) -> reqwest::blocking::RequestBuilder {
    match auth {
        CursorAuth::Admin(api_key) => request.basic_auth(api_key, Some("")),
        CursorAuth::Dashboard(token) => request.header(
            reqwest::header::COOKIE,
            format!("WorkosCursorSessionToken={token}"),
        ),
        CursorAuth::IdeBearer(token) => request.bearer_auth(token),
    }
}

pub(crate) fn cursor_session_key_for(auth_kind: CursorAuthKind) -> &'static str {
    match auth_kind {
        CursorAuthKind::Admin => "cursor-admin",
        CursorAuthKind::Dashboard => "cursor-dashboard",
        CursorAuthKind::IdeBearer => "cursor-ide",
    }
}

fn cursor_auth_label(auth_kind: CursorAuthKind) -> &'static str {
    match auth_kind {
        CursorAuthKind::Admin => "admin",
        CursorAuthKind::Dashboard => "dashboard",
        CursorAuthKind::IdeBearer => "ide",
    }
}

fn build_cursor_usage_request_payload(
    auth: &CursorAuth,
    page: usize,
    since: Option<NaiveDate>,
) -> Value {
    let (start_ms, end_ms) = cursor_api_time_range_ms(since);
    let mut payload = match auth {
        CursorAuth::IdeBearer(_) => serde_json::json!({
            "startDate": start_ms,
            "endDate": end_ms,
            "page": page,
            "pageSize": CURSOR_API_PAGE_SIZE,
        }),
        _ => serde_json::json!({
            "startDate": start_ms.parse::<i64>().unwrap_or_default(),
            "endDate": end_ms.parse::<i64>().unwrap_or_default(),
            "page": page,
            "pageSize": CURSOR_API_PAGE_SIZE,
        }),
    };
    if matches!(auth, CursorAuth::Admin(_)) {
        if let Some(email) = read_cursor_cached_email() {
            payload["email"] = Value::String(email);
        }
    }
    payload
}

fn fetch_cursor_usage_events(
    auth: &CursorAuth,
    since: Option<NaiveDate>,
) -> Result<Vec<ParsedEntry>, String> {
    let auth_kind = auth.kind();
    let auth_label = cursor_auth_label(auth_kind);
    let session_key = cursor_session_key_for(auth_kind);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| {
            let message = format!("Failed to build Cursor HTTP client ({auth_label}): {e}");
            tracing::error!(error = %message, "Cursor HTTP client initialization failed");
            message
        })?;

    let url = cursor_request_url(auth);
    let mut page = 1usize;
    let mut entries = Vec::new();
    loop {
        let payload = build_cursor_usage_request_payload(auth, page, since);
        let mut req = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");
        if auth_kind == CursorAuthKind::IdeBearer {
            req = req.header("Connect-Protocol-Version", "1");
        }
        let request = apply_cursor_auth(req.json(&payload), auth);
        let response = request.send().map_err(|e| {
            let message = format!("Cursor {auth_label} API request failed: {e}");
            tracing::warn!(page, auth = auth_label, error = %message, "Cursor request failed");
            message
        })?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            tracing::warn!(
                page,
                auth = auth_label,
                status = %response.status(),
                "Cursor API rejected the configured credentials"
            );
            return Err(match auth_kind {
                CursorAuthKind::Admin => format!(
                    "Cursor admin API rejected the configured key with HTTP {}.",
                    response.status()
                ),
                CursorAuthKind::Dashboard => format!(
                    "Cursor dashboard rejected the configured session token with HTTP {}. The token may have expired — re-copy `WorkosCursorSessionToken` from cursor.com cookies.",
                    response.status()
                ),
                CursorAuthKind::IdeBearer => format!(
                    "Cursor api2 rejected the auto-detected IDE token with HTTP {}. Sign back into Cursor IDE on this machine to refresh the token (Cursor IDE will re-write `cursorAuth/accessToken` in state.vscdb).",
                    response.status()
                ),
            });
        }
        if !response.status().is_success() {
            tracing::warn!(
                page,
                auth = auth_label,
                status = %response.status(),
                "Cursor API returned a non-success status"
            );
            return Err(format!(
                "Cursor {auth_label} API returned HTTP {}",
                response.status()
            ));
        }

        let data: Value = response.json().map_err(|e| {
            let message = format!("Cursor {auth_label} API payload parse failed: {e}");
            tracing::warn!(page, auth = auth_label, error = %message, "payload parse failed");
            message
        })?;
        let mut next_entries = parse_cursor_official_usage_events(&data, since, session_key)?;
        entries.append(&mut next_entries);
        let has_next = cursor_response_has_next_page(&data, page, CURSOR_API_PAGE_SIZE);
        if !has_next || page >= CURSOR_API_MAX_PAGES {
            break;
        }
        page += 1;
    }

    tracing::info!(
        since = ?since,
        auth = auth_label,
        entries = entries.len(),
        "Loaded Cursor token usage entries from remote API"
    );
    Ok(entries)
}

pub(crate) fn fetch_cursor_remote_entries(
    since: Option<NaiveDate>,
) -> Result<Option<Vec<ParsedEntry>>, String> {
    refresh_cursor_ide_token();
    let Some(auth) = resolve_cursor_auth() else {
        tracing::warn!(
            "Cursor remote auth not configured (no admin key, dashboard token, or IDE access token)"
        );
        return Ok(None);
    };
    let result = std::thread::scope(|s| {
        s.spawn(|| fetch_cursor_usage_events(&auth, since))
            .join()
            .unwrap_or_else(|_| Err(String::from("Cursor fetch thread panicked")))
    });
    result.map(Some)
}

// ─────────────────────────────────────────────────────────────────────────────
// Cursor local entry loading (used by UsageParser)
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) fn load_cursor_local_entries(
    root_dir: &Path,
    since: Option<NaiveDate>,
) -> (Vec<ParsedEntry>, ProviderReadDebug) {
    let root_exists = root_dir.exists();
    let mut report = ProviderReadDebug {
        provider: String::from("cursor"),
        root_dir: path_to_string(root_dir),
        root_exists,
        since: since.map(|d| d.format("%Y-%m-%d").to_string()),
        strategy: String::from("workspace-chat-json-token-probe"),
        ..ProviderReadDebug::default()
    };
    if !root_exists {
        tracing::warn!(
            root_dir = %report.root_dir,
            "Cursor workspace storage root does not exist"
        );
        return (Vec::new(), report);
    }

    let files = glob_cursor_chat_session_files(root_dir);
    report.discovered_paths = files.len();
    if files.is_empty() {
        tracing::warn!(
            root_dir = %report.root_dir,
            "No Cursor chat session files were discovered"
        );
    }
    let mut entries = Vec::new();
    for path in files {
        report.attempted_paths += 1;
        push_sample_path(&mut report.sample_paths, &path);
        if let Some(since_date) = since {
            if !modified_since(&path, since_date) {
                report.skipped_paths += 1;
                report.skipped_by_mtime += 1;
                push_sample_path(&mut report.sample_skipped_paths, &path);
                continue;
            }
        }

        let (parsed_entries, _change_events, lines_read, opened) = parse_cursor_session_file(&path);
        report.lines_read += lines_read;
        if opened {
            report.opened_paths += 1;
        } else {
            report.failed_paths += 1;
            continue;
        }
        for entry in parsed_entries {
            if since.is_some_and(|since_date| entry.timestamp.date_naive() < since_date) {
                continue;
            }
            entries.push(entry);
        }
    }

    entries.sort_by_key(|entry| entry.timestamp);
    report.emitted_entries = entries.len();
    if entries.is_empty() && report.opened_paths > 0 {
        tracing::warn!(
            root_dir = %report.root_dir,
            discovered_paths = report.discovered_paths,
            opened_paths = report.opened_paths,
            lines_read = report.lines_read,
            "Cursor chat session files were readable but contained no token usage entries"
        );
    }
    (entries, report)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_cursor_secret_admin_key() {
        let auth = classify_cursor_secret("key_abc123").unwrap();
        assert!(matches!(auth, CursorAuth::Admin(_)));
    }

    #[test]
    fn classify_cursor_secret_dashboard_token() {
        let auth = classify_cursor_secret("user_abc::jwt.token").unwrap();
        assert!(matches!(auth, CursorAuth::Dashboard(_)));
    }

    #[test]
    fn classify_cursor_secret_empty_is_none() {
        assert!(classify_cursor_secret("").is_none());
        assert!(classify_cursor_secret("   ").is_none());
    }

    #[test]
    fn choose_auth_priority_override_wins() {
        let result = choose_cursor_auth(
            Some("key_env"),
            Some("session_env"),
            Some("key_override"),
            Some("ide_token"),
        );
        assert!(matches!(result, Some(CursorAuth::Admin(_))));
    }

    #[test]
    fn choose_auth_session_env_beats_api_key_env() {
        let result = choose_cursor_auth(Some("key_env"), Some("session_env"), None, None);
        assert!(matches!(result, Some(CursorAuth::Dashboard(_))));
    }

    #[test]
    fn choose_auth_api_key_env_beats_ide() {
        let result = choose_cursor_auth(Some("key_env"), None, None, Some("ide_token"));
        assert!(matches!(result, Some(CursorAuth::Admin(_))));
    }

    #[test]
    fn choose_auth_ide_token_is_ide_bearer() {
        let result = choose_cursor_auth(None, None, None, Some("ide_token"));
        assert!(matches!(result, Some(CursorAuth::IdeBearer(_))));
    }

    #[test]
    fn choose_auth_all_none() {
        assert!(choose_cursor_auth(None, None, None, None).is_none());
    }
}
