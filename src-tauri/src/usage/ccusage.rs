use crate::models::{
    ActiveBlock, ChartBucket, ChartSegment, ModelSummary, UsagePayload, UsageSource,
};
use chrono::{DateTime, Datelike, Local, NaiveDate};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Command as StdCommand;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const CCUSAGE_CLI_PATH_ENV: &str = "CCUSAGE_CLI_PATH";
const CCUSAGE_CODEX_CLI_PATH_ENV: &str = "CCUSAGE_CODEX_CLI_PATH";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CcusageProvider {
    Claude,
    Codex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BucketMode {
    Day,
    Month,
}

#[derive(Clone, Debug)]
struct DailyRow {
    date: NaiveDate,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    cost_usd: f64,
    breakdown: Vec<ModelUsageRow>,
}

#[derive(Clone, Debug)]
struct BlockRow {
    block_start: DateTime<Local>,
    cost_usd: f64,
    projected_cost: Option<f64>,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    is_active: bool,
    models: Vec<String>,
    breakdown: Vec<ModelUsageRow>,
}

#[derive(Clone, Debug)]
struct ModelUsageRow {
    raw_model: String,
    cost_usd: f64,
    total_tokens: u64,
}

#[derive(Clone, Debug, Default)]
struct BucketAccum {
    label: String,
    sort_key: String,
    total_cost: f64,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    model_map: HashMap<String, (String, f64, u64)>,
}

pub fn fetch_usage_payload(
    provider: &str,
    period: &str,
    offset: i32,
) -> Result<UsagePayload, String> {
    let provider = match provider {
        "claude" => CcusageProvider::Claude,
        "codex" => CcusageProvider::Codex,
        other => return Err(format!("ccusage does not support provider \"{other}\"")),
    };

    match (provider, period) {
        (_, "day") => Err(String::from(
            "ccusage does not provide hourly reports for the day view yet",
        )),
        (CcusageProvider::Codex, "5h") => Err(String::from(
            "@ccusage/codex does not expose 5-hour blocks reports yet",
        )),
        _ if cfg!(test) && std::env::var_os("TOKEN_MONITOR_ENABLE_CCUSAGE_TESTS").is_none() => {
            Err(String::from("ccusage integration is disabled in tests"))
        }
        (CcusageProvider::Claude, "5h") => fetch_blocks_payload(provider),
        (_, "week" | "month") => {
            fetch_daily_aggregate_payload(provider, period, offset, BucketMode::Day)
        }
        (_, "year") => fetch_daily_aggregate_payload(provider, period, offset, BucketMode::Month),
        _ => Err(format!("Unsupported period \"{period}\"")),
    }
}

fn fetch_daily_aggregate_payload(
    provider: CcusageProvider,
    period: &str,
    offset: i32,
    bucket_mode: BucketMode,
) -> Result<UsagePayload, String> {
    let (start, end) = date_bounds_for_period(period, offset)?;
    let json = run_daily_report(provider, start, end - chrono::Duration::days(1))?;
    let rows = parse_daily_rows(&json, true)?;
    Ok(build_payload_from_daily_rows(rows, bucket_mode))
}

fn fetch_blocks_payload(provider: CcusageProvider) -> Result<UsagePayload, String> {
    let today = Local::now().date_naive();
    let json = run_blocks_report(provider, today, today)?;
    let rows = parse_block_rows(&json)?;
    Ok(build_payload_from_block_rows(rows))
}

fn run_daily_report(
    provider: CcusageProvider,
    since: NaiveDate,
    until: NaiveDate,
) -> Result<Value, String> {
    let mut args = vec![
        String::from("daily"),
        String::from("--json"),
        String::from("--breakdown"),
        String::from("--since"),
        since.format("%Y%m%d").to_string(),
        String::from("--until"),
        until.format("%Y%m%d").to_string(),
    ];

    if provider == CcusageProvider::Claude {
        args.push(String::from("--mode"));
        args.push(String::from("auto"));
    }

    run_ccusage_command(provider, &args)
}

fn run_blocks_report(
    provider: CcusageProvider,
    since: NaiveDate,
    until: NaiveDate,
) -> Result<Value, String> {
    let mut args = vec![
        String::from("blocks"),
        String::from("--json"),
        String::from("--since"),
        since.format("%Y%m%d").to_string(),
        String::from("--until"),
        until.format("%Y%m%d").to_string(),
    ];

    if provider == CcusageProvider::Claude {
        args.push(String::from("--mode"));
        args.push(String::from("auto"));
    }

    run_ccusage_command(provider, &args)
}

fn run_ccusage_command(provider: CcusageProvider, args: &[String]) -> Result<Value, String> {
    let (program, mut base_args) = resolve_cli_command(provider)?;
    base_args.extend(args.iter().cloned());

    let mut command = StdCommand::new(&program);
    command.args(&base_args);

    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|error| format!("failed to launch {}: {error}", display_program(provider)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit code {:?}", output.status.code())
        };
        return Err(format!(
            "{} exited unsuccessfully: {detail}",
            display_program(provider)
        ));
    }

    serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "{} returned invalid JSON: {error}",
            display_program(provider)
        )
    })
}

