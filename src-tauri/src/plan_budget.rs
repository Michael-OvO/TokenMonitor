//! Plan budgets in dollars, learned from how far each rate-limit meter moved
//! while one model did (almost) all of the spending.
//!
//! Every rate-limit refresh appends the meters whose reading changed to
//! `limit-samples.jsonl` in the app data dir. A stretch between two readings
//! where one model made ≥90% of the local spend gives that model's dollars
//! per percentage point; summing its stretches over the last two weeks gives
//! its budget for a full window.

use crate::commands::AppState;
use crate::models::RateLimitsPayload;
use crate::usage::parser::UsageParser;
use chrono::{DateTime, Duration, Local, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration as StdDuration, Instant, SystemTime};
use tauri::State;

const FILE_NAME: &str = "limit-samples.jsonl";
const RETAIN_DAYS: i64 = 30;
const LOOKBACK_DAYS: i64 = 14;
const TRIM_OVER_BYTES: u64 = 4 * 1024 * 1024;
/// Share of a stretch's spend one model needs for the stretch to count as its.
const DOMINANT_SHARE: f64 = 0.9;
/// Meter points a model needs before its budget is shown.
const MIN_POINTS: f64 = 10.0;
/// Resets this close together are the same window: sources round differently.
const SAME_RESET_SLACK_MINS: i64 = 10;

static SAMPLES_FILE: OnceLock<PathBuf> = OnceLock::new();
type LastSeen = HashMap<(String, String), (f64, Option<String>)>;
static LAST_SEEN: OnceLock<Mutex<LastSeen>> = OnceLock::new();

