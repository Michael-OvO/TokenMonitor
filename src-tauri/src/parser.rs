use crate::models::{
    ActiveBlock, ChartBucket, ChartSegment, ModelSummary, UsagePayload,
};
use chrono::{DateTime, Local, NaiveDate, Timelike};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

// ─────────────────────────────────────────────────────────────────────────────
// Parsed entry (shared between Claude and Codex)
// ─────────────────────────────────────────────────────────────────────────────

pub struct ParsedEntry {
    pub timestamp: DateTime<Local>,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Claude JSONL serde types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ClaudeJsonlEntry {
    #[serde(rename = "type", default)]
    entry_type: String,
    #[serde(default)]
    timestamp: String,
    message: Option<ClaudeJsonlMessage>,
}

#[derive(Deserialize)]
struct ClaudeJsonlMessage {
    model: Option<String>,
    usage: Option<ClaudeJsonlUsage>,
}

#[derive(Deserialize)]
struct ClaudeJsonlUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Codex JSONL serde types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CodexJsonlEntry {
    #[serde(rename = "type", default)]
    entry_type: String,
    #[serde(default)]
    timestamp: String,
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    #[serde(rename = "type", default)]
    payload_type: String,
    info: Option<CodexTokenInfo>,
}

#[derive(Deserialize)]
struct CodexTokenInfo {
    last_token_usage: Option<CodexTokenUsage>,
}

#[derive(Deserialize)]
struct CodexTokenUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    reasoning_output_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
}

// ─────────────────────────────────────────────────────────────────────────────
// File scanning helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Recursively find all `.jsonl` files under `dir`.
pub fn glob_jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if !dir.exists() {
        return results;
    }
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return results,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let mut sub = glob_jsonl_files(&path);
            results.append(&mut sub);
        } else if path.extension().map_or(false, |e| e == "jsonl") {
            results.push(path);
        }
    }
    results
}

/// Parse a `since` string in `YYYYMMDD` format into a `NaiveDate`.
pub fn parse_since_date(since: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(since, "%Y%m%d").ok()
}