fn resolve_cli_command(provider: CcusageProvider) -> Result<(PathBuf, Vec<String>), String> {
    match provider {
        CcusageProvider::Claude => {
            if let Some(path) = std::env::var_os(CCUSAGE_CLI_PATH_ENV).map(PathBuf::from) {
                if path.is_file() {
                    return Ok((path, Vec::new()));
                }
            }
            if let Some(path) = command_in_path("ccusage") {
                return Ok((path, Vec::new()));
            }
            if let Some(path) = common_ccusage_paths()
                .into_iter()
                .find(|candidate| candidate.is_file())
            {
                return Ok((path, Vec::new()));
            }
            let npx = resolve_npx_path()?;
            Ok((npx, vec![String::from("ccusage@latest")]))
        }
        CcusageProvider::Codex => {
            if let Some(path) = std::env::var_os(CCUSAGE_CODEX_CLI_PATH_ENV).map(PathBuf::from) {
                if path.is_file() {
                    return Ok((path, Vec::new()));
                }
            }
            let npx = resolve_npx_path()?;
            Ok((npx, vec![String::from("@ccusage/codex@latest")]))
        }
    }
}

fn display_program(provider: CcusageProvider) -> &'static str {
    match provider {
        CcusageProvider::Claude => "ccusage",
        CcusageProvider::Codex => "@ccusage/codex",
    }
}

fn resolve_npx_path() -> Result<PathBuf, String> {
    if let Some(path) = command_in_path("npx") {
        return Ok(path);
    }

    if let Some(path) = common_npx_paths()
        .into_iter()
        .find(|candidate| candidate.is_file())
    {
        return Ok(path);
    }

    Err(String::from("npx was not found on this system"))
}

fn command_in_path(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(target_os = "windows")]
        {
            let cmd = dir.join(format!("{binary}.cmd"));
            if cmd.is_file() {
                return Some(cmd);
            }
            let exe = dir.join(format!("{binary}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

fn common_ccusage_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    #[cfg(not(target_os = "windows"))]
    {
        candidates.push(PathBuf::from("/opt/homebrew/bin/ccusage"));
        candidates.push(PathBuf::from("/usr/local/bin/ccusage"));
        candidates.push(PathBuf::from("/usr/bin/ccusage"));
    }

    if let Some(home) = dirs::home_dir() {
        #[cfg(not(target_os = "windows"))]
        {
            candidates.push(home.join(".local").join("bin").join("ccusage"));
            candidates.push(home.join(".npm-global").join("bin").join("ccusage"));
            candidates.push(home.join(".volta").join("bin").join("ccusage"));
            candidates.push(
                home.join(".fnm")
                    .join("aliases")
                    .join("default")
                    .join("bin")
                    .join("ccusage"),
            );
        }

        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let appdata = PathBuf::from(appdata);
                candidates.push(appdata.join("npm").join("ccusage.cmd"));
                candidates.push(appdata.join("npm").join("ccusage"));
            }
        }

        add_nvm_candidates(&home, &mut candidates, "ccusage");
    }

    candidates
}