/// (provider, window_id)
pub(crate) type Key = (String, String);
/// A result with the start of the window it was computed for.
type Results = HashMap<Key, (DateTime<Utc>, PlanBudget)>;
struct Job {
    since: DateTime<Utc>,
    asked: Instant,
}
/// Bars the page has asked for, with the start of their current window.
static JOBS: OnceLock<Mutex<HashMap<Key, Job>>> = OnceLock::new();
/// What the page reads: the last published result per bar.
static PUBLISHED: OnceLock<Mutex<Results>> = OnceLock::new();
/// Results the refresh computed since its last publish.
static STAGED: OnceLock<Mutex<Results>> = OnceLock::new();
/// A bar not asked for this long drops out of the refresh.
const ASK_TTL: StdDuration = StdDuration::from_secs(3_600);
/// A provider's bars: each one's window id and the start of its window.
pub(crate) type Bars = Vec<(String, DateTime<Utc>)>;
/// What a provider's bars are computed from: the change counts of its
/// entries, of its meter readings and of the price table.
type Inputs = (u64, u64, u64);
/// Per provider, what the refresh last computed its bars from, and when.
static COMPUTED: OnceLock<Mutex<HashMap<String, (Inputs, Instant)>>> = OnceLock::new();
/// Readings appended to the samples file so far, per provider.
static RECORDED: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
/// Bars whose inputs hold are still recomputed this often: their two-week
/// lookback moves on regardless.
const RECOMPUTE_EVERY: StdDuration = StdDuration::from_secs(3_600);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Sample {
    t: DateTime<Utc>,
    p: String,
    w: String,
    u: f64,
    r: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelBudget {
    pub model: String,
    pub usd: f64,
    pub low_usd: f64,
    pub high_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanBudget {
    /// Local spend since the window started (for the rough fallback range).
    pub spend: f64,
    /// Budget per model with enough single-model stretches, cheapest first.
    pub models: Vec<ModelBudget>,
}

pub fn init(app_data: &Path) {
    let _ = SAMPLES_FILE.set(app_data.join(FILE_NAME));
}

/// Append every meter whose reading changed since the last call.
pub fn record(payload: &RateLimitsPayload) {
    let Some(path) = SAMPLES_FILE.get() else {
        return;
    };
    let now = Utc::now();
    let mut last = LAST_SEEN
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut lines = String::new();
    let mut recorded = Vec::new();
    let providers = [
        &payload.claude,
        &payload.codex,
        &payload.cursor,
        &payload.kimi,
    ];
    for limits in providers.into_iter().flatten() {
        // Errored/stale payloads carry cached windows, not a new reading.
        // Codex readings come from its own session logs instead.
        if limits.stale || limits.error.is_some() || limits.provider == "codex" {
            continue;
        }
        let t = reading_time(&limits.fetched_at, now);
        for w in &limits.windows {
            let reading = (w.utilization, w.resets_at.clone());
            let key = (limits.provider.clone(), w.window_id.clone());
            if last.get(&key) == Some(&reading) {
                continue;
            }
            last.insert(key, reading);
            let sample = Sample {
                t,
                p: limits.provider.clone(),
                w: w.window_id.clone(),
                u: w.utilization,
                r: w.resets_at.as_deref().and_then(parse_utc),
            };
            if let Ok(line) = serde_json::to_string(&sample) {
                lines.push_str(&line);
                lines.push('\n');
                recorded.push(sample.p);
            }
        }
    }
    if lines.is_empty() {
        return;
    }
    let appended = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(lines.as_bytes()));
    if let Err(err) = appended {
        tracing::warn!("limit samples append failed: {err}");
        return;
    }
    let mut counts = lock(&RECORDED);
    for provider in recorded {
        *counts.entry(provider).or_default() += 1;
    }
    drop(counts);
    if fs::metadata(path).is_ok_and(|m| m.len() > TRIM_OVER_BYTES) {
        let kept: Vec<String> = read_samples(path)
            .into_iter()
            .filter(|s| s.t >= now - Duration::days(RETAIN_DAYS))
            .filter_map(|s| serde_json::to_string(&s).ok())
            .collect();
        let tmp = path.with_extension("jsonl.tmp");
        if fs::write(&tmp, kept.join("\n") + "\n").is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }
}

/// When a reading was taken: its `fetched_at`, never later than `now`, or
/// `now` when that does not parse. A statusline reading can be minutes old,
/// and the spend it is paired with must end where the meter's reading did.
fn reading_time(fetched_at: &str, now: DateTime<Utc>) -> DateTime<Utc> {
    parse_utc(fetched_at).map_or(now, |t| t.min(now))
}

fn parse_utc(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn read_samples(path: &Path) -> Vec<Sample> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

fn same_window(a: &Sample, b: &Sample) -> bool {
    match (a.r, b.r) {
        (Some(x), Some(y)) => within_slack(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// Resets (or window starts) this close together belong to the same window.
fn within_slack(a: DateTime<Utc>, b: DateTime<Utc>) -> bool {
    (a - b).num_minutes().abs() <= SAME_RESET_SLACK_MINS
}

#[derive(Default)]
struct Acc {
    spend: f64,
    points: f64,
    runs: u32,
}

/// Budget per model from one meter's readings and the priced entries
/// `(time, display_name, model_key, usd)`, both sorted by time.
fn calibrate(
    samples: &[Sample],
    entries: &[(DateTime<Utc>, String, String, f64)],
) -> Vec<ModelBudget> {
    let mut acc: HashMap<String, (String, Acc)> = HashMap::new();
    let mut run_model: Option<String> = None;
    let mut i = 0;
    for pair in samples.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let points = b.u - a.u;
        if !same_window(a, b) || points < 0.0 {
            run_model = None;
            continue;
        }
        while i < entries.len() && entries[i].0 < a.t {
            i += 1;
        }
        let mut by_model: HashMap<&str, (&str, f64)> = HashMap::new();
        while i < entries.len() && entries[i].0 < b.t {
            let (_, name, key, usd) = &entries[i];
            by_model.entry(key).or_insert((name, 0.0)).1 += usd;
            i += 1;
        }
        let total: f64 = by_model.values().map(|(_, usd)| usd).sum();
        if total <= 0.0 {
            if points > 0.0 {
                // The meter moved with nothing spent here: usage elsewhere.
                run_model = None;
            }
            continue;
        }
        let Some((key, (name, top))) = by_model
            .iter()
            .max_by(|x, y| x.1 .1.total_cmp(&y.1 .1))
            .map(|(k, v)| (k.to_string(), *v))
        else {
            continue;
        };
        if top / total < DOMINANT_SHARE {
            run_model = None;
            continue;
        }
        let (_, entry) = acc
            .entry(key.clone())
            .or_insert_with(|| (name.to_string(), Acc::default()));
        entry.spend += total;
        entry.points += points;
        if run_model.as_deref() != Some(key.as_str()) {
            entry.runs += 1;
            run_model = Some(key);
        }
    }

    let mut out: Vec<ModelBudget> = acc
        .into_values()
        .filter_map(|(model, a)| {
            // Meters report whole percents, so each run is off by up to a
            // point at its ends; independent runs add up like a random walk.
            let err = f64::from(a.runs).sqrt();
            if a.points < MIN_POINTS || a.points <= 3.0 * err {
                return None;
            }
            Some(ModelBudget {
                model,
                usd: 100.0 * a.spend / a.points,
                low_usd: 100.0 * a.spend / (a.points + err),
                high_usd: 100.0 * a.spend / (a.points - err),
            })
        })
        .collect();
    out.sort_by(|x, y| x.usd.total_cmp(&y.usd));
    out
}

/// The published result for a bar's current window, computed right away when
/// there is none yet or the bar was out of the refresh. Registers the bar so
/// the refresh keeps it current.
#[tauri::command]
pub async fn get_plan_budget(
    provider: String,
    window_id: String,
    since: String,
    state: State<'_, AppState>,
) -> Result<Option<PlanBudget>, String> {
    if !["claude", "codex", "cursor", "kimi"].contains(&provider.as_str()) {
        return Err(format!("unknown provider {provider}"));
    }
    let since = parse_utc(&since).ok_or("invalid since")?;
    Ok(get_plan_budget_inner(&state, provider, window_id, since).await)
}

pub(crate) async fn get_plan_budget_inner(
    state: &AppState,
    provider: String,
    window_id: String,
    since: DateTime<Utc>,
) -> Option<PlanBudget> {
    if !state.usage_access_enabled.load(Ordering::SeqCst) {
        return None;
    }
    let key = (provider, window_id);
    let last_ask = lock(&JOBS).insert(
        key.clone(),
        Job {
            since,
            asked: Instant::now(),
        },
    );
    if last_ask.is_some_and(|job| job.asked.elapsed() < ASK_TTL) {
        if let Some(budget) = published(&key, since) {
            return Some(budget);
        }
    } else {
        // Out of the refresh for a while: its published result may be hours
        // old, and the page asks again only at its next reading.
        lock(&PUBLISHED).remove(&key);
    }
    // First ask, or a new window: answer now rather than at the next refresh.
    let _gate = state.compute.lock().await;
    // A request queued ahead of this one may have just computed it.
    if let Some(budget) = published(&key, since) {
        return Some(budget);
    }
    let budget = compute_blocking(state, key.0.clone(), vec![(key.1.clone(), since)])
        .await?
        .pop()?;
    lock(&PUBLISHED).insert(key, (since, budget.clone()));
    Some(budget)
}

/// Bars asked for within the last hour, with the window each was asked for;
/// the refresh keeps these current.
fn registered() -> Vec<(Key, DateTime<Utc>)> {
    let mut jobs = lock(&JOBS);
    // Bars nobody has looked at for a while stop costing anything.
    jobs.retain(|_, job| job.asked.elapsed() < ASK_TTL);
    jobs.iter().map(|(k, job)| (k.clone(), job.since)).collect()
}

/// Hold a refreshed result until the refresh publishes its whole set.
fn stage(key: Key, since: DateTime<Utc>, budget: PlanBudget) {
    lock(&STAGED).insert(key, (since, budget));
}

/// Publish every staged result at once. Bars not staged keep their result.
pub(crate) fn publish_staged() {
    let staged = std::mem::take(&mut *lock(&STAGED));
    let mut published = lock(&PUBLISHED);
    for (key, (since, budget)) in staged {
        // A result for an older window never replaces one for a newer window,
        // which an ask may have computed while this one was staged.
        let superseded = published
            .get(&key)
            .is_some_and(|(newer, _)| *newer > since && !within_slack(*newer, since));
        if !superseded {
            published.insert(key, (since, budget));
        }
    }
}

/// The published result for the window starting at `since`. Sources round a
/// reset differently, so a start within the slack counts as the same window.
fn published(key: &Key, since: DateTime<Utc>) -> Option<PlanBudget> {
    lock(&PUBLISHED)
        .get(key)
        .filter(|(published_since, _)| within_slack(*published_since, since))
        .map(|(_, budget)| budget.clone())
}

/// The registered bars by provider, less the providers whose bars are
/// published and were computed within the hour from inputs that still hold;
/// the refresh recomputes these.
pub(crate) fn due(parser: &UsageParser) -> Vec<(String, Bars)> {
    let mut by_provider: BTreeMap<String, Bars> = BTreeMap::new();
    for ((provider, window_id), since) in registered() {
        by_provider
            .entry(provider)
            .or_default()
            .push((window_id, since));
    }
    by_provider
        .into_iter()
        .filter(|(provider, bars)| !current(parser, provider, bars))
        .collect()
}

fn current(parser: &UsageParser, provider: &str, bars: &Bars) -> bool {
    let now = inputs(parser, provider);
    let computed = lock(&COMPUTED)
        .get(provider)
        .is_some_and(|(then, at)| *then == now && at.elapsed() < RECOMPUTE_EVERY);
    computed
        && bars.iter().all(|(window_id, since)| {
            published(&(provider.to_string(), window_id.clone()), *since).is_some()
        })
}

fn inputs(parser: &UsageParser, provider: &str) -> Inputs {
    (
        parser.data_version(provider),
        lock(&RECORDED).get(provider).copied().unwrap_or(0),
        crate::usage::pricing::pricing_epoch(),
    )
}

/// Recompute a provider's bars for the next publish, and note what from.
/// The caller holds the compute gate.
pub(crate) async fn refresh(state: &AppState, provider: String, bars: Bars) {
    // Read first: a change during the compute is left for the next one.
    let inputs = inputs(&state.parser, &provider);
    let Some(budgets) = compute_blocking(state, provider.clone(), bars.clone()).await else {
        return;
    };
    for ((window_id, since), budget) in bars.into_iter().zip(budgets) {
        stage((provider.clone(), window_id), since, budget);
    }
    lock(&COMPUTED).insert(provider, (inputs, Instant::now()));
}

/// Compute a provider's bars off the async runtime. The caller holds the
/// compute gate.
async fn compute_blocking(
    state: &AppState,
    provider: String,
    bars: Bars,
) -> Option<Vec<PlanBudget>> {
    let parser = state.parser.clone();
    tokio::task::spawn_blocking(move || compute(&parser, &provider, &bars))
        .await
        .ok()
}

fn lock<T: Default>(cell: &'static OnceLock<Mutex<T>>) -> std::sync::MutexGuard<'static, T> {
    cell.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// A provider's bars, from its entries and meter readings loaded once for
/// all of them.
fn compute(
    parser: &UsageParser,
    provider: &str,
    bars: &[(String, DateTime<Utc>)],
) -> Vec<PlanBudget> {
    let lookback = Utc::now() - Duration::days(LOOKBACK_DAYS);
    let start = bars
        .iter()
        .map(|(_, since)| *since)
        .fold(lookback, DateTime::min);
    let entries: Vec<_> = parser
        .priced_entries_since(provider, start.with_timezone(&Local))
        .into_iter()
        .map(|(t, name, key, usd)| (t.with_timezone(&Utc), name, key, usd))
        .collect();
    let samples: Vec<Sample> = match (provider, SAMPLES_FILE.get()) {
        // Cursor's first-party meter shares spend with its API pool; no clean stretches.
        ("cursor", _) => Vec::new(),
        ("codex", _) => codex_logs(parser.codex_dir(), lookback)
            .iter()
            .flat_map(|log| log.samples.iter().filter(|s| s.t >= lookback).cloned())
            .collect(),
        (_, Some(path)) => read_samples(path)
            .into_iter()
            .filter(|s| s.p == provider && s.t >= lookback)
            .collect(),
        (_, None) => Vec::new(),
    };
    bars.iter()
        .map(|(window_id, since)| {
            let mut window: Vec<Sample> = samples
                .iter()
                .filter(|s| &s.w == window_id)
                .cloned()
                .collect();
            window.sort_by_key(|s| s.t);
            PlanBudget {
                spend: entries.iter().filter(|e| e.0 >= *since).map(|e| e.3).sum(),
                models: calibrate(&window, &entries),
            }
        })
        .collect()
}

/// What one Codex session log holds of the `codex` meters.
struct CodexLog {
    len: u64,
    modified: SystemTime,
    /// A sample per meter per reading, in the log's order.
    samples: Vec<Sample>,
    /// The last reading whole, with when it was logged.
    last: Option<(DateTime<Utc>, serde_json::Value)>,
}

/// Per log path, reused while the log's length and mtime hold.
static CODEX_LOGS: OnceLock<Mutex<HashMap<PathBuf, Arc<CodexLog>>>> = OnceLock::new();

/// Codex writes its meters into every `token_count` event, so its readings
/// come straight from the session logs: exact times and full history. The
/// logs last written at or after `since`; each is read again only once it
/// has changed.
fn codex_logs(dir: &Path, since: DateTime<Utc>) -> Vec<Arc<CodexLog>> {
    let lookback = Utc::now() - Duration::days(LOOKBACK_DAYS);
    let mut files = Vec::new();
    // A directory listing on Windows can show a log Codex still has open at
    // its size and mtime of minutes ago, so it only cuts at the lookback and
    // each log is stat'd itself.
    collect_jsonl_since(dir, since.min(lookback), &mut files, 0);
    let mut cache = lock(&CODEX_LOGS);
    cache.retain(|_, log| DateTime::<Utc>::from(log.modified) >= lookback);
    files
        .into_iter()
        .filter_map(|path| {
            let meta = fs::metadata(&path).ok()?;
            let modified = meta.modified().ok()?;
            if DateTime::<Utc>::from(modified) < since {
                return None;
            }
            let cached = cache.get(&path);
            if let Some(log) =
                cached.filter(|log| log.len == meta.len() && log.modified == modified)
            {
                return Some(log.clone());
            }
            let log = Arc::new(read_codex_log(&path, meta.len(), modified)?);
            cache.insert(path, log.clone());
            Some(log)
        })
        .collect()
}

/// The newest reading of the `codex` meters Codex logged at or after
/// `since`, with when it was logged.
pub(crate) fn latest_codex_reading(
    dir: &Path,
    since: DateTime<Utc>,
) -> Option<(DateTime<Utc>, serde_json::Value)> {
    let logs = codex_logs(dir, since);
    logs.iter()
        .filter_map(|log| log.last.as_ref())
        .filter(|(t, _)| *t >= since)
        .max_by_key(|(t, _)| *t)
        .cloned()
}

fn read_codex_log(path: &Path, len: u64, modified: SystemTime) -> Option<CodexLog> {
    let file = fs::File::open(path).ok()?;
    let mut log = CodexLog {
        len,
        modified,
        samples: Vec::new(),
        last: None,
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if !line.contains("rate_limits") {
            continue;
        }
        let Some((t, rl)) = serde_json::from_str(&line).ok().and_then(codex_reading) else {
            continue;
        };
        log.samples.extend(codex_samples(t, &rl));
        log.last = Some((t, rl));
    }
    Some(log)
}

fn collect_jsonl_since(dir: &Path, since: DateTime<Utc>, out: &mut Vec<PathBuf>, depth: u32) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() && depth < 4 {
            collect_jsonl_since(&path, since, out, depth + 1);
        } else if path.extension().is_some_and(|ext| ext == "jsonl")
            && meta
                .modified()
                .is_ok_and(|m| DateTime::<Utc>::from(m) >= since)
        {
            out.push(path);
        }
    }
}

/// A log line's reading of the `codex` meters: when it was logged, and its
/// `rate_limits`. Other limit ids (e.g. "premium") are separate meters.
fn codex_reading(mut line: serde_json::Value) -> Option<(DateTime<Utc>, serde_json::Value)> {
    let t = parse_utc(line.get("timestamp")?.as_str()?)?;
    let rl = line.get_mut("payload")?.get_mut("rate_limits")?.take();
    let limit_id = rl.get("limit_id").and_then(|id| id.as_str());
    if !rl.is_object() || limit_id.is_some_and(|id| id != "codex") {
        return None;
    }
    Some((t, rl))
}

/// A sample per meter in a reading.
fn codex_samples(t: DateTime<Utc>, rl: &serde_json::Value) -> impl Iterator<Item = Sample> + '_ {
    rl.as_object()
        .into_iter()
        .flatten()
        .filter_map(move |(window_id, window)| {
            Some(Sample {
                t,
                p: "codex".into(),
                w: window_id.clone(),
                u: window.get("used_percent")?.as_f64()?,
                r: window
                    .get("resets_at")
                    .and_then(|s| s.as_i64())
                    .and_then(|s| DateTime::from_timestamp(s, 0)),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-20T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
            + Duration::minutes(h * 30)
    }

    fn sample(h: i64, u: f64) -> Sample {
        Sample {
            t: at(h),
            p: "claude".into(),
            w: "five_hour".into(),
            u,
            r: Some(at(100)),
        }
    }

    fn spend(h: i64, model: &str, usd: f64) -> (DateTime<Utc>, String, String, f64) {
        (
            at(h) + Duration::minutes(1),
            model.into(),
            model.into(),
            usd,
        )
    }

    #[test]
    fn learns_each_models_dollars_per_point() {
        // Opus: $3 per 5 points → $60 per window. Sonnet: $5 per 5 → $100.
        let samples: Vec<Sample> = (0..9).map(|h| sample(h, h as f64 * 5.0)).collect();
        let mut entries: Vec<_> = (0..4).map(|h| spend(h, "opus", 3.0)).collect();
        entries.extend((4..8).map(|h| spend(h, "sonnet", 5.0)));
        let got = calibrate(&samples, &entries);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].model, "opus");
        assert!((got[0].usd - 60.0).abs() < 1e-9);
        assert!(got[0].low_usd < 60.0 && got[0].high_usd > 60.0);
        assert!((got[1].usd - 100.0).abs() < 1e-9);
    }

    #[test]
    fn skips_mixed_foreign_and_reset_stretches() {
        let mut samples: Vec<Sample> = (0..6).map(|h| sample(h, h as f64 * 5.0)).collect();
        samples[5].r = Some(at(200)); // new window
        let entries = vec![
            spend(0, "opus", 1.0),
            spend(0, "sonnet", 1.0), // 50/50: mixed
            // stretch 1: nothing local, meter moved: usage elsewhere
            spend(2, "opus", 3.0),
            spend(3, "opus", 3.0),
            spend(4, "opus", 99.0), // straddles the reset
        ];
        // Only stretches 2 and 3 count: $6 over 10 points.
        let got = calibrate(&samples, &entries);
        assert_eq!(got.len(), 1);
        assert!((got[0].usd - 60.0).abs() < 1e-9);
    }

    #[test]
    fn reads_codex_meters_from_token_count_events() {
        let line = |limit_id: &str| {
            serde_json::json!({
                "timestamp": "2026-09-11T20:09:44.351Z",
                "payload": {"type": "token_count", "rate_limits": {
                    "limit_id": limit_id,
                    "primary": {"used_percent": 6.0, "window_minutes": 300, "resets_at": 1789174877},
                    "secondary": {"used_percent": 30.0, "window_minutes": 10080, "resets_at": 1789447675}
                }}
            })
        };
        let (t, rl) = codex_reading(line("codex")).unwrap();
        let samples: Vec<Sample> = codex_samples(t, &rl).collect();
        assert_eq!(samples.len(), 2, "one per meter");
        let s = samples.iter().find(|s| s.w == "secondary").unwrap();
        assert_eq!(s.u, 30.0);
        assert_eq!(s.t.to_rfc3339(), "2026-09-11T20:09:44.351+00:00");
        assert_eq!(s.r.unwrap().timestamp(), 1789447675);
        assert!(codex_reading(line("premium")).is_none());
    }

    #[test]
    fn a_codex_log_is_read_again_only_once_it_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let line = |used: f64| {
            serde_json::json!({
                "timestamp": Utc::now().to_rfc3339(),
                "payload": {"type": "token_count", "rate_limits": {
                    "limit_id": "codex",
                    "primary": {"used_percent": used, "window_minutes": 300, "resets_at": 1789174877}
                }}
            })
            .to_string()
                + "\n"
        };
        fs::write(&path, line(5.0)).unwrap();
        let since = Utc::now() - Duration::hours(1);
        let first = codex_logs(dir.path(), since);
        assert_eq!(first.len(), 1);
        assert!(
            Arc::ptr_eq(&first[0], &codex_logs(dir.path(), since)[0]),
            "unchanged: reused"
        );

        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(line(7.0).as_bytes())
            .unwrap();
        let grown = codex_logs(dir.path(), since);
        assert_eq!(grown[0].samples.len(), 2, "grown: read again");
        let (_, last) = latest_codex_reading(dir.path(), since).unwrap();
        assert_eq!(last["primary"]["used_percent"], 7.0);
    }

    #[test]
    #[ignore = "reads this machine's Codex logs"]
    fn live_codex_budgets() {
        use std::time::Instant;
        let parser = crate::usage::parser::UsageParser::new();
        let since = Utc::now() - Duration::days(LOOKBACK_DAYS);
        let priced = |provider: &str| -> Vec<_> {
            parser
                .priced_entries_since(provider, since.with_timezone(&Local))
                .into_iter()
                .map(|(t, name, key, usd)| (t.with_timezone(&Utc), name, key, usd))
                .collect()
        };
        for provider in ["claude", "codex"] {
            let t = Instant::now();
            let n = priced(provider).len();
            let cold = t.elapsed();
            let t = Instant::now();
            priced(provider);
            println!(
                "{provider}: {n} entries, priced cold {cold:?}, warm {:?}",
                t.elapsed()
            );
        }
        let entries = priced("codex");
        for window in ["primary", "secondary"] {
            let t = Instant::now();
            let mut samples: Vec<Sample> = codex_logs(parser.codex_dir(), since)
                .iter()
                .flat_map(|log| log.samples.iter().filter(|s| s.w == window).cloned())
                .collect();
            let read = t.elapsed();
            samples.sort_by_key(|s| s.t);
            let t = Instant::now();
            let got = calibrate(&samples, &entries);
            println!(
                "{window}: {} readings, log read {read:?}, calibrate {:?} → {got:?}",
                samples.len(),
                t.elapsed()
            );
        }
    }

    #[test]
    fn needs_ten_points_before_answering() {
        let samples: Vec<Sample> = (0..3).map(|h| sample(h, h as f64 * 4.0)).collect();
        let entries = vec![spend(0, "opus", 1.0), spend(1, "opus", 1.0)];
        assert!(calibrate(&samples, &entries).is_empty());
    }

    fn budget(spend: f64) -> PlanBudget {
        PlanBudget {
            spend,
            models: Vec::new(),
        }
    }

    fn published_spend(key: &Key) -> Option<(DateTime<Utc>, f64)> {
        lock(&PUBLISHED)
            .get(key)
            .map(|(since, budget)| (*since, budget.spend))
    }

    /// A bar the page asked for just now, with what the last cycle published.
    fn refreshed(key: &Key, since: DateTime<Utc>, spend: f64) {
        let asked = Instant::now();
        lock(&JOBS).insert(key.clone(), Job { since, asked });
        lock(&PUBLISHED).insert(key.clone(), (since, budget(spend)));
    }

    // The statics are shared by every test in the crate, so each test uses
    // keys of its own.
    #[test]
    fn publish_staged_merges_per_key_and_never_regresses() {
        let kept: Key = ("test-publish-kept".into(), "w".into());
        let moved: Key = ("test-publish-moved".into(), "w".into());
        let newer: Key = ("test-publish-newer".into(), "w".into());
        stage(kept.clone(), at(0), budget(1.0));
        stage(moved.clone(), at(0), budget(2.0));
        stage(newer.clone(), at(10), budget(3.0));
        publish_staged();

        // The next cycle restages only some bars, one for an older window.
        stage(moved.clone(), at(1), budget(20.0));
        stage(newer.clone(), at(0), budget(30.0));
        publish_staged();

        assert_eq!(
            published_spend(&kept),
            Some((at(0), 1.0)),
            "unstaged keys survive"
        );
        assert_eq!(published_spend(&moved), Some((at(1), 20.0)));
        assert_eq!(
            published_spend(&newer),
            Some((at(10), 3.0)),
            "an older window never replaces a newer one"
        );
    }

    /// A parser over a temp dir holding one Claude request at each time.
    fn claude_state(times: &[DateTime<Utc>]) -> (tempfile::TempDir, AppState) {
        let dir = tempfile::TempDir::new().unwrap();
        let claude_dir = dir.path().join("claude");
        let codex_dir = dir.path().join("codex");
        fs::create_dir_all(&claude_dir).unwrap();
        let lines: Vec<String> = times
            .iter()
            .enumerate()
            .map(|(i, t)| {
                format!(
                    r#"{{"type":"assistant","timestamp":"{}","requestId":"req_{i}","message":{{"id":"msg_{i}","model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":1000,"output_tokens":500}},"stop_reason":"end_turn"}}}}"#,
                    t.to_rfc3339()
                )
            })
            .collect();
        fs::write(claude_dir.join("session.jsonl"), lines.join("\n")).unwrap();
        let mut state = AppState::new();
        state.usage_access_enabled.store(true, Ordering::SeqCst);
        state.parser = std::sync::Arc::new(UsageParser::with_dirs(claude_dir, codex_dir));
        (dir, state)
    }

    fn priced_since(state: &AppState, since: DateTime<Utc>) -> f64 {
        state
            .parser
            .priced_entries_since("claude", since.with_timezone(&Local))
            .iter()
            .map(|e| e.3)
            .sum()
    }

    #[tokio::test]
    async fn unseen_bar_is_computed_inline() {
        let now = Utc::now();
        let (_dir, state) = claude_state(&[now - Duration::hours(1)]);
        let since = now - Duration::hours(5);
        let key: Key = ("claude".into(), "test-unseen".into());

        let got = get_plan_budget_inner(&state, key.0.clone(), key.1.clone(), since)
            .await
            .expect("the first ask is answered, not left empty");
        let fixture = priced_since(&state, since);
        assert!(fixture > 0.0, "guard: the fixture has spend");
        assert!((got.spend - fixture).abs() < 1e-12);
        assert_eq!(published_spend(&key), Some((since, got.spend)));
        assert!(
            registered().contains(&(key, since)),
            "the cycle refreshes it"
        );
    }

    #[tokio::test]
    async fn new_window_since_recomputes() {
        let now = Utc::now();
        let (_dir, state) = claude_state(&[now - Duration::hours(4), now - Duration::hours(1)]);
        let (old, new) = (now - Duration::hours(5), now - Duration::hours(2));
        let key: Key = ("claude".into(), "test-new-window".into());
        refreshed(&key, old, 99.0);

        let same = get_plan_budget_inner(&state, key.0.clone(), key.1.clone(), old).await;
        assert_eq!(same.map(|b| b.spend), Some(99.0), "same window: published");

        let got = get_plan_budget_inner(&state, key.0.clone(), key.1.clone(), new)
            .await
            .expect("a new window is computed inline");
        let fixture = priced_since(&state, new);
        assert!(
            fixture > 0.0 && fixture < priced_since(&state, old),
            "guard: only the later request is in the new window"
        );
        assert!((got.spend - fixture).abs() < 1e-12);
        assert_eq!(published_spend(&key), Some((new, got.spend)));
    }

    #[tokio::test]
    async fn reset_rounding_is_the_same_window() {
        let now = Utc::now();
        let (_dir, state) = claude_state(&[now - Duration::hours(1)]);
        let since = now - Duration::hours(5);
        // The CLI probe and the statusline round the same reset a minute apart.
        let rounded = since - Duration::seconds(60);
        let key: Key = ("claude".into(), "test-rounding".into());
        refreshed(&key, since, 99.0);
        assert!(
            priced_since(&state, rounded) > 0.0,
            "guard: a compute differs"
        );

        for _ in 0..2 {
            let got = get_plan_budget_inner(&state, key.0.clone(), key.1.clone(), rounded).await;
            assert_eq!(got.map(|b| b.spend), Some(99.0), "served from PUBLISHED");
        }
        // The cycle recomputes the start it was last asked for.
        stage(key.clone(), rounded, budget(5.0));
        publish_staged();
        assert_eq!(published_spend(&key), Some((rounded, 5.0)));
    }

    #[test]
    fn a_reading_is_dated_when_it_was_taken() {
        let now = at(10);
        assert_eq!(
            reading_time(&at(4).to_rfc3339(), now),
            at(4),
            "a statusline reading"
        );
        assert_eq!(
            reading_time(&at(12).to_rfc3339(), now),
            now,
            "never in the future"
        );
        assert_eq!(reading_time("", now), now, "none given");
    }

    #[test]
    fn spend_is_dated_by_request_not_by_archived_hour() {
        use crate::usage::archive::ArchiveManager;
        use chrono::Timelike;
        // Two requests in an hour the archive has since taken.
        let hour = Local::now() - Duration::hours(3);
        let hour = hour - Duration::seconds(i64::from(hour.num_seconds_from_midnight() % 3600));
        let hour = hour.with_nanosecond(0).unwrap();
        let (early, late) = (hour + Duration::minutes(10), hour + Duration::minutes(40));
        let (dir, state) = claude_state(&[early.with_timezone(&Utc), late.with_timezone(&Utc)]);
        let archive = ArchiveManager::new(dir.path());
        let (entries, _, _) = state.parser.load_entries("claude", None);
        let now = Local::now();
        let archived = archive.archive_completed_hours(
            &entries,
            "local:claude",
            "claude",
            now.date_naive(),
            now.hour() as u8,
        );
        assert_eq!(archived, 1, "guard: the hour is archived");
        state.parser.set_archive(archive);
        let (merged, _, _) = state.parser.load_entries("claude", Some(hour.date_naive()));
        assert_eq!(
            merged.iter().map(|e| e.timestamp).collect::<Vec<_>>(),
            vec![hour],
            "guard: the views read the hour as one row at its top"
        );

        let rows = state.parser.priced_entries_since("claude", hour);
        assert_eq!(
            rows.iter().map(|r| r.0).collect::<Vec<_>>(),
            vec![early, late],
            "each request at its own time"
        );
        // A window that opened between the two holds only the later one.
        let since = (hour + Duration::minutes(30)).with_timezone(&Utc);
        let got = &compute(&state.parser, "claude", &[("test-archived".into(), since)])[0];
        assert!(rows[1].3 > 0.0, "guard: the request is priced");
        assert!((got.spend - rows[1].3).abs() < 1e-12, "spend {}", got.spend);
    }

    #[tokio::test]
    async fn bar_out_of_the_refresh_is_recomputed() {
        let now = Utc::now();
        let (_dir, state) = claude_state(&[now - Duration::hours(1)]);
        let since = now - Duration::hours(5);
        let key: Key = ("claude".into(), "test-dropped".into());
        // Published before the bar went unasked for ASK_TTL and left JOBS.
        lock(&PUBLISHED).insert(key.clone(), (since, budget(99.0)));

        let got = get_plan_budget_inner(&state, key.0.clone(), key.1.clone(), since)
            .await
            .expect("answered");
        let fixture = priced_since(&state, since);
        assert!(fixture > 0.0, "guard: the fixture has spend");
        assert!((got.spend - fixture).abs() < 1e-12, "not the old result");
        assert_eq!(published_spend(&key), Some((since, got.spend)));
        assert!(
            registered().contains(&(key, since)),
            "the cycle refreshes it"
        );
    }

    #[tokio::test]
    async fn a_provider_is_recomputed_only_once_its_inputs_move() {
        // Codex, which no other test registers: the statics are shared.
        let dir = tempfile::TempDir::new().unwrap();
        let codex_dir = dir.path().join("codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let log = codex_dir.join("rollout.jsonl");
        fs::write(&log, "{}\n").unwrap();
        let mut state = AppState::new();
        state.usage_access_enabled.store(true, Ordering::SeqCst);
        state.parser = Arc::new(UsageParser::with_dirs(dir.path().join("claude"), codex_dir));
        let since = Utc::now() - Duration::hours(5);
        get_plan_budget_inner(&state, "codex".into(), "test-due".into(), since).await;
        let codex_due = || due(&state.parser).iter().any(|(p, _)| p == "codex");
        assert!(codex_due(), "never refreshed");

        let epoch = crate::usage::pricing::pricing_epoch();
        refresh(&state, "codex".into(), vec![("test-due".into(), since)]).await;
        publish_staged();
        // Another test setting prices meanwhile rightly makes it due.
        assert!(
            !codex_due() || crate::usage::pricing::pricing_epoch() != epoch,
            "nothing it depends on moved"
        );

        OpenOptions::new()
            .append(true)
            .open(&log)
            .unwrap()
            .write_all(b"{}\n")
            .unwrap();
        assert!(
            state.parser.invalidate_if_changed(),
            "guard: the sweep sees it"
        );
        assert!(codex_due(), "its logs changed");
    }
}