/// Extract a model name from a raw JSON line by searching for `"model":"<value>"`.
pub fn extract_model_from_line(line: &str) -> Option<String> {
    let marker = "\"model\":\"";
    let start = line.find(marker)? + marker.len();
    let end = line[start..].find('"')? + start;
    Some(line[start..end].to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Model normalisation helper
// ─────────────────────────────────────────────────────────────────────────────

fn normalize_model(raw: &str) -> (&str, &str) {
    if raw.starts_with("gpt")
        || raw.starts_with("o1")
        || raw.starts_with("o3")
        || raw.starts_with("o4")
        || raw.contains("codex")
    {
        crate::models::normalize_codex_model(raw)
    } else {
        crate::models::normalize_claude_model(raw)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Provider-specific readers
// ─────────────────────────────────────────────────────────────────────────────

/// Check if a file was modified on or after the given date.
fn modified_since(path: &Path, since: NaiveDate) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| {
            let dt: chrono::DateTime<Local> = t.into();
            dt.date_naive() >= since
        })
        .unwrap_or(true) // if we can't read metadata, include the file
}

/// Read all Claude assistant entries from JSONL files under `projects_dir`,
/// optionally filtering to entries on or after `since`.
pub fn read_claude_entries(projects_dir: &Path, since: Option<NaiveDate>) -> Vec<ParsedEntry> {
    let mut entries = Vec::new();
    let files = glob_jsonl_files(projects_dir);

    for path in files {
        // Skip files that haven't been modified since the since date.
        // This avoids reading hundreds of old files for short time periods.
        if let Some(since_date) = since {
            if !modified_since(&path, since_date) {
                continue;
            }
        }

        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in contents.lines() {
            let entry: ClaudeJsonlEntry = match serde_json::from_str(line) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.entry_type != "assistant" {
                continue;
            }
            let msg = match &entry.message {
                Some(m) => m,
                None => continue,
            };
            let usage = match &msg.usage {
                Some(u) => u,
                None => continue,
            };
            let model = match &msg.model {
                Some(m) => m.clone(),
                None => continue,
            };
            let ts = match chrono::DateTime::parse_from_rfc3339(&entry.timestamp) {
                Ok(dt) => dt.with_timezone(&Local),
                Err(_) => continue,
            };
            if let Some(since_date) = since {
                if ts.date_naive() < since_date {
                    continue;
                }
            }
            entries.push(ParsedEntry {
                timestamp: ts,
                model,
                input_tokens: usage.input_tokens.unwrap_or(0),
                output_tokens: usage.output_tokens.unwrap_or(0),
                cache_creation_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
                cache_read_tokens: usage.cache_read_input_tokens.unwrap_or(0),
            });
        }
    }
    entries
}

/// Parse a single Codex session JSONL file.
/// Codex `last_token_usage` is cumulative — only the FINAL `token_count` event
/// per file is used.  Model name is extracted via regex-free string search.
fn parse_codex_session_file(path: &Path) -> Option<ParsedEntry> {
    let contents = fs::read_to_string(path).ok()?;
    let mut final_usage: Option<(DateTime<Local>, String, u64, u64, u64)> = None;
    let mut session_model = String::from("codex-mini-latest");

    for line in contents.lines() {
        // Try to extract a model name anywhere in this line
        if line.contains("\"model\":\"") {
            if let Some(m) = extract_model_from_line(line) {
                if !m.is_empty() {
                    session_model = m;
                }
            }
        }

        let entry: CodexJsonlEntry = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.entry_type != "event_msg" {
            continue;
        }
        let payload = match &entry.payload {
            Some(p) => p,
            None => continue,
        };
        if payload.payload_type != "token_count" {
            continue;
        }
        let info = match &payload.info {
            Some(i) => i,
            None => continue,
        };
        let usage = match &info.last_token_usage {
            Some(u) => u,
            None => continue,
        };

        let ts = match chrono::DateTime::parse_from_rfc3339(&entry.timestamp) {
            Ok(dt) => dt.with_timezone(&Local),
            Err(_) => continue,
        };

        let input = usage.input_tokens.unwrap_or(0);
        let output = usage.output_tokens.unwrap_or(0)
            + usage.reasoning_output_tokens.unwrap_or(0);
        let cache_read = usage.cached_input_tokens.unwrap_or(0);

        // Overwrite — we always want the FINAL token_count event
        final_usage = Some((ts, session_model.clone(), input, output, cache_read));
    }

    let (ts, model, input, output, cache_read) = final_usage?;
    Some(ParsedEntry {
        timestamp: ts,
        model,
        input_tokens: input,
        output_tokens: output,
        cache_creation_tokens: 0,
        cache_read_tokens: cache_read,
    })
}

/// Read all Codex session entries from `sessions_dir`, iterating date dirs
/// from `since` (inclusive) through today.
pub fn read_codex_entries(sessions_dir: &Path, since: Option<NaiveDate>) -> Vec<ParsedEntry> {
    let today = Local::now().date_naive();
    let start = since.unwrap_or(today);
    let mut entries = Vec::new();

    let mut current = start;
    while current <= today {
        let day_dir = sessions_dir
            .join(current.format("%Y").to_string())
            .join(current.format("%m").to_string())
            .join(current.format("%d").to_string());

        if day_dir.exists() {
            if let Ok(rd) = fs::read_dir(&day_dir) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if !path.extension().map_or(false, |e| e == "jsonl") {
                        continue;
                    }
                    if let Some(parsed) = parse_codex_session_file(&path) {
                        // Apply date filter
                        if parsed.timestamp.date_naive() >= start {
                            entries.push(parsed);
                        }
                    }
                }
            }
        }
        current = match current.succ_opt() {
            Some(d) => d,
            None => break,
        };
    }
    entries
}