fn common_npx_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    #[cfg(not(target_os = "windows"))]
    {
        candidates.push(PathBuf::from("/opt/homebrew/bin/npx"));
        candidates.push(PathBuf::from("/usr/local/bin/npx"));
        candidates.push(PathBuf::from("/usr/bin/npx"));
    }

    if let Some(home) = dirs::home_dir() {
        #[cfg(not(target_os = "windows"))]
        {
            candidates.push(home.join(".local").join("bin").join("npx"));
            candidates.push(home.join(".npm-global").join("bin").join("npx"));
            candidates.push(home.join(".volta").join("bin").join("npx"));
            candidates.push(
                home.join(".fnm")
                    .join("aliases")
                    .join("default")
                    .join("bin")
                    .join("npx"),
            );
        }

        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let appdata = PathBuf::from(appdata);
                candidates.push(appdata.join("npm").join("npx.cmd"));
                candidates.push(appdata.join("npm").join("npx"));
            }
            let program_files = std::env::var("ProgramFiles").ok().map(PathBuf::from);
            if let Some(program_files) = program_files {
                candidates.push(program_files.join("nodejs").join("npx.cmd"));
            }
        }

        add_nvm_candidates(&home, &mut candidates, "npx");
    }

    candidates
}

fn add_nvm_candidates(home: &std::path::Path, candidates: &mut Vec<PathBuf>, binary: &str) {
    #[cfg(not(target_os = "windows"))]
    let nvm_dir = home.join(".nvm").join("versions").join("node");
    #[cfg(target_os = "windows")]
    let nvm_dir = std::env::var("NVM_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("APPDATA")
                .map(|a| PathBuf::from(a).join("nvm"))
                .unwrap_or_else(|_| home.join("AppData").join("Roaming").join("nvm"))
        });

    if let Ok(entries) = std::fs::read_dir(nvm_dir) {
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        versions.sort_unstable_by(|a, b| b.cmp(a));
        for version in versions {
            #[cfg(not(target_os = "windows"))]
            candidates.push(version.join("bin").join(binary));
            #[cfg(target_os = "windows")]
            {
                candidates.push(version.join(format!("{binary}.cmd")));
                candidates.push(version.join(binary));
            }
        }
    }
}

fn date_bounds_for_period(period: &str, offset: i32) -> Result<(NaiveDate, NaiveDate), String> {
    let now = Local::now();
    let today = now.date_naive();
    match period {
        "week" => {
            let monday =
                today - chrono::Duration::days(now.weekday().num_days_from_monday() as i64);
            let start = monday + chrono::Duration::days((offset * 7) as i64);
            Ok((start, start + chrono::Duration::days(7)))
        }
        "month" => {
            let mut year = now.year();
            let mut month = now.month() as i32 + offset;
            while month <= 0 {
                year -= 1;
                month += 12;
            }
            while month > 12 {
                year += 1;
                month -= 12;
            }
            let start = NaiveDate::from_ymd_opt(year, month as u32, 1).unwrap();
            let end = if month == 12 {
                NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(year, month as u32 + 1, 1).unwrap()
            };
            Ok((start, end))
        }
        "year" => {
            let year = now.year() + offset;
            Ok((
                NaiveDate::from_ymd_opt(year, 1, 1).unwrap(),
                NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap(),
            ))
        }
        other => Err(format!(
            "Unsupported ccusage date bounds period \"{other}\""
        )),
    }
}

fn build_payload_from_daily_rows(rows: Vec<DailyRow>, bucket_mode: BucketMode) -> UsagePayload {
    let mut buckets = BTreeMap::<String, BucketAccum>::new();
    let mut total_cost = 0.0;
    let mut total_tokens = 0u64;
    let mut total_input = 0u64;
    let mut total_output = 0u64;
    let mut global_model_map = HashMap::<String, (String, f64, u64)>::new();

    for row in rows {
        let (sort_key, label) = match bucket_mode {
            BucketMode::Day => (
                row.date.format("%Y-%m-%d").to_string(),
                row.date.format("%b %-d").to_string(),
            ),
            BucketMode::Month => (
                row.date.format("%Y-%m").to_string(),
                row.date.format("%b").to_string(),
            ),
        };

        let bucket = buckets
            .entry(sort_key.clone())
            .or_insert_with(|| BucketAccum {
                label,
                sort_key,
                ..BucketAccum::default()
            });

        bucket.total_cost += row.cost_usd;
        bucket.total_tokens += row.total_tokens;
        bucket.input_tokens += row.input_tokens;
        bucket.output_tokens += row.output_tokens;
        total_cost += row.cost_usd;
        total_tokens += row.total_tokens;
        total_input += row.input_tokens;
        total_output += row.output_tokens;

        for model in row.breakdown {
            merge_model_usage(&mut bucket.model_map, &model);
            merge_model_usage(&mut global_model_map, &model);
        }
    }

    let chart_buckets = buckets
        .into_values()
        .map(|bucket| ChartBucket {
            label: bucket.label,
            sort_key: bucket.sort_key,
            total: bucket.total_cost,
            segments: model_map_to_segments(bucket.model_map),
        })
        .collect::<Vec<_>>();

    UsagePayload {
        total_cost,
        total_tokens,
        session_count: chart_buckets
            .iter()
            .filter(|bucket| bucket.total > 0.0)
            .count() as u32,
        input_tokens: total_input,
        output_tokens: total_output,
        cache_read_tokens: 0,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        web_search_requests: 0,
        chart_buckets,
        model_breakdown: model_map_to_model_summaries(global_model_map),
        active_block: None,
        five_hour_cost: 0.0,
        last_updated: Local::now().to_rfc3339(),
        from_cache: false,
        usage_source: UsageSource::Ccusage,
        usage_warning: None,
        period_label: String::new(),
        has_earlier_data: false,
        change_stats: None,
        subagent_stats: None,
        device_breakdown: None,
        device_chart_buckets: None,
    }
}

fn build_payload_from_block_rows(rows: Vec<BlockRow>) -> UsagePayload {
    let mut chart_buckets = Vec::new();
    let mut global_model_map = HashMap::<String, (String, f64, u64)>::new();
    let mut total_cost = 0.0;
    let mut total_tokens = 0u64;
    let mut total_input = 0u64;
    let mut total_output = 0u64;
    let mut active_block = None;

    for row in rows {
        total_cost += row.cost_usd;
        total_tokens += row.total_tokens;
        total_input += row.input_tokens;
        total_output += row.output_tokens;

        let segments = if row.breakdown.is_empty() {
            let label = if row.models.len() == 1 {
                let model = crate::models::known_model_from_raw(&row.models[0]);
                (model.display_name, model.model_key)
            } else {
                (String::from("Usage"), String::from("usage"))
            };
            vec![ChartSegment {
                model: label.0,
                model_key: label.1,
                cost: row.cost_usd,
                tokens: row.total_tokens,
            }]
        } else {
            row.breakdown
                .iter()
                .map(|model| {
                    let known = crate::models::known_model_from_raw(&model.raw_model);
                    merge_model_usage(&mut global_model_map, model);
                    ChartSegment {
                        model: known.display_name,
                        model_key: known.model_key,
                        cost: model.cost_usd,
                        tokens: model.total_tokens,
                    }
                })
                .collect()
        };

        if row.breakdown.is_empty() {
            let fallback = ModelUsageRow {
                raw_model: row
                    .models
                    .first()
                    .cloned()
                    .unwrap_or_else(|| String::from("usage")),
                cost_usd: row.cost_usd,
                total_tokens: row.total_tokens,
            };
            merge_model_usage(&mut global_model_map, &fallback);
        }

        if row.is_active {
            active_block = Some(ActiveBlock {
                cost: row.cost_usd,
                burn_rate_per_hour: row.projected_cost.unwrap_or(row.cost_usd) / 5.0,
                projected_cost: row.projected_cost.unwrap_or(row.cost_usd),
                is_active: true,
            });
        }

        chart_buckets.push(ChartBucket {
            label: row.block_start.format("%-I%P").to_string(),
            sort_key: row.block_start.to_rfc3339(),
            total: row.cost_usd,
            segments,
        });
    }

    UsagePayload {
        total_cost,
        total_tokens,
        session_count: chart_buckets.len() as u32,
        input_tokens: total_input,
        output_tokens: total_output,
        cache_read_tokens: 0,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        web_search_requests: 0,
        chart_buckets,
        model_breakdown: model_map_to_model_summaries(global_model_map),
        active_block: active_block.clone(),
        five_hour_cost: active_block
            .as_ref()
            .map(|block| block.cost)
            .unwrap_or(total_cost),
        last_updated: Local::now().to_rfc3339(),
        from_cache: false,
        usage_source: UsageSource::Ccusage,
        usage_warning: None,
        period_label: String::new(),
        has_earlier_data: false,
        change_stats: None,
        subagent_stats: None,
        device_breakdown: None,
        device_chart_buckets: None,
    }
}