// ─────────────────────────────────────────────────────────────────────────────
// Hour label helper
// ─────────────────────────────────────────────────────────────────────────────

fn format_hour(h: u32) -> String {
    match h {
        0 => "12AM".into(),
        1..=11 => format!("{}AM", h),
        12 => "12PM".into(),
        _ => format!("{}PM", h - 12),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared aggregation utility — build segments map for a bucket
// ─────────────────────────────────────────────────────────────────────────────

/// Aggregate (display_name, cost, tokens) keyed by model_key for a slice of entries.
fn build_segment_map(
    entries: &[&ParsedEntry],
) -> HashMap<String, (String, f64, u64)> {
    let mut map: HashMap<String, (String, f64, u64)> = HashMap::new();
    for e in entries {
        let cost = crate::pricing::calculate_cost(
            &e.model,
            e.input_tokens,
            e.output_tokens,
            e.cache_creation_tokens,
            e.cache_read_tokens,
        );
        let (name, key) = normalize_model(&e.model);
        let entry = map
            .entry(key.to_string())
            .or_insert((name.to_string(), 0.0, 0));
        entry.1 += cost;
        entry.2 += e.input_tokens + e.output_tokens;
    }
    map
}

fn segment_map_to_vec(map: HashMap<String, (String, f64, u64)>) -> Vec<ChartSegment> {
    map.into_iter()
        .map(|(key, (name, cost, tokens))| ChartSegment {
            model: name,
            model_key: key,
            cost,
            tokens,
        })
        .collect()
}

fn segment_map_to_model_summaries(
    map: &HashMap<String, (String, f64, u64)>,
) -> Vec<ModelSummary> {
    map.iter()
        .map(|(key, (name, cost, tokens))| ModelSummary {
            display_name: name.clone(),
            model_key: key.clone(),
            cost: *cost,
            tokens: *tokens,
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// UsageParser
// ─────────────────────────────────────────────────────────────────────────────

const CACHE_TTL_SECS: u64 = 120;

pub struct UsageParser {
    claude_dir: PathBuf,
    codex_dir: PathBuf,
    cache: Mutex<HashMap<String, (UsagePayload, Instant)>>,
}

impl UsageParser {
    /// Create with default home-directory paths.
    #[allow(dead_code)]
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            claude_dir: home.join(".claude").join("projects"),
            codex_dir: home.join(".codex").join("sessions"),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Create with an explicit Claude projects directory (for testing).
    #[allow(dead_code)]
    pub fn with_claude_dir(claude_dir: PathBuf) -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            claude_dir,
            codex_dir: home.join(".codex").join("sessions"),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Create with an explicit Codex sessions directory (for testing).
    #[allow(dead_code)]
    pub fn with_codex_dir(codex_dir: PathBuf) -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            claude_dir: home.join(".claude").join("projects"),
            codex_dir,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Create with explicit directories for both providers (for testing).
    #[allow(dead_code)]
    pub fn with_dirs(claude_dir: PathBuf, codex_dir: PathBuf) -> Self {
        Self {
            claude_dir,
            codex_dir,
            cache: Mutex::new(HashMap::new()),
        }
    }

    // ── Cache helpers ──

    #[allow(dead_code)]
    pub fn clear_cache(&self) {
        if let Ok(mut c) = self.cache.lock() {
            c.clear();
        }
    }

    pub fn check_cache(&self, key: &str) -> Option<UsagePayload> {
        let c = self.cache.lock().ok()?;
        if let Some((payload, ts)) = c.get(key) {
            if ts.elapsed().as_secs() < CACHE_TTL_SECS {
                let mut p = payload.clone();
                p.from_cache = true;
                return Some(p);
            }
        }
        None
    }

    pub fn store_cache(&self, key: &str, payload: UsagePayload) {
        if let Ok(mut c) = self.cache.lock() {
            c.insert(key.to_string(), (payload, Instant::now()));
        }
    }

    // ── Internal: load entries for a provider/since combination ──

    fn load_entries(&self, provider: &str, since: Option<NaiveDate>) -> Vec<ParsedEntry> {
        match provider {
            "claude" => read_claude_entries(&self.claude_dir, since),
            "codex" => read_codex_entries(&self.codex_dir, since),
            _ => {
                let mut all = read_claude_entries(&self.claude_dir, since);
                all.extend(read_codex_entries(&self.codex_dir, since));
                all
            }
        }
    }

    // ── Internal: build model_breakdown across all entries ──

    #[allow(dead_code)]
    fn build_model_breakdown(entries: &[ParsedEntry]) -> Vec<ModelSummary> {
        let refs: Vec<&ParsedEntry> = entries.iter().collect();
        let map = build_segment_map(&refs);
        segment_map_to_model_summaries(&map)
    }

    // ── Aggregation: daily ──

    pub fn get_daily(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("daily:{}:{}", provider, since);
        if let Some(cached) = self.check_cache(&cache_key) {
            return cached;
        }

        let since_date = parse_since_date(since);
        let entries = self.load_entries(provider, since_date);

        // Group by NaiveDate using a BTreeMap so dates are ordered
        let mut day_map: std::collections::BTreeMap<NaiveDate, Vec<&ParsedEntry>> =
            std::collections::BTreeMap::new();
        for e in &entries {
            day_map.entry(e.timestamp.date_naive()).or_default().push(e);
        }

        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, (String, f64, u64)> = HashMap::new();

        for (date, day_entries) in &day_map {
            let label = date.format("%b %-d").to_string();
            let seg_map = build_segment_map(day_entries);
            let bucket_cost: f64 = seg_map.values().map(|(_, c, _)| c).sum();
            let bucket_tokens: u64 = seg_map.values().map(|(_, _, t)| t).sum();

            total_cost += bucket_cost;
            total_tokens += bucket_tokens;

            for e in day_entries.iter() {
                total_input += e.input_tokens + e.cache_creation_tokens + e.cache_read_tokens;
                total_output += e.output_tokens;
            }

            // Merge into global model map
            for (key, (name, cost, tokens)) in &seg_map {
                let gm = global_model_map
                    .entry(key.clone())
                    .or_insert((name.clone(), 0.0, 0));
                gm.1 += cost;
                gm.2 += tokens;
            }

            chart_buckets.push(ChartBucket {
                label,
                total: bucket_cost,
                segments: segment_map_to_vec(seg_map),
            });
        }

        let model_breakdown = segment_map_to_model_summaries(&global_model_map);
        let session_count = day_map.len() as u32;

        let payload = UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
        };

        self.store_cache(&cache_key, payload.clone());
        payload
    }

    // ── Aggregation: monthly ──

    pub fn get_monthly(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("monthly:{}:{}", provider, since);
        if let Some(cached) = self.check_cache(&cache_key) {
            return cached;
        }

        let since_date = parse_since_date(since);
        let entries = self.load_entries(provider, since_date);

        // Group by YYYY-MM string using a BTreeMap for order
        let mut month_map: std::collections::BTreeMap<String, Vec<&ParsedEntry>> =
            std::collections::BTreeMap::new();
        for e in &entries {
            let key = e.timestamp.format("%Y-%m").to_string();
            month_map.entry(key).or_default().push(e);
        }

        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, (String, f64, u64)> = HashMap::new();

        for (ym, month_entries) in &month_map {
            // Label: parse "YYYY-MM" -> "Jan", "Feb", etc.
            let label = NaiveDate::parse_from_str(&format!("{}-01", ym), "%Y-%m-%d")
                .map(|d| d.format("%b").to_string())
                .unwrap_or_else(|_| ym.clone());

            let seg_map = build_segment_map(month_entries);
            let bucket_cost: f64 = seg_map.values().map(|(_, c, _)| c).sum();
            let bucket_tokens: u64 = seg_map.values().map(|(_, _, t)| t).sum();

            total_cost += bucket_cost;
            total_tokens += bucket_tokens;

            for e in month_entries.iter() {
                total_input += e.input_tokens + e.cache_creation_tokens + e.cache_read_tokens;
                total_output += e.output_tokens;
            }

            for (key, (name, cost, tokens)) in &seg_map {
                let gm = global_model_map
                    .entry(key.clone())
                    .or_insert((name.clone(), 0.0, 0));
                gm.1 += cost;
                gm.2 += tokens;
            }

            chart_buckets.push(ChartBucket {
                label,
                total: bucket_cost,
                segments: segment_map_to_vec(seg_map),
            });
        }

        let model_breakdown = segment_map_to_model_summaries(&global_model_map);
        let session_count = month_map.len() as u32;

        let payload = UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
        };

        self.store_cache(&cache_key, payload.clone());
        payload
    }

    // ── Aggregation: hourly ──

    pub fn get_hourly(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("hourly:{}:{}", provider, since);
        if let Some(cached) = self.check_cache(&cache_key) {
            return cached;
        }

        let since_date = parse_since_date(since);
        let entries = self.load_entries(provider, since_date);

        // Group by hour (0-23)
        let mut hour_map: HashMap<u32, Vec<&ParsedEntry>> = HashMap::new();
        for e in &entries {
            hour_map
                .entry(e.timestamp.hour())
                .or_default()
                .push(e);
        }

        let now = Local::now();
        let current_hour = now.hour();
        let min_hour = hour_map.keys().copied().min().unwrap_or(current_hour);

        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, (String, f64, u64)> = HashMap::new();

        for h in min_hour..=current_hour {
            let label = format_hour(h);
            let hour_entries = hour_map.get(&h).map(|v| v.as_slice()).unwrap_or(&[]);

            let seg_map = build_segment_map(hour_entries);
            let bucket_cost: f64 = seg_map.values().map(|(_, c, _)| c).sum();
            let bucket_tokens: u64 = seg_map.values().map(|(_, _, t)| t).sum();

            total_cost += bucket_cost;
            total_tokens += bucket_tokens;

            for e in hour_entries.iter() {
                total_input += e.input_tokens + e.cache_creation_tokens + e.cache_read_tokens;
                total_output += e.output_tokens;
            }

            for (key, (name, cost, tokens)) in &seg_map {
                let gm = global_model_map
                    .entry(key.clone())
                    .or_insert((name.clone(), 0.0, 0));
                gm.1 += cost;
                gm.2 += tokens;
            }

            chart_buckets.push(ChartBucket {
                label,
                total: bucket_cost,
                segments: segment_map_to_vec(seg_map),
            });
        }

        let model_breakdown = segment_map_to_model_summaries(&global_model_map);
        let session_count = chart_buckets.iter().filter(|b| b.total > 0.0).count() as u32;

        let payload = UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            chart_buckets,
            model_breakdown,
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
        };

        self.store_cache(&cache_key, payload.clone());
        payload
    }

    // ── Aggregation: blocks ──

    pub fn get_blocks(&self, provider: &str, since: &str) -> UsagePayload {
        let cache_key = format!("blocks:{}:{}", provider, since);
        if let Some(cached) = self.check_cache(&cache_key) {
            return cached;
        }

        let since_date = parse_since_date(since);
        let mut entries = self.load_entries(provider, since_date);

        // Sort by timestamp ascending
        entries.sort_by_key(|e| e.timestamp);

        // NOT a const — chrono::Duration::minutes() is not const fn
        let gap_threshold = chrono::Duration::minutes(30);

        // Split into blocks separated by gaps > 30 minutes
        let mut blocks: Vec<Vec<&ParsedEntry>> = Vec::new();
        {
            let entry_refs: Vec<&ParsedEntry> = entries.iter().collect();
            let mut current_block: Vec<&ParsedEntry> = Vec::new();
            let mut prev_ts: Option<DateTime<Local>> = None;

            for e in &entry_refs {
                if let Some(prev) = prev_ts {
                    if e.timestamp - prev > gap_threshold {
                        if !current_block.is_empty() {
                            blocks.push(std::mem::take(&mut current_block));
                        }
                    }
                }
                current_block.push(e);
                prev_ts = Some(e.timestamp);
            }
            if !current_block.is_empty() {
                blocks.push(current_block);
            }
        }

        let now = Local::now();
        let mut chart_buckets: Vec<ChartBucket> = Vec::new();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut global_model_map: HashMap<String, (String, f64, u64)> = HashMap::new();
        let mut active_block: Option<ActiveBlock> = None;
        let mut five_hour_cost = 0.0f64;

        for (idx, block) in blocks.iter().enumerate() {
            let seg_map = build_segment_map(block);
            let block_cost: f64 = seg_map.values().map(|(_, c, _)| c).sum();
            let block_tokens: u64 = seg_map.values().map(|(_, _, t)| t).sum();

            total_cost += block_cost;
            total_tokens += block_tokens;

            for e in block.iter() {
                total_input += e.input_tokens + e.cache_creation_tokens + e.cache_read_tokens;
                total_output += e.output_tokens;
            }

            for (key, (name, cost, tokens)) in &seg_map {
                let gm = global_model_map
                    .entry(key.clone())
                    .or_insert((name.clone(), 0.0, 0));
                gm.1 += cost;
                gm.2 += tokens;
            }

            // Label: start time of block formatted as "9am", "10am", etc.
            let start_ts = block[0].timestamp;
            let label = start_ts.format("%-I%P").to_string();

            chart_buckets.push(ChartBucket {
                label,
                total: block_cost,
                segments: segment_map_to_vec(seg_map),
            });

            // Last block gets ActiveBlock data
            if idx == blocks.len() - 1 {
                let last_entry_ts = block.last().unwrap().timestamp;
                let is_active = (now - last_entry_ts) <= gap_threshold;

                let duration_secs = {
                    let d = last_entry_ts - start_ts;
                    d.num_seconds().max(1) as f64
                };
                let burn_rate_per_hour = block_cost / (duration_secs / 3600.0);

                // Project to 5-hour block
                let projected_cost = burn_rate_per_hour * 5.0;

                active_block = Some(ActiveBlock {
                    cost: block_cost,
                    burn_rate_per_hour,
                    projected_cost,
                    is_active,
                });
                five_hour_cost = block_cost;
            }
        }

        if active_block.is_none() {
            five_hour_cost = total_cost;
        }

        let model_breakdown = segment_map_to_model_summaries(&global_model_map);
        let session_count = blocks.len() as u32;

        let payload = UsagePayload {
            total_cost,
            total_tokens,
            session_count,
            input_tokens: total_input,
            output_tokens: total_output,
            chart_buckets,
            model_breakdown,
            active_block,
            five_hour_cost,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
        };

        self.store_cache(&cache_key, payload.clone());
        payload
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── Helpers ──

    fn write_file(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Claude parsing
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn parse_claude_entries_from_jsonl() {
        let dir = TempDir::new().unwrap();
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"user","timestamp":"2026-03-15T12:01:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":10,"output_tokens":5}}}
{"type":"assistant","timestamp":"2026-03-15T12:02:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":200,"output_tokens":80}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let entries = read_claude_entries(dir.path(), None);
        assert_eq!(entries.len(), 2, "should parse only assistant entries");
        assert_eq!(entries[0].input_tokens, 100);
        assert_eq!(entries[1].input_tokens, 200);
    }

    #[test]
    fn parse_claude_filters_by_date() {
        let dir = TempDir::new().unwrap();
        // Use noon UTC to avoid local-timezone edge cases near midnight
        let content = r#"{"type":"assistant","timestamp":"2026-01-01T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":200,"output_tokens":80}}}"#;
        write_file(&dir.path().join("session.jsonl"), content);

        let since = parse_since_date("20260301");
        let entries = read_claude_entries(dir.path(), since);
        assert_eq!(entries.len(), 1, "should only return the March entry");
        assert_eq!(entries[0].input_tokens, 200);
    }

    #[test]
    fn parse_claude_recursive_glob() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("project-abc").join("session-1");
        fs::create_dir_all(&sub).unwrap();

        let entry_line = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":50,"output_tokens":20}}}"#;
        write_file(&dir.path().join("root.jsonl"), entry_line);
        write_file(&sub.join("nested.jsonl"), entry_line);

        let entries = read_claude_entries(dir.path(), None);
        assert_eq!(entries.len(), 2, "should find files in nested subdirectories");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Codex parsing
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn parse_codex_uses_final_token_count_only() {
        let dir = TempDir::new().unwrap();
        let today = Local::now().date_naive();
        let day_dir = dir
            .path()
            .join(today.format("%Y").to_string())
            .join(today.format("%m").to_string())
            .join(today.format("%d").to_string());
        fs::create_dir_all(&day_dir).unwrap();

        // Two token_count events — cumulative: first says 100, second says 200
        // We must only use the final one (200).
        let ts = Local::now().format("%Y-%m-%dT12:00:00+00:00").to_string();
        let content = format!(
            r#"{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50,"reasoning_output_tokens":0,"cached_input_tokens":0}}}}}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":200,"output_tokens":100,"reasoning_output_tokens":0,"cached_input_tokens":0}}}}}}}}"#,
            ts = ts
        );
        write_file(&day_dir.join("session.jsonl"), &content);

        let today_str = today.format("%Y%m%d").to_string();
        let entries = read_codex_entries(dir.path(), parse_since_date(&today_str));
        assert_eq!(entries.len(), 1, "should produce one entry per session file");
        assert_eq!(
            entries[0].input_tokens, 200,
            "should use the final token_count (200), not the first (100)"
        );
    }

    #[test]
    fn parse_codex_filters_by_date() {
        let dir = TempDir::new().unwrap();

        // Old date dir — should be excluded when since = today
        let old_dir = dir.path().join("2025").join("01").join("01");
        fs::create_dir_all(&old_dir).unwrap();
        let old_ts = "2025-01-01T12:00:00+00:00";
        let old_content = format!(
            r#"{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":999,"output_tokens":1}}}}}}}}"#,
            ts = old_ts
        );
        write_file(&old_dir.join("old.jsonl"), &old_content);

        let today = Local::now().date_naive();
        let today_str = today.format("%Y%m%d").to_string();
        let entries = read_codex_entries(dir.path(), parse_since_date(&today_str));
        assert!(entries.is_empty(), "old date dir should be excluded");
    }

    #[test]
    fn parse_codex_empty_dir_returns_empty() {
        let dir = TempDir::new().unwrap();
        let entries = read_codex_entries(dir.path(), None);
        assert!(entries.is_empty());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Daily aggregation
    // ─────────────────────────────────────────────────────────────────────────

    fn make_parser_with_claude_data(content: &str) -> (TempDir, UsageParser) {
        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("session.jsonl"), content);
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        (dir, parser)
    }

    #[test]
    fn daily_aggregation_groups_by_date() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-14T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":1000,"output_tokens":500}}}
{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":2000,"output_tokens":1000}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);
        let payload = parser.get_daily("claude", "20260101");

        assert_eq!(payload.chart_buckets.len(), 2, "should have 2 day buckets");
        let labels: Vec<&str> = payload.chart_buckets.iter().map(|b| b.label.as_str()).collect();
        assert!(labels.contains(&"Mar 14"), "should have Mar 14 bucket");
        assert!(labels.contains(&"Mar 15"), "should have Mar 15 bucket");
    }

    #[test]
    fn daily_aggregation_model_breakdown() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":1000,"output_tokens":500}}}
{"type":"assistant","timestamp":"2026-03-15T12:30:00+00:00","message":{"model":"claude-opus-4-6","usage":{"input_tokens":500,"output_tokens":200}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);
        let payload = parser.get_daily("claude", "20260315");

        assert_eq!(
            payload.model_breakdown.len(),
            2,
            "should have 2 distinct model summaries"
        );
        let keys: Vec<&str> = payload.model_breakdown.iter().map(|m| m.model_key.as_str()).collect();
        assert!(keys.contains(&"sonnet"), "should include sonnet");
        assert!(keys.contains(&"opus"), "should include opus");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Caching
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn cache_returns_same_payload_within_ttl() {
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":1000,"output_tokens":500}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);

        let first = parser.get_daily("claude", "20260315");
        assert!(!first.from_cache, "first call should NOT be from cache");

        let second = parser.get_daily("claude", "20260315");
        assert!(second.from_cache, "second call within TTL should be from cache");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Monthly aggregation
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn monthly_aggregation_groups_by_month() {
        let content = r#"{"type":"assistant","timestamp":"2026-01-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":1000,"output_tokens":500}}}
{"type":"assistant","timestamp":"2026-02-10T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":2000,"output_tokens":1000}}}
{"type":"assistant","timestamp":"2026-03-05T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":3000,"output_tokens":1500}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);
        let payload = parser.get_monthly("claude", "20260101");

        assert_eq!(payload.chart_buckets.len(), 3, "should have 3 month buckets");
        let labels: Vec<&str> = payload.chart_buckets.iter().map(|b| b.label.as_str()).collect();
        assert!(labels.contains(&"Jan"), "should have Jan bucket");
        assert!(labels.contains(&"Feb"), "should have Feb bucket");
        assert!(labels.contains(&"Mar"), "should have Mar bucket");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Hourly aggregation
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn hourly_aggregation_groups_by_hour() {
        // Use today's date so entries are not filtered out by "today only" logic
        let today = Local::now().format("%Y-%m-%dT").to_string();
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{today}09:00:00+00:00","message":{{"model":"claude-sonnet-4-6","usage":{{"input_tokens":1000,"output_tokens":500}}}}}}
{{"type":"assistant","timestamp":"{today}14:00:00+00:00","message":{{"model":"claude-sonnet-4-6","usage":{{"input_tokens":2000,"output_tokens":1000}}}}}}"#,
            today = today
        );

        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("session.jsonl"), &content);
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());

        let today_str = Local::now().format("%Y%m%d").to_string();
        let payload = parser.get_hourly("claude", &today_str);

        // Should have buckets covering from min_hour (9) to current hour
        assert!(
            !payload.chart_buckets.is_empty(),
            "should produce chart buckets"
        );
        // At minimum 9AM bucket should exist
        let has_9am = payload.chart_buckets.iter().any(|b| b.label == "9AM");
        assert!(has_9am, "should have a 9AM bucket");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Blocks aggregation
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn blocks_detects_activity_windows() {
        // Two entries more than 30 minutes apart -> 2 blocks
        let content = r#"{"type":"assistant","timestamp":"2026-03-15T09:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":1000,"output_tokens":500}}}
{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":2000,"output_tokens":1000}}}"#;
        let (_dir, parser) = make_parser_with_claude_data(content);
        let payload = parser.get_blocks("claude", "20260315");

        assert_eq!(
            payload.chart_buckets.len(),
            2,
            "entries >30 min apart should produce 2 activity blocks"
        );
    }
}