fn merge_model_usage(map: &mut HashMap<String, (String, f64, u64)>, usage: &ModelUsageRow) {
    let known = crate::models::known_model_from_raw(&usage.raw_model);
    let entry = map
        .entry(known.model_key.clone())
        .or_insert((known.display_name, 0.0, 0));
    entry.1 += usage.cost_usd;
    entry.2 += usage.total_tokens;
}

fn model_map_to_segments(map: HashMap<String, (String, f64, u64)>) -> Vec<ChartSegment> {
    map.into_iter()
        .map(|(model_key, (model, cost, tokens))| ChartSegment {
            model,
            model_key,
            cost,
            tokens,
        })
        .collect()
}

fn model_map_to_model_summaries(map: HashMap<String, (String, f64, u64)>) -> Vec<ModelSummary> {
    map.into_iter()
        .map(|(model_key, (display_name, cost, tokens))| ModelSummary {
            display_name,
            model_key,
            cost,
            tokens,
            change_stats: None,
        })
        .collect()
}

fn parse_daily_rows(
    value: &Value,
    require_detailed_breakdown: bool,
) -> Result<Vec<DailyRow>, String> {
    let raw_rows = rows_from_value(value, &["data", "daily"])?;
    raw_rows
        .iter()
        .map(|row| parse_daily_row(row, require_detailed_breakdown))
        .collect()
}

fn parse_daily_row(value: &Value, require_detailed_breakdown: bool) -> Result<DailyRow, String> {
    let date_str = value
        .get("date")
        .and_then(Value::as_str)
        .ok_or_else(|| String::from("ccusage daily row is missing date"))?;
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .map_err(|error| format!("invalid ccusage date \"{date_str}\": {error}"))?;
    let models = parse_models(value);
    let breakdown = parse_model_breakdown(value, &models, require_detailed_breakdown)?;
    let input_tokens = read_u64(value, &["inputTokens", "totalInputTokens"]);
    let output_tokens = read_u64(value, &["outputTokens", "totalOutputTokens"]);
    let cache_creation_tokens =
        read_u64(value, &["cacheCreationTokens", "totalCacheCreationTokens"]);
    let cache_read_tokens = read_u64(value, &["cacheReadTokens", "totalCacheReadTokens"]);
    let total_tokens = read_u64(value, &["totalTokens"])
        .max(input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens);
    Ok(DailyRow {
        date,
        input_tokens,
        output_tokens,
        total_tokens,
        cost_usd: read_f64(value, &["costUSD", "totalCost"]),
        breakdown,
    })
}

fn parse_block_rows(value: &Value) -> Result<Vec<BlockRow>, String> {
    let raw_rows = rows_from_value(value, &["data", "blocks"])?;
    raw_rows.iter().map(parse_block_row).collect()
}

fn parse_block_row(value: &Value) -> Result<BlockRow, String> {
    let start_str = value
        .get("blockStart")
        .and_then(Value::as_str)
        .ok_or_else(|| String::from("ccusage blocks row is missing blockStart"))?;
    let block_start = DateTime::parse_from_rfc3339(start_str)
        .map(|dt| dt.with_timezone(&Local))
        .map_err(|error| format!("invalid ccusage blockStart \"{start_str}\": {error}"))?;
    let models = parse_models(value);
    let input_tokens = read_u64(value, &["inputTokens"]);
    let output_tokens = read_u64(value, &["outputTokens"]);
    let cache_creation_tokens = read_u64(value, &["cacheCreationTokens"]);
    let cache_read_tokens = read_u64(value, &["cacheReadTokens"]);
    Ok(BlockRow {
        block_start,
        cost_usd: read_f64(value, &["costUSD", "totalCost"]),
        projected_cost: read_optional_f64(value, &["projectedCost"]),
        input_tokens,
        output_tokens,
        total_tokens: read_u64(value, &["totalTokens"])
            .max(input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens),
        is_active: value
            .get("isActive")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        models: models.clone(),
        breakdown: parse_model_breakdown(value, &models, false)?,
    })
}

fn rows_from_value<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Vec<Value>, String> {
    for key in keys {
        if let Some(rows) = value.get(*key).and_then(Value::as_array) {
            return Ok(rows);
        }
    }
    Err(String::from(
        "ccusage JSON did not contain a recognized rows array",
    ))
}

fn parse_models(value: &Value) -> Vec<String> {
    ["models", "modelsUsed"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_array))
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_model_breakdown(
    value: &Value,
    models: &[String],
    require_detailed_breakdown: bool,
) -> Result<Vec<ModelUsageRow>, String> {
    if let Some(breakdown) = value.get("breakdown") {
        if let Some(parsed) = parse_breakdown_object(breakdown) {
            return Ok(parsed);
        }
    }

    if let Some(model_breakdowns) = value.get("modelBreakdowns") {
        if let Some(parsed) = parse_breakdown_array(model_breakdowns) {
            return Ok(parsed);
        }
        if let Some(parsed) = parse_breakdown_object(model_breakdowns) {
            return Ok(parsed);
        }
    }

    if !require_detailed_breakdown {
        return Ok(Vec::new());
    }

    if models.len() <= 1 {
        let raw_model = models
            .first()
            .cloned()
            .unwrap_or_else(|| String::from("unknown"));
        return Ok(vec![ModelUsageRow {
            raw_model,
            cost_usd: read_f64(value, &["costUSD", "totalCost"]),
            total_tokens: read_u64(value, &["totalTokens"]),
        }]);
    }

    Err(String::from(
        "ccusage JSON did not include a per-model breakdown for a multi-model bucket",
    ))
}

fn parse_breakdown_object(value: &Value) -> Option<Vec<ModelUsageRow>> {
    let object = value.as_object()?;
    Some(
        object
            .iter()
            .map(|(raw_model, stats)| ModelUsageRow {
                raw_model: raw_model.clone(),
                cost_usd: read_f64(stats, &["costUSD", "totalCost", "cost"]),
                total_tokens: read_u64(stats, &["totalTokens"]),
            })
            .collect(),
    )
}

fn parse_breakdown_array(value: &Value) -> Option<Vec<ModelUsageRow>> {
    let array = value.as_array()?;
    Some(
        array
            .iter()
            .map(|entry| ModelUsageRow {
                raw_model: read_string(entry, &["model", "modelName", "name"])
                    .unwrap_or_else(|| String::from("unknown")),
                cost_usd: read_f64(entry, &["costUSD", "totalCost", "cost"]),
                total_tokens: read_u64(entry, &["totalTokens"]),
            })
            .collect(),
    )
}

fn read_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

fn read_u64(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(value_to_u64)
        .unwrap_or(0)
}

fn value_to_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|v| u64::try_from(v).ok()))
        .or_else(|| {
            value
                .as_f64()
                .filter(|v| *v >= 0.0)
                .map(|v| v.round() as u64)
        })
}

fn read_f64(value: &Value, keys: &[&str]) -> f64 {
    read_optional_f64(value, keys).unwrap_or(0.0)
}

fn read_optional_f64(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(value_to_f64)
}

fn value_to_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_u64().map(|v| v as f64))
        .or_else(|| value.as_i64().map(|v| v as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_daily_type_data_breakdown_object_shape() {
        let payload = json!({
            "type": "daily",
            "data": [{
                "date": "2026-03-15",
                "inputTokens": 100,
                "outputTokens": 40,
                "cacheCreationTokens": 10,
                "cacheReadTokens": 5,
                "totalTokens": 155,
                "costUSD": 1.25,
                "models": ["claude-sonnet-4-6-20260101"],
                "breakdown": {
                    "claude-sonnet-4-6-20260101": {
                        "totalTokens": 155,
                        "costUSD": 1.25
                    }
                }
            }]
        });

        let rows = parse_daily_rows(&payload, true).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date, NaiveDate::from_ymd_opt(2026, 3, 15).unwrap());
        assert_eq!(rows[0].breakdown.len(), 1);
        assert!((rows[0].cost_usd - 1.25).abs() < 1e-9);
    }

    #[test]
    fn parses_legacy_daily_totals_model_breakdowns_shape() {
        let payload = json!({
            "daily": [{
                "date": "2026-03-15",
                "inputTokens": 100,
                "outputTokens": 40,
                "cacheCreationTokens": 10,
                "cacheReadTokens": 5,
                "totalTokens": 155,
                "totalCost": 1.25,
                "modelsUsed": ["gpt-5.4"],
                "modelBreakdowns": [{
                    "modelName": "gpt-5.4",
                    "totalTokens": 155,
                    "totalCost": 1.25
                }]
            }]
        });

        let rows = parse_daily_rows(&payload, true).unwrap();
        assert_eq!(rows[0].breakdown[0].raw_model, "gpt-5.4");
    }

    #[test]
    fn builds_month_buckets_from_daily_rows() {
        let rows = vec![
            DailyRow {
                date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
                cost_usd: 1.0,
                breakdown: vec![ModelUsageRow {
                    raw_model: String::from("claude-sonnet-4-6-20260101"),
                    cost_usd: 1.0,
                    total_tokens: 30,
                }],
            },
            DailyRow {
                date: NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
                input_tokens: 5,
                output_tokens: 10,
                total_tokens: 15,
                cost_usd: 0.5,
                breakdown: vec![ModelUsageRow {
                    raw_model: String::from("claude-sonnet-4-6-20260101"),
                    cost_usd: 0.5,
                    total_tokens: 15,
                }],
            },
        ];

        let payload = build_payload_from_daily_rows(rows, BucketMode::Month);
        assert_eq!(payload.chart_buckets.len(), 1);
        assert_eq!(payload.chart_buckets[0].sort_key, "2026-01");
        assert!((payload.total_cost - 1.5).abs() < 1e-9);
    }

    #[test]
    fn parses_blocks_and_derives_cost_burn_rate() {
        let payload = json!({
            "type": "blocks",
            "data": [{
                "blockStart": "2026-03-15T10:00:00.000Z",
                "isActive": true,
                "inputTokens": 100,
                "outputTokens": 40,
                "totalTokens": 140,
                "costUSD": 2.5,
                "projectedCost": 7.5,
                "models": ["claude-sonnet-4-6-20260101"]
            }]
        });

        let payload = build_payload_from_block_rows(parse_block_rows(&payload).unwrap());
        let active = payload.active_block.expect("active block");
        assert!((active.burn_rate_per_hour - 1.5).abs() < 1e-9);
        assert!((payload.five_hour_cost - 2.5).abs() < 1e-9);
    }

    #[test]
    fn rejects_multi_model_rows_without_breakdown() {
        let payload = json!({
            "type": "daily",
            "data": [{
                "date": "2026-03-15",
                "totalTokens": 10,
                "costUSD": 1.0,
                "models": ["claude-sonnet-4-6-20260101", "claude-opus-4-6-20260101"]
            }]
        });

        let error = parse_daily_rows(&payload, true).unwrap_err();
        assert!(error.contains("per-model breakdown"));
    }
}
