//! The periodic refresh. One loop owns every periodic job: each cycle takes
//! one sample of the usage logs, computes what is shown from it one job at a
//! time, and publishes it all at once, so the tray, the float ball and the
//! popover always show the same sample.

use crate::commands::tray::{
    enabled_integrations, patch_tray_utilization, tray_utilization_from_rate_limits,
};
use crate::commands::usage_query::view_built_on_appended_logs;
use crate::commands::AppState;
use crate::rate_limits::{merge_rate_limits, RateLimitSelection};
use crate::usage::integrations::UsageIntegrationId;
use crate::usage::litellm::DynamicModelRates;
use crate::usage::parser::LogChanges;
use chrono::{DateTime, Local, NaiveDateTime};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

/// How long the loop waits at launch for the frontend's settings.
const SETTINGS_WAIT: Duration = Duration::from_secs(10);
/// Longest the rate-limit probes may run before the providers not reached yet
/// keep their cached value; half the interval when that is shorter.
const PROBE_BUDGET_SECS: u64 = 20;
/// The SSH hosts sync every this many cycles (5 min at 30 s, 50 at 300 s).
const SSH_SYNC_EVERY_N_CYCLES: u64 = 10;
/// How long a host that failed for good (its host key) is left out of the
/// background sync; the hold doubles on each repeat, up to a day.
const SSH_HOLD_FIRST: Duration = Duration::from_secs(3600);
const SSH_HOLD_MAX: Duration = Duration::from_secs(24 * 3600);
/// How often the price and exchange-rate tables' TTLs are checked.
const PRICING_CHECK_EVERY: Duration = Duration::from_secs(3600);
/// A cycle plans the next tick from this long after it started, so a tick
/// that comes while it runs is run late, while the tick it ran for is never
/// run twice.
const TICK_GUARD: Duration = Duration::from_secs(1);
/// Longest single wait for a tick before the wall clock is checked again.
const MAX_WAIT: Duration = Duration::from_secs(300);
/// The last stretch before a tick in which no slot job starts.
const SLOT_GUARD: Duration = Duration::from_secs(1);
/// While the interval is Off, a cycle's slot jobs run within this long of it.
const OFF_SLOT_WINDOW: Duration = Duration::from_secs(60);
/// While the interval is Off, a look at the popover refreshes a sample at
/// least this old.
const FOCUS_REFRESH_AGE: chrono::TimeDelta = chrono::TimeDelta::seconds(30);
/// A sample sweeps every file this often, in a new day, at a user's request
/// and after the popover was shown; the others stat only the files written
/// within a week.
const FULL_SWEEP_EVERY: chrono::TimeDelta = chrono::TimeDelta::hours(1);

#[derive(Default)]
pub(crate) struct RefreshState {
    /// Bumped once per sample. A rolling 5h view's key carries it, so its
    /// clock-driven window is recomputed once per refresh and reused between.
    pub(crate) generation: AtomicU64,
    /// When the last sample ran; every usage view computed since is stamped
    /// with it as its `last_updated`.
    pub(crate) last_sample: Mutex<Option<DateTime<Local>>>,
    /// The (provider, period, offset) the popover last asked for in the
    /// foreground; background warm-ups leave it alone.
    pub(crate) active_view: Mutex<Option<(String, String, i32)>>,
    /// The day cost the tray and float ball show, as last published.
    pub(crate) tray_cost: Mutex<Option<f64>>,
    /// Bumped, under the `tray_cost` lock, by every publish outside a cycle's
    /// own: the cycle keeps a cost published after its Tray step.
    pub(crate) tray_cost_version: AtomicU64,
    /// The archive changed where the sweep cannot see it; the next sample
    /// drops the views built from it.
    pub(crate) pending_change: AtomicBool,
    /// A user action asked for a cycle now.
    pub(crate) requested: AtomicBool,
    /// The next sample sweeps every file: a user action or a look asked.
    pub(crate) full_sweep: AtomicBool,
    /// Wakes the loop: to run a requested cycle, or to re-plan its ticks after
    /// the interval changed.
    pub(crate) wake: Notify,
    /// The frontend has pushed the settings the first cycle depends on.
    pub(crate) settings_ready: Notify,
    /// Price and exchange-rate tables fetched since the last sample. The next
    /// sample applies them, so no value moves between refreshes.
    pub(crate) pending_pricing: Mutex<Option<HashMap<String, DynamicModelRates>>>,
    pub(crate) pending_fx: Mutex<Option<HashMap<String, f64>>>,
    /// An SSH sync is running.
    pub(crate) ssh_inflight: AtomicBool,
    /// Held around each SSH host's sync, background or manual: two syncs of
    /// one host would write the same temp file and clobber each other.
    pub(crate) ssh_sync: tokio::sync::Mutex<()>,
    /// SSH hosts the background sync holds back after a failure only the user
    /// can fix: alias → (no sync before, by the wall clock so a sleep counts,
    /// the hold that set it). A manual sync or a passing test of the host, or
    /// any SSH config change, lifts the hold.
    pub(crate) ssh_holds: Mutex<SshHolds>,
    /// When the price tables' TTLs were last checked.
    pub(crate) last_pricing_check: Mutex<Option<Instant>>,
    /// A price-table fetch is running.
    pub(crate) pricing_inflight: AtomicBool,
    /// The generation of the last sample that swept every file, and the time
    /// the sweep started; one taken while usage access was off sweeps nothing.
    pub(crate) swept: Mutex<Option<(u64, DateTime<Local>)>>,
    /// A cycle is running, and will publish a fresh sample.
    pub(crate) cycle_running: AtomicBool,
    /// An auto-export pass is reading or writing its folder, possibly still
    /// after its caller stopped waiting.
    pub(crate) export_folder_busy: AtomicBool,
}

/// Ask the loop for a cycle now; it realigns to its ticks afterwards.
pub(crate) fn request_refresh(state: &AppState) {
    // A user action: whatever it shows should include Cursor's latest, and
    // every log.
    wake_cursor(state);
    state.refresh.full_sweep.store(true, Ordering::SeqCst);
    state.refresh.requested.store(true, Ordering::SeqCst);
    state.refresh.wake.notify_one();
}

/// The popover is shown: the next cycle brings Cursor's latest and sweeps
/// every log, so a resumed old session, which appends to a file the other
/// samples skip, shows up within a refresh of a look.
pub(crate) fn popover_shown(state: &AppState) {
    wake_cursor(state);
    state.refresh.full_sweep.store(true, Ordering::SeqCst);
}

/// Cursor may be in use (the user is looking, acting, or the IDE wrote its
/// state): end the idle back-off of its remote data and its meters.
pub(crate) fn wake_cursor(state: &AppState) {
    state.parser.reset_cursor_remote_ttl();
    crate::rate_limits::reset_cursor_refetch_floor();
}

/// Sweep the logs for a user action that archives outside a cycle (a manual
/// export or an import). The archive trusts the file cache, which between
/// samples may predate what was logged since: archiving from it would move
/// the frontier past those rows and hide them for good. A change the sweep
/// finds drops the views built without it now, and a refresh is requested to
/// publish it. Returns when the sweep started, the archive's horizon. The
/// caller holds the compute gate.
pub(crate) async fn sweep_before_user_archive(state: &AppState) -> DateTime<Local> {
    let swept_at = Local::now();
    if !state.parser.invalidate_if_changed() {
        return swept_at;
    }
    // Disk first: the disk clear waits out the disk hits in flight, and the
    // memory clear after it drops the copies they made.
    state.clear_payload_disk_cache().await;
    state.parser.clear_payload_cache();
    // The sweep took the change the next sample would have found.
    state.refresh.pending_change.store(true, Ordering::SeqCst);
    request_refresh(state);
    swept_at
}

/// The frontend has pushed its settings: the first cycle need not wait any
/// longer for them.
#[tauri::command]
pub async fn refresh_ready(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.refresh.settings_ready.notify_one();
    Ok(())
}

/// The popover gained focus. While the interval is Off nothing refreshes on
/// its own, so a look at data at least [`FOCUS_REFRESH_AGE`] old refreshes
/// it, unless a cycle already running will; with an interval set, the ticks
/// keep it fresh.
#[tauri::command]
pub async fn refresh_on_focus(state: tauri::State<'_, AppState>) -> Result<(), String> {
    refresh_if_looked_at(&state).await;
    Ok(())
}

async fn refresh_if_looked_at(state: &AppState) {
    if *state.refresh_interval.read().await != 0
        || state.refresh.cycle_running.load(Ordering::SeqCst)
    {
        return;
    }
    let last_sample = state.refresh.last_sample.lock().ok().and_then(|last| *last);
    if last_sample.is_none_or(|at| Local::now() - at >= FOCUS_REFRESH_AGE) {
        request_refresh(state);
    }
}

/// A claimed single-flight flag, released however its holder ends.
pub(crate) struct InFlight<'a>(&'a AtomicBool);

impl<'a> InFlight<'a> {
    /// Claim `flag`, or `None` while another holder has it.
    pub(crate) fn claim(flag: &'a AtomicBool) -> Option<Self> {
        if flag.swap(true, Ordering::SeqCst) {
            // Build no guard: dropping one would release the holder's claim.
            None
        } else {
            Some(Self(flag))
        }
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// One step of a refresh cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// First cycle only: purge the duplicate device sources older builds left.
    Cleanup,
    /// Refresh today's Cursor remote data once its TTL has lapsed.
    CursorToday,
    /// Probe the rate limits, one provider at a time.
    RateLimits,
    /// The one invalidation point: sweep the logs and drop what changed.
    Sample,
    /// Compute the day cost the tray and float ball show.
    Tray,
    /// Recompute the view the popover shows.
    ActiveView,
    /// Make it all visible at once.
    Publish,
}

/// The steps of a cycle, in order. The I/O comes before the sample, so all
/// that is computed after it sees the new Cursor data and 5h reset, and
/// nothing invalidates between the sample and the publish. The first cycle's
/// cleanup takes the compute gate only after the I/O, which then runs beside
/// whatever holds the gate at launch.
pub(crate) fn cycle_steps(first: bool) -> Vec<Step> {
    let mut steps = Vec::with_capacity(7);
    steps.extend([Step::CursorToday, Step::RateLimits]);
    if first {
        steps.push(Step::Cleanup);
    }
    steps.extend([Step::Sample, Step::Tray, Step::ActiveView, Step::Publish]);
    steps
}

/// The refresh loop: cycle 0 at launch, then a cycle at every aligned tick
/// (local midnight + 1 s + k * interval, or only 00:00:01 while the interval
/// is Off) and whenever a user action requests one.
pub(crate) async fn run(app: AppHandle) {
    let state = app.state::<AppState>();
    // Only the sample revalidates the session-file listings; queries between
    // samples reuse what was listed.
    state.parser.set_listings_frozen(true);
    let interval = *state.refresh_interval.read().await;
    state
        .parser
        .set_payload_ttl_secs(crate::usage::parser::payload_ttl_for(interval));

    // The first cycle should see the settings the frontend pushes at
    // bootstrap: usage access, the enabled integrations, the rate-limit flag.
    if tokio::time::timeout(SETTINGS_WAIT, state.refresh.settings_ready.notified())
        .await
        .is_err()
    {
        tracing::warn!("Refresh: no settings after {SETTINGS_WAIT:?}, starting anyway");
    }
    tracing::info!("Refresh loop started");

    let mut plan_from = Moment::now().later(TICK_GUARD);
    // A cycle serves every request made before it starts.
    state.refresh.requested.store(false, Ordering::SeqCst);
    let generation = run_cycle(Some(&app), &state, true).await;
    run_slots(&app, &state, plan_from, generation).await;

    let mut cycles_since_ssh: u64 = 0;
    loop {
        let interval = *state.refresh_interval.read().await;
        let tick = next_tick(plan_from, interval);
        if !cycle_due(&state, tick).await {
            // The interval changed: re-plan against its grid, still from the
            // last cycle, so a tick that fell due meanwhile runs at once.
            continue;
        }
        let aligned = tick.is_due(Moment::now());
        plan_from = Moment::now().later(TICK_GUARD);
        state.refresh.requested.store(false, Ordering::SeqCst);
        let generation = run_cycle(Some(&app), &state, false).await;
        // Requested cycles count only while the interval is Off: they are
        // then the only cycles there are.
        if aligned || interval == 0 {
            cycles_since_ssh += 1;
            if cycles_since_ssh == SSH_SYNC_EVERY_N_CYCLES {
                cycles_since_ssh = 0;
                kick_ssh(&app);
            }
        }
        run_slots(&app, &state, plan_from, generation).await;
    }
}

/// A point in time on both clocks: the wall clock the ticks align to, and the
/// monotonic one timers run on.
#[derive(Clone, Copy)]
struct Moment {
    wall: DateTime<Local>,
    mono: Instant,
}

impl Moment {
    fn now() -> Self {
        Self {
            wall: Local::now(),
            mono: Instant::now(),
        }
    }

    fn later(self, by: Duration) -> Self {
        Self {
            wall: self.wall + chrono::TimeDelta::from_std(by).unwrap_or(chrono::TimeDelta::zero()),
            mono: self.mono + by,
        }
    }
}

/// A planned tick on both clocks. It is due once either clock reaches it: the
/// monotonic clock can stand still while the machine sleeps, and the wall
/// clock catches the tick it slept through. The wall time is local and naive,
/// like the grid, so 00:00:01 stays 00:00:01 across a DST change.
#[derive(Clone, Copy)]
struct Tick {
    wall: NaiveDateTime,
    mono: Instant,
}

impl Tick {
    fn is_due(&self, now: Moment) -> bool {
        now.mono >= self.mono || now.wall.naive_local() >= self.wall
    }
}

/// The first aligned tick after `from`.
fn next_tick(from: Moment, interval_secs: u64) -> Tick {
    let period = if interval_secs == 0 {
        crate::SECS_PER_DAY as u64
    } else {
        interval_secs
    };
    let after = Duration::from_secs_f64(crate::secs_until_next_refresh(from.wall, period));
    Tick {
        wall: from.wall.naive_local() + chrono::TimeDelta::from_std(after).unwrap_or_default(),
        mono: from.mono + after,
    }
}

/// Wait for `tick` or a wake. True when a cycle is due now: the tick came
/// (at once for one that passed while the last cycle ran), or a refresh was
/// requested. False when the wake only changed the interval.
async fn cycle_due(state: &AppState, tick: Tick) -> bool {
    loop {
        // A request whose wake the slots took ends them and is left here.
        if state.refresh.requested.swap(false, Ordering::SeqCst) {
            return true;
        }
        let now = Moment::now();
        if tick.is_due(now) {
            return true;
        }
        let wait = tick.mono.saturating_duration_since(now.mono).min(MAX_WAIT);
        if tokio::time::timeout(wait, state.refresh.wake.notified())
            .await
            .is_ok()
        {
            return state.refresh.requested.swap(false, Ordering::SeqCst);
        }
    }
}

/// What a sample found, for the publish.
#[derive(Default)]
struct Sampled {
    generation: u64,
    changed: bool,
    fx_applied: bool,
}

/// Run one refresh cycle. The first also purges duplicate devices and drops
/// whatever a previous session left in the payload caches. Without an `app`
/// nothing is painted or emitted. Returns the cycle's generation.
pub(crate) async fn run_cycle(app: Option<&AppHandle>, state: &AppState, first: bool) -> u64 {
    // Only the loop runs cycles, one at a time; the claim just marks it.
    let _running = InFlight::claim(&state.refresh.cycle_running);
    let cycle_t0 = Instant::now();
    let mut timings = [Duration::ZERO; 7];
    let mut cursor_changed = false;
    let mut sampled = Sampled::default();
    let mut cost = 0.0;
    let mut tray_version = 0;
    for step in cycle_steps(first) {
        let step_t0 = Instant::now();
        match step {
            Step::Cleanup => {
                let _gate = state.compute.lock().await;
                if crate::cleanup_duplicate_devices(state).await {
                    state.refresh.pending_change.store(true, Ordering::SeqCst);
                }
            }
            Step::CursorToday => cursor_changed = refresh_cursor_today(state).await,
            Step::RateLimits => refresh_rate_limits(state).await,
            Step::Sample => sampled = sample(state, first, cursor_changed).await,
            Step::Tray => {
                let _gate = state.compute.lock().await;
                tray_version = state.refresh.tray_cost_version.load(Ordering::SeqCst);
                cost = crate::commands::tray::current_daily_total_cost_if_allowed(state);
            }
            Step::ActiveView => refresh_active_view(app, state).await,
            Step::Publish => publish(app, state, cost, tray_version, &sampled).await,
        }
        timings[step as usize] = step_t0.elapsed();
    }
    let took = |step: Step| timings[step as usize];
    tracing::info!(
        "[PROFILE] cycle gen={} cursor={:?} rl={:?} sample={:?} tray={:?} view={:?} total={:?} changed={}",
        sampled.generation,
        took(Step::CursorToday),
        took(Step::RateLimits),
        took(Step::Sample),
        took(Step::Tray),
        took(Step::ActiveView),
        cycle_t0.elapsed(),
        sampled.changed,
    );
    sampled.generation
}

/// Refresh the Cursor remote data from yesterday on, once its TTL has lapsed,
/// when Cursor counts. Returns whether the stored data changed.
async fn refresh_cursor_today(state: &AppState) -> bool {
    if !state.usage_access_enabled()
        || !enabled_integrations(state).contains(&UsageIntegrationId::Cursor)
    {
        return false;
    }
    if crate::usage::cursor_parser::cursor_ide_touched() {
        wake_cursor(state);
    }
    let today = Local::now().date_naive();
    let since = Some(today.pred_opt().unwrap_or(today));
    if !state.parser.needs_cursor_remote_fetch(since) {
        return false;
    }
    use crate::commands::usage_query::{fetch_cursor_remote_now, CursorFetch};
    fetch_cursor_remote_now(state, since).await != CursorFetch::Unchanged
}

/// Probe the enabled providers' rate limits one at a time, until the probe
/// budget runs out, and store the merged result. Emits nothing: the publish
/// shows it.
async fn refresh_rate_limits(state: &AppState) {
    if !state.usage_access_enabled() || !state.rate_limits_enabled.load(Ordering::SeqCst) {
        return;
    }
    let interval = *state.refresh_interval.read().await;
    let deadline = Instant::now() + probe_budget(interval);
    let _io = state.rate_limits_io.lock().await;
    crate::statusline::source::maybe_trim(&crate::statusline::events_file());

    let codex_dir = state.parser.codex_dir().to_path_buf();
    let cached = state.cached_rate_limits.read().await.clone();
    // Only the integrations the user has switched on: the others would cost
    // a probe for numbers nobody sees.
    let fresh = crate::rate_limits::fetch_selected_rate_limits_until(
        &codex_dir,
        RateLimitSelection::enabled(&enabled_integrations(state)),
        cached.as_ref(),
        Some(deadline),
        false,
    )
    .await;

    let merged = merge_rate_limits(fresh, cached.as_ref());
    crate::plan_budget::record(&merged);
    *state.cached_rate_limits.write().await = Some(merged.clone());
    patch_tray_utilization(state, tray_utilization_from_rate_limits(Some(&merged))).await;
}

fn probe_budget(interval_secs: u64) -> Duration {
    let secs = if interval_secs == 0 {
        PROBE_BUDGET_SECS
    } else {
        (interval_secs / 2).clamp(1, PROBE_BUDGET_SECS)
    };
    Duration::from_secs(secs)
}

/// The cycle's one invalidation point, in one gate hold: apply the tables
/// fetched since the last sample, sweep the logs and the SSH caches, and drop
/// every payload a change made stale. Lines appended to listed logs drop only
/// the views that count their integration and reach their days; any other
/// change drops them all. The first sample also drops what the previous
/// session left: bootstrap queries may have listed the logs before it, which
/// hides their changes from the sweep.
async fn sample(state: &AppState, first: bool, cursor_changed: bool) -> Sampled {
    let _gate = state.compute.lock().await;
    let generation = state.refresh.generation.fetch_add(1, Ordering::SeqCst) + 1;

    let pricing = take(&state.refresh.pending_pricing);
    let pricing_applied = pricing.is_some();
    if let Some(rates) = pricing {
        crate::usage::pricing::set_dynamic_pricing(rates);
    }
    let fx = take(&state.refresh.pending_fx);
    let fx_applied = fx.is_some();
    if let Some(rates) = fx {
        crate::usage::exchange_rates::set_exchange_rates(rates);
    }

    let ssh_changed = state
        .ssh_cache
        .read()
        .await
        .as_ref()
        .is_some_and(|mgr| mgr.revalidate_records_memo());
    let mut clear_all = first | cursor_changed | ssh_changed | pricing_applied;
    let mut appends = None;
    if state.usage_access_enabled() {
        // Re-read the Kimi CLI's config.toml so a model switch ("K2.7
        // Coding" to "K3") shows up without an app restart, in every view.
        clear_all |= crate::models::set_model_display_overrides(
            crate::usage::kimi_parser::kimi_model_display_names(),
        );
        // Before the sweep: all that was logged before it, the sweep sees.
        let swept_at = Local::now();
        let requested = state.refresh.full_sweep.swap(false, Ordering::SeqCst);
        let full = first || requested || full_sweep_due(last_full_sweep(state), swept_at);
        match state.parser.sweep(full) {
            LogChanges::None => {}
            LogChanges::Appended(found) => appends = Some(found),
            LogChanges::Any => clear_all = true,
        }
        clear_all |= state.refresh.pending_change.swap(false, Ordering::SeqCst);
        if full {
            put(&state.refresh.swept, (generation, swept_at));
        }
    }
    // Disk first: the disk clear waits out the disk hits in flight, and the
    // memory clear after it drops the copies they made.
    if clear_all {
        state.clear_payload_disk_cache().await;
        state.parser.clear_payload_cache();
    } else if let Some(appends) = &appends {
        let stale = |key: &str| view_built_on_appended_logs(key, appends);
        state.clear_payload_disk_cache_where(stale).await;
        state.parser.clear_payload_cache_where(stale);
    }
    if let Ok(mut last) = state.refresh.last_sample.lock() {
        *last = Some(Local::now());
    }
    Sampled {
        generation,
        changed: clear_all || appends.is_some(),
        fx_applied,
    }
}

fn last_full_sweep(state: &AppState) -> Option<DateTime<Local>> {
    let swept = state.refresh.swept.lock().ok().and_then(|swept| *swept);
    swept.map(|(_, at)| at)
}

/// Whether a sample at `now` sweeps every file: in a new day, or an hour
/// after the last full sweep (or before it: the clock went back). An append
/// to a file not written for a week shows up that late at most.
fn full_sweep_due(last: Option<DateTime<Local>>, now: DateTime<Local>) -> bool {
    last.is_none_or(|at| {
        at.date_naive() != now.date_naive()
            || !(chrono::TimeDelta::zero()..FULL_SWEEP_EVERY).contains(&(now - at))
    })
}

fn take<T>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot.lock().ok().and_then(|mut value| value.take())
}

fn put<T>(slot: &Mutex<Option<T>>, value: T) {
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(value);
    }
}

/// Recompute the view the popover last asked for, so its re-read after the
/// publish is a hit. A failure is logged; the cycle goes on.
async fn refresh_active_view(app: Option<&AppHandle>, state: &AppState) {
    let view = state
        .refresh
        .active_view
        .lock()
        .ok()
        .and_then(|view| view.clone());
    let Some((provider, period, offset)) = view else {
        return;
    };
    if let Err(e) =
        crate::commands::usage_query::get_usage_data_inner(app, state, &provider, &period, offset)
            .await
    {
        tracing::warn!("Refresh: recomputing {provider}/{period}/{offset} failed: {e}");
    }
}

/// Make the cycle's values visible together: the tray cost, the staged plan
/// budgets, the tray paint, then the one `data-updated` every surface
/// re-reads on. `tray_version` is the tray cost's version at the Tray step.
async fn publish(
    app: Option<&AppHandle>,
    state: &AppState,
    cost: f64,
    tray_version: u64,
    sampled: &Sampled,
) {
    let cost = publish_cycle_tray_cost(state, cost, tray_version);
    crate::plan_budget::publish_staged();
    let Some(app) = app else {
        return;
    };
    crate::commands::tray::paint_tray_quietly(app, state, cost).await;
    if sampled.fx_applied {
        // The webview formats with the rates it pulled until told otherwise.
        let _ = app.emit("exchange-rates-updated", ());
    }
    let _ = app.emit("data-updated", sampled.generation);
}

/// A job independent of what a cycle shows. They run after the publish, in
/// slots spread evenly across the time to the next tick.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SlotJob {
    /// Archive the completed hours the cycle's sample swept, then start a
    /// background sync of the auto-export folder when one is set.
    ArchiveExport,
    /// Recompute a provider's plan-budget bars for the next publish.
    Budget(String, crate::plan_budget::Bars),
    /// Start fetching the price tables whose TTL has lapsed, in the
    /// background, for the next sample.
    PricingCheck,
}

/// The jobs for one cycle's slots: the archive first, so it follows the
/// sample it relies on as closely as it can.
pub(crate) fn plan_slots(
    budgets: Vec<(String, crate::plan_budget::Bars)>,
    pricing_due: bool,
) -> Vec<SlotJob> {
    let mut jobs = Vec::with_capacity(budgets.len() + 2);
    jobs.push(SlotJob::ArchiveExport);
    jobs.extend(
        budgets
            .into_iter()
            .map(|(provider, bars)| SlotJob::Budget(provider, bars)),
    );
    if pricing_due {
        jobs.push(SlotJob::PricingCheck);
    }
    jobs
}

/// When each of `n` jobs starts after the publish: evenly spaced through
/// `window`, less its last second, which stays free for the tick. The jobs
/// that do not fit are left out.
pub(crate) fn slot_offsets(window: Duration, n: usize) -> Vec<Duration> {
    let room = window.saturating_sub(SLOT_GUARD);
    if room.is_zero() {
        return Vec::new();
    }
    let gap = room / (n as u32 + 1);
    (1..=n as u32).map(|i| gap * i).collect()
}

/// Run the slot jobs from now, the publish, until the next tick after
/// `plan_from`; within a minute while the interval is Off. Each CPU job is
/// one gate hold. A job that overruns its slot pushes the next one back, a
/// job that would start in the last second is left for the next cycle, and a
/// request or a new interval ends the slots. `generation` is the cycle's.
async fn run_slots(app: &AppHandle, state: &AppState, plan_from: Moment, generation: u64) {
    let interval = *state.refresh_interval.read().await;
    let tick = next_tick(plan_from, interval);
    let budgets = if state.usage_access_enabled() {
        crate::plan_budget::due(&state.parser)
    } else {
        Vec::new()
    };
    let jobs = plan_slots(budgets, pricing_due(state));
    let publish = Instant::now();
    let mut window = tick.mono.saturating_duration_since(publish);
    if interval == 0 {
        window = window.min(OFF_SLOT_WINDOW);
    }
    let end = publish + window;
    let offsets = slot_offsets(window, jobs.len());
    for (job, offset) in jobs.into_iter().zip(offsets) {
        let slot = publish + offset;
        if !slot_reached(state, slot, interval).await {
            return;
        }
        let soon = Moment::now().later(SLOT_GUARD);
        if soon.mono >= end || tick.is_due(soon) {
            return;
        }
        let t0 = Instant::now();
        let label = format!("{job:?}");
        run_slot_job(app, state, job, generation).await;
        tracing::info!(
            "[PROFILE] slot job={label} took={:?} late={:?}",
            t0.elapsed(),
            t0.saturating_duration_since(slot)
        );
    }
}

/// Wait until `at`. False when the slots should end instead: a refresh was
/// requested (left for the loop) or the interval changed. A wake for neither
/// is stale and slept through.
async fn slot_reached(state: &AppState, at: Instant, interval: u64) -> bool {
    loop {
        if state.refresh.requested.load(Ordering::SeqCst)
            || *state.refresh_interval.read().await != interval
        {
            return false;
        }
        let wait = at.saturating_duration_since(Instant::now());
        if wait.is_zero() {
            return true;
        }
        // Timed out or woken: check again either way.
        let _ = tokio::time::timeout(wait, state.refresh.wake.notified()).await;
    }
}

async fn run_slot_job(app: &AppHandle, state: &AppState, job: SlotJob, generation: u64) {
    match job {
        SlotJob::ArchiveExport => archive_or_export(app, state, generation).await,
        SlotJob::Budget(provider, bars) => {
            let _gate = state.compute.lock().await;
            crate::plan_budget::refresh(state, provider, bars).await;
        }
        // Only a kick: the network never holds up the slots, and the tables
        // wait for the next sample.
        SlotJob::PricingCheck => {
            put(&state.refresh.last_pricing_check, Instant::now());
            kick_price_tables(app);
        }
    }
}

/// Archive the completed hours the sample of cycle `generation` swept, then
/// start a sync of the auto-export folder when one is set. The sync runs in
/// the background, outside the gate: the folder may be a network share or a
/// cloud-synced one, which neither the gate nor the slots may wait on.
async fn archive_or_export(app: &AppHandle, state: &AppState, generation: u64) {
    if !state.usage_access_enabled() {
        return;
    }
    {
        let _gate = state.compute.lock().await;
        archive_after_sample(state, generation).await;
    }
    if state.auto_export.read().await.folder.is_some() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            // A peer merge is left for the next sample.
            crate::commands::usage_io::run_auto_export(&app, &app.state::<AppState>()).await;
        });
    }
}

/// When the sample of cycle `generation` swept every file, if it did. One
/// taken while usage access was off swept none, and one between full sweeps
/// skipped the files not written for a week, while the archive trusts the
/// file cache: on one no sweep revalidated it would miss what was logged
/// since, and the frontier it advances would hide that for good.
fn swept_at(state: &AppState, generation: u64) -> Option<DateTime<Local>> {
    let (swept, at) = state.refresh.swept.lock().ok().and_then(|swept| *swept)?;
    (generation != 0 && swept == generation).then_some(at)
}

/// Archive the completed local and SSH hours, if the sample of cycle
/// `generation` swept every file; returns whether it did. Only the local hours
/// before the sample's own: the file cache lacks what was logged after it,
/// and the slot may run in a later hour. The caller holds the compute gate.
async fn archive_after_sample(state: &AppState, generation: u64) -> bool {
    let Some(swept_at) = swept_at(state, generation) else {
        return false;
    };
    crate::archive_local_usage(state, swept_at);
    crate::archive_ssh_device_usage(state).await;
    true
}

/// The price tables' TTLs are checked hourly.
fn pricing_due(state: &AppState) -> bool {
    state
        .refresh
        .last_pricing_check
        .lock()
        .is_ok_and(|last| last.is_none_or(|at| at.elapsed() >= PRICING_CHECK_EVERY))
}

/// Fetch the stale price tables in the background. Detached, so the network
/// and the multi-MB parse never hold up the refresh loop.
fn kick_price_tables(app: &AppHandle) {
    let Ok(app_data) = app.path().app_data_dir() else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        fetch_price_tables_once(&app.state::<AppState>(), &app_data).await;
    });
}

/// Fetch the stale price tables unless a fetch is already running; false if
/// one was.
pub(crate) async fn fetch_price_tables_once(state: &AppState, app_data: &Path) -> bool {
    let Some(_inflight) = InFlight::claim(&state.refresh.pricing_inflight) else {
        return false;
    };
    fetch_stale_price_tables(state, app_data).await;
    true
}

/// Fetch the price (7 d TTL) and exchange-rate (24 h TTL) tables that are
/// stale, and hold them for the next sample to apply.
async fn fetch_stale_price_tables(state: &AppState, app_data: &Path) {
    use crate::usage::{exchange_rates, litellm};
    if litellm::should_refresh(app_data) {
        match litellm::fetch_and_cache(app_data).await {
            Ok(rates) => {
                put(&state.refresh.pending_pricing, rates);
                tracing::info!(
                    "Dynamic pricing fetched (LiteLLM + OpenRouter), applied at the next refresh"
                );
            }
            Err(e) => tracing::warn!("Pricing fetch failed (keeping the current table): {e}"),
        }
    }
    if exchange_rates::should_refresh(app_data) {
        match exchange_rates::fetch_and_cache(app_data).await {
            Ok(rates) => {
                put(&state.refresh.pending_fx, rates);
                tracing::info!(
                    "Exchange rates fetched (frankfurter.dev), applied at the next refresh"
                );
            }
            Err(e) => tracing::warn!("Exchange rate fetch failed (keeping the current rates): {e}"),
        }
    }
}

/// Sync the included SSH hosts in the background. Emits nothing: the next
/// sample sees the rewritten remote caches.
fn kick_ssh(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        sync_ssh_once(&app.state::<AppState>()).await;
    });
}

/// Sync the SSH hosts unless a sync is already running; false if one was.
async fn sync_ssh_once(state: &AppState) -> bool {
    let Some(_inflight) = InFlight::claim(&state.refresh.ssh_inflight) else {
        return false;
    };
    sync_ssh_hosts(state).await;
    true
}

/// Sync the enabled, included SSH hosts one after another.
async fn sync_ssh_hosts(state: &AppState) {
    let enabled: Vec<String> = state
        .ssh_hosts
        .read()
        .await
        .iter()
        .filter(|c| c.enabled && c.include_in_stats)
        .map(|c| c.alias.clone())
        .collect();
    if enabled.is_empty() {
        return;
    }

    // Clone (sharing the records memo) so no RwLock guard is held across the
    // network I/O below.
    let Some(mgr) = state.ssh_cache.read().await.clone() else {
        return;
    };

    for alias in &enabled {
        let held_until = state
            .refresh
            .ssh_holds
            .lock()
            .ok()
            .and_then(|holds| holds.get(alias).map(|(until, _)| *until));
        if held_until.is_some_and(|until| SystemTime::now() < until) {
            continue;
        }
        // No `echo ok` pre-test: the sync's own ssh already runs in batch mode
        // with a connect timeout, so a dead host fails it just as fast.
        let synced = crate::usage::ssh_remote::with_host_timeout(async {
            // Within the host's timeout: a manual sync of it may hold this.
            let _one_sync = state.refresh.ssh_sync.lock().await;
            mgr.sync_host(alias).await
        })
        .await;
        let Ok(mut holds) = state.refresh.ssh_holds.lock() else {
            continue;
        };
        match synced {
            Ok(Ok(_)) => {
                holds.remove(alias);
            }
            Ok(Err(e)) if crate::usage::ssh_remote::is_permanent_ssh_failure(&e) => {
                let hold = next_ssh_hold(holds.get(alias).map(|(_, hold)| *hold));
                holds.insert(alias.clone(), (SystemTime::now() + hold, hold));
                tracing::warn!(
                    alias = %alias,
                    error = %e,
                    "SSH sync failed; left out of the background sync for {hold:?}"
                );
            }
            Ok(Err(e)) => tracing::error!(alias = %alias, error = %e, "SSH sync failed"),
            Err(_) => tracing::warn!(alias = %alias, "SSH sync timed out after 60s, skipping"),
        }
    }
}

type SshHolds = HashMap<String, (SystemTime, Duration)>;

/// The hold after another permanent failure: an hour, then doubling to a day.
fn next_ssh_hold(previous: Option<Duration>) -> Duration {
    previous.map_or(SSH_HOLD_FIRST, |hold| (hold * 2).min(SSH_HOLD_MAX))
}

/// The published tray cost. Before anything has been published, the first
/// reader computes it under the compute gate and publishes it, so the menu
/// bar never starts at `$0`.
pub(crate) async fn tray_cost(state: &AppState) -> f64 {
    if let Some(cost) = published_tray_cost(state) {
        return cost;
    }
    let _gate = state.compute.lock().await;
    // A reader queued ahead of this one may have just published it.
    if let Some(cost) = published_tray_cost(state) {
        return cost;
    }
    publish_tray_cost(state)
}

/// Recompute the tray cost under the compute gate and publish it.
pub(crate) async fn recompute_tray_cost(state: &AppState) -> f64 {
    let _gate = state.compute.lock().await;
    publish_tray_cost(state)
}

fn published_tray_cost(state: &AppState) -> Option<f64> {
    state.refresh.tray_cost.lock().ok().and_then(|cost| *cost)
}

/// The caller holds the compute gate.
fn publish_tray_cost(state: &AppState) -> f64 {
    let cost = crate::commands::tray::current_daily_total_cost_if_allowed(state);
    if let Ok(mut published) = state.refresh.tray_cost.lock() {
        *published = Some(cost);
        state
            .refresh
            .tray_cost_version
            .fetch_add(1, Ordering::SeqCst);
    }
    cost
}

/// Publish the cost a cycle computed at its Tray step, at `version`, and
/// return the cost to paint. A cost published since then (the enabled
/// integrations changed) is newer: it stays.
fn publish_cycle_tray_cost(state: &AppState, cost: f64, version: u64) -> f64 {
    let Ok(mut published) = state.refresh.tray_cost.lock() else {
        return cost;
    };
    if state.refresh.tray_cost_version.load(Ordering::SeqCst) == version {
        *published = Some(cost);
        return cost;
    }
    published.unwrap_or(cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::usage_query::get_usage_data_inner;
    use crate::usage::archive::ArchiveManager;
    use crate::usage::integrations::UsageIntegrationId;
    use crate::usage::parser::UsageParser;
    use chrono::Timelike;
    use std::io::Write;
    use std::path::Path;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    fn position(steps: &[Step], step: Step) -> usize {
        steps
            .iter()
            .position(|s| *s == step)
            .unwrap_or_else(|| panic!("{step:?} missing from {steps:?}"))
    }

    #[test]
    fn cycle_steps_do_the_io_first_sample_once_and_publish_last() {
        for first in [true, false] {
            let steps = cycle_steps(first);
            let at = |step| position(&steps, step);
            assert!(at(Step::CursorToday) < at(Step::Sample));
            assert!(at(Step::RateLimits) < at(Step::Sample));
            assert!(at(Step::Sample) < at(Step::Tray));
            assert!(at(Step::Sample) < at(Step::ActiveView));
            assert_eq!(steps.last(), Some(&Step::Publish));
            assert_eq!(steps.iter().filter(|s| **s == Step::Sample).count(), 1);
            assert_eq!(steps.contains(&Step::Cleanup), first);
            if first {
                assert!(at(Step::Cleanup) < at(Step::Sample));
                // The launch's cold tray compute holds the gate meanwhile.
                assert!(at(Step::RateLimits) < at(Step::Cleanup));
            }
        }
    }

    /// A tick `after` from now on both clocks.
    fn tick_in(after: Duration) -> Tick {
        let now = Moment::now();
        Tick {
            wall: now.wall.naive_local() + chrono::TimeDelta::from_std(after).unwrap(),
            mono: now.mono + after,
        }
    }

    #[tokio::test]
    async fn a_requested_refresh_runs_a_cycle_and_any_other_wake_only_replans() {
        let st = AppState::new();
        let later = || tick_in(Duration::from_secs(60));

        st.refresh.wake.notify_one();
        assert!(
            !cycle_due(&st, later()).await,
            "an interval change re-plans"
        );

        request_refresh(&st);
        assert!(cycle_due(&st, later()).await, "a request runs a cycle now");
        assert!(!st.refresh.requested.load(Ordering::SeqCst), "and is taken");

        assert!(
            cycle_due(&st, tick_in(Duration::ZERO)).await,
            "a tick that has already passed runs late, not never"
        );
    }

    #[tokio::test]
    async fn focus_refreshes_only_while_off_and_only_older_data() {
        let st = AppState::new();
        let sampled_ago = |secs: i64| {
            *st.refresh.last_sample.lock().unwrap() =
                Some(Local::now() - chrono::TimeDelta::seconds(secs));
        };
        let requested = || st.refresh.requested.swap(false, Ordering::SeqCst);

        *st.refresh_interval.write().await = 300;
        sampled_ago(3600);
        refresh_if_looked_at(&st).await;
        assert!(!requested(), "a running interval refreshes on its own");

        *st.refresh_interval.write().await = 0;
        sampled_ago(10);
        refresh_if_looked_at(&st).await;
        assert!(!requested(), "data this fresh is shown as it is");

        sampled_ago(30);
        refresh_if_looked_at(&st).await;
        assert!(requested(), "while Off, a look at older data refreshes it");

        *st.refresh.last_sample.lock().unwrap() = None;
        refresh_if_looked_at(&st).await;
        assert!(requested(), "as does a look before the first sample");
    }

    #[tokio::test]
    async fn a_request_the_slots_woke_for_still_runs_a_cycle() {
        // The slots' sleep took the wake and left the request for the loop.
        let st = AppState::new();
        st.refresh.requested.store(true, Ordering::SeqCst);
        let due = tokio::time::timeout(
            Duration::from_secs(5),
            cycle_due(&st, tick_in(Duration::from_secs(60))),
        )
        .await;
        assert!(matches!(due, Ok(true)), "the cycle runs now");
        assert!(!st.refresh.requested.load(Ordering::SeqCst), "and takes it");
    }

    #[test]
    fn slot_offsets_spread_the_jobs_evenly_before_the_deadline() {
        let offsets = slot_offsets(Duration::from_secs(291), 5);
        assert_eq!(offsets.len(), 5);
        assert!(offsets[0] > Duration::ZERO, "none at the publish itself");
        let gap = offsets[0];
        for pair in offsets.windows(2) {
            assert!(pair[1] > pair[0], "strictly increasing: {offsets:?}");
            assert_eq!(pair[1] - pair[0], gap, "evenly spaced: {offsets:?}");
        }
        assert!(
            *offsets.last().unwrap() < Duration::from_secs(290),
            "the last second before the tick stays free: {offsets:?}"
        );
    }

    #[test]
    fn slot_offsets_leave_out_what_cannot_fit() {
        let window = Duration::from_millis(500);
        let offsets = slot_offsets(window, 3);
        assert!(
            offsets.iter().all(|at| *at < window),
            "nothing past the window: {offsets:?}"
        );
        assert!(offsets.is_empty(), "no room before the last second");
        assert!(slot_offsets(Duration::from_secs(60), 0).is_empty());
    }

    #[test]
    fn plan_slots_archives_first() {
        let bars = vec![("5h".to_string(), chrono::Utc::now())];
        let jobs = plan_slots(vec![("claude".to_string(), bars.clone())], true);
        assert_eq!(
            jobs,
            vec![
                SlotJob::ArchiveExport,
                SlotJob::Budget("claude".to_string(), bars),
                SlotJob::PricingCheck,
            ]
        );
        assert_eq!(plan_slots(Vec::new(), false), vec![SlotJob::ArchiveExport]);
    }

    #[tokio::test]
    async fn slots_end_on_a_request_or_a_new_interval_and_sleep_through_a_stale_wake() {
        let st = AppState::new();
        let interval = *st.refresh_interval.read().await;
        let later = Instant::now() + Duration::from_secs(60);

        st.refresh.wake.notify_one();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                slot_reached(&st, later, interval)
            )
            .await
            .is_err(),
            "a wake that asks for nothing is slept through"
        );
        assert!(
            slot_reached(&st, Instant::now(), interval).await,
            "a slot that has come runs"
        );

        *st.refresh_interval.write().await = interval + 1;
        assert!(
            !slot_reached(&st, later, interval).await,
            "a new interval ends the slots, to re-plan"
        );

        request_refresh(&st);
        assert!(
            !slot_reached(&st, later, interval + 1).await,
            "a request ends the slots"
        );
        assert!(
            st.refresh.requested.load(Ordering::SeqCst),
            "and is left for the cycle"
        );
    }

    #[tokio::test]
    async fn an_ssh_kick_while_one_runs_does_nothing() {
        let st = AppState::new();
        let running = InFlight::claim(&st.refresh.ssh_inflight).unwrap();
        assert!(!sync_ssh_once(&st).await, "a second sync does not start");
        assert!(
            st.refresh.ssh_inflight.load(Ordering::SeqCst),
            "nor lets go of the running one's claim"
        );
        drop(running);
        assert!(sync_ssh_once(&st).await, "once it ends, the next kick runs");
        assert!(
            !st.refresh.ssh_inflight.load(Ordering::SeqCst),
            "and lets go when done"
        );
    }

    #[test]
    fn a_failing_ssh_host_is_held_an_hour_then_doubling_to_a_day() {
        let hour = Duration::from_secs(3600);
        assert_eq!(next_ssh_hold(None), hour);
        assert_eq!(next_ssh_hold(Some(hour)), 2 * hour);
        assert_eq!(next_ssh_hold(Some(16 * hour)), 24 * hour);
        assert_eq!(next_ssh_hold(Some(24 * hour)), 24 * hour);
    }

    #[tokio::test]
    async fn a_price_check_while_one_runs_does_nothing() {
        let st = AppState::new();
        let dir = TempDir::new().unwrap();
        let running = InFlight::claim(&st.refresh.pricing_inflight).unwrap();
        assert!(
            !fetch_price_tables_once(&st, dir.path()).await,
            "a second fetch does not start"
        );
        assert!(
            st.refresh.pricing_inflight.load(Ordering::SeqCst),
            "nor lets go of the running one's claim"
        );
        drop(running);
    }

    #[tokio::test]
    async fn a_look_while_a_cycle_runs_requests_no_other() {
        let st = AppState::new();
        *st.refresh_interval.write().await = 0;
        // Hold the cycle at its sample, before it has stamped a new one.
        let gate = st.compute.lock().await;
        let look = async {
            while !st.refresh.cycle_running.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
            refresh_if_looked_at(&st).await;
            let requested = st.refresh.requested.load(Ordering::SeqCst);
            drop(gate);
            requested
        };
        let (_, requested) = tokio::join!(run_cycle(None, &st, false), look);
        assert!(
            !requested,
            "the running cycle publishes what the look wants"
        );
        assert!(
            !st.refresh.cycle_running.load(Ordering::SeqCst),
            "and lets go when done"
        );

        *st.refresh.last_sample.lock().unwrap() = None;
        refresh_if_looked_at(&st).await;
        assert!(
            st.refresh.requested.load(Ordering::SeqCst),
            "a look after it requests one again"
        );
    }

    #[tokio::test]
    async fn a_tick_slept_through_is_due_by_the_wall_clock() {
        // The machine slept: the wall clock has passed the tick, while the
        // monotonic clock, stopped during the sleep, says it is an hour off.
        let st = AppState::new();
        let tick = Tick {
            wall: Local::now().naive_local() - chrono::TimeDelta::seconds(1),
            mono: Instant::now() + Duration::from_secs(3600),
        };
        let due = tokio::time::timeout(Duration::from_secs(5), cycle_due(&st, tick)).await;
        assert!(matches!(due, Ok(true)), "the tick runs now");
    }

    fn claude_line(input_tokens: u64) -> String {
        model_line("claude-sonnet-4-6-20260301", input_tokens)
    }

    fn model_line(model: &str, input_tokens: u64) -> String {
        model_line_at(model, input_tokens, Local::now())
    }

    fn model_line_at(model: &str, input_tokens: u64, at: DateTime<Local>) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"model":"{model}","usage":{{"input_tokens":{input_tokens},"output_tokens":500}},"stop_reason":"end_turn"}}}}"#,
            at.to_rfc3339()
        ) + "\n"
    }

    fn append(path: &Path, line: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::options()
            .create(true)
            .append(true)
            .open(path)
            .unwrap()
            .write_all(line.as_bytes())
            .unwrap();
    }

    /// A Claude-only state over temp dirs, set up as the refresh loop runs:
    /// usage access on, listings frozen, a disk cache under `app_data`.
    async fn claude_state(claude_dir: &Path, app_data: &Path) -> AppState {
        let mut state = AppState::new();
        state.usage_access_enabled.store(true, Ordering::SeqCst);
        let parser = UsageParser::with_dirs(claude_dir.to_path_buf(), app_data.join("codex"));
        parser.set_listings_frozen(true);
        state.parser = Arc::new(parser);
        *state.enabled_integrations.write().unwrap() = vec![UsageIntegrationId::Claude];
        *state.payload_disk_cache.write().await = Some(
            crate::usage::payload_disk_cache::PayloadDiskCache::new(app_data),
        );
        state
    }

    async fn day_view(state: &AppState) -> crate::models::UsagePayload {
        get_usage_data_inner(None, state, "claude", "day", 0)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn values_move_only_at_a_sample() {
        let dir = TempDir::new().unwrap();
        let claude_dir = dir.path().join("claude");
        let log = claude_dir.join("session.jsonl");
        append(&log, &claude_line(1_000));
        let st = claude_state(&claude_dir, dir.path()).await;
        *st.refresh.active_view.lock().unwrap() = Some(("claude".into(), "day".into(), 0));

        run_cycle(None, &st, true).await;
        let generation = st.refresh.generation.load(Ordering::SeqCst);
        let before = tray_cost(&st).await;
        assert!(before > 0.0, "guard: the fixture has usage today");
        assert!((day_view(&st).await.total_cost - before).abs() < 1e-9);

        append(&log, &claude_line(50_000));
        assert!(
            (day_view(&st).await.total_cost - before).abs() < 1e-9,
            "no sample yet: the view keeps the published value"
        );
        assert_eq!(tray_cost(&st).await, before, "and so does the tray");

        run_cycle(None, &st, false).await;
        let after = tray_cost(&st).await;
        assert!(after > before, "the sample picks the append up");
        assert!((day_view(&st).await.total_cost - after).abs() < 1e-9);
        assert_eq!(st.refresh.generation.load(Ordering::SeqCst), generation + 1);
    }

    /// An archive under `dir` that takes only the fixture's Claude logs: this
    /// machine's own Cursor and Kimi logs count as scanned this hour.
    fn claude_only_archive(st: &AppState, dir: &Path) -> ArchiveManager {
        let archive = ArchiveManager::new(dir);
        skip_other_sources(&archive, Local::now());
        st.parser.set_archive(archive.clone());
        archive
    }

    /// Count this machine's own Cursor and Kimi logs as scanned for an
    /// archive whose horizon is `at`.
    fn skip_other_sources(archive: &ArchiveManager, at: DateTime<Local>) {
        for source in ["local:codex", "local:cursor", "local:kimi"] {
            archive.should_scan(source, at.date_naive(), at.hour() as u8, 0);
        }
    }

    #[tokio::test]
    async fn the_archive_slot_leaves_the_hour_of_its_sample_open() {
        let dir = TempDir::new().unwrap();
        let claude_dir = dir.path().join("claude");
        let logged = Local::now() - chrono::TimeDelta::hours(2);
        append(
            &claude_dir.join("session.jsonl"),
            &model_line_at("claude-sonnet-4-6-20260301", 1_000, logged),
        );
        let st = claude_state(&claude_dir, dir.path()).await;
        let archive = claude_only_archive(&st, dir.path());
        let generation = run_cycle(None, &st, true).await;
        let archived = || archive.read_raw("local:claude").len();

        // The sample was taken in the hour the row was logged in, and the
        // slot runs in a later one: more may be logged to that hour after
        // the sample, which its file cache lacks.
        put(&st.refresh.swept, (generation, logged));
        skip_other_sources(&archive, logged);
        assert!(archive_after_sample(&st, generation).await);
        assert_eq!(archived(), 0, "the hour of the sample stays open");

        let now = Local::now();
        put(&st.refresh.swept, (generation, now));
        skip_other_sources(&archive, now);
        assert!(archive_after_sample(&st, generation).await);
        assert_eq!(archived(), 1, "a later sample's slot archives it");
    }

    #[test]
    fn a_tray_cost_published_after_the_tray_step_outlives_the_cycle() {
        let st = AppState::new();
        let at_tray_step = st.refresh.tray_cost_version.load(Ordering::SeqCst);
        assert_eq!(publish_cycle_tray_cost(&st, 7.0, at_tray_step), 7.0);
        assert_eq!(
            published_tray_cost(&st),
            Some(7.0),
            "nothing newer: published"
        );

        // The enabled integrations change after the Tray step, and their
        // cost is published at once.
        let at_tray_step = st.refresh.tray_cost_version.load(Ordering::SeqCst);
        let newer = publish_tray_cost(&st);
        assert_ne!(newer, 7.0, "guard: the costs differ");
        assert_eq!(publish_cycle_tray_cost(&st, 7.0, at_tray_step), newer);
        assert_eq!(
            published_tray_cost(&st),
            Some(newer),
            "the newer cost stays"
        );
    }

    #[tokio::test]
    async fn the_archive_reads_the_logs_from_its_frontiers_day_on() {
        let dir = TempDir::new().unwrap();
        let claude_dir = dir.path().join("claude");
        let log = claude_dir.join("session.jsonl");
        let model = "claude-sonnet-4-6-20260301";
        let first = (Local::now() - chrono::TimeDelta::days(2))
            .with_hour(10)
            .unwrap();
        append(&log, &model_line_at(model, 1_000, first));
        let st = claude_state(&claude_dir, dir.path()).await;
        let archive = claude_only_archive(&st, dir.path());
        let horizon = first + chrono::TimeDelta::hours(1);
        skip_other_sources(&archive, horizon);
        crate::archive_local_usage(&st, horizon);
        assert_eq!(archive.read_raw("local:claude").len(), 1, "guard");

        // A later hour of the frontier's day.
        let later = first + chrono::TimeDelta::hours(2);
        append(&log, &model_line_at(model, 1_000, later));
        assert!(st.parser.invalidate_if_changed(), "guard: swept");
        skip_other_sources(&archive, Local::now());
        crate::archive_local_usage(&st, Local::now());
        assert_eq!(archive.read_raw("local:claude").len(), 2);
    }

    #[tokio::test]
    async fn the_archive_slot_archives_only_after_a_sample_that_swept() {
        let dir = TempDir::new().unwrap();
        let claude_dir = dir.path().join("claude");
        let completed_hour = Local::now() - chrono::TimeDelta::days(2);
        append(
            &claude_dir.join("session.jsonl"),
            &model_line_at("claude-sonnet-4-6-20260301", 1_000, completed_hour),
        );
        let st = claude_state(&claude_dir, dir.path()).await;
        let archive = claude_only_archive(&st, dir.path());
        let archived = || archive.read_raw("local:claude").len();

        // Usage access was off at the sample, so it swept nothing.
        st.usage_access_enabled.store(false, Ordering::SeqCst);
        let unswept = run_cycle(None, &st, true).await;
        st.usage_access_enabled.store(true, Ordering::SeqCst);
        assert!(!archive_after_sample(&st, unswept).await);
        assert_eq!(archived(), 0, "no archive on a file cache no sample swept");

        let swept = run_cycle(None, &st, false).await;
        assert!(archive_after_sample(&st, swept).await);
        assert_eq!(archived(), 1, "the completed hour this sample swept");
    }

    #[tokio::test]
    async fn first_sample_drops_previous_session_disk_entries() {
        let dir = TempDir::new().unwrap();
        let claude_dir = dir.path().join("claude");
        let log = claude_dir.join("session.jsonl");
        append(&log, &claude_line(1_000));

        // The previous session persisted its day view, then more was logged.
        let old = day_view(&claude_state(&claude_dir, dir.path()).await)
            .await
            .total_cost;
        append(&log, &claude_line(50_000));

        // At launch another tab's query lists the logs before the first
        // sample, and the day view comes back from disk into memory.
        let st = claude_state(&claude_dir, dir.path()).await;
        get_usage_data_inner(None, &st, "claude", "week", 0)
            .await
            .unwrap();
        let restored = day_view(&st).await;
        assert!(
            restored.from_cache && (restored.total_cost - old).abs() < 1e-9,
            "guard: served from the previous session's disk entry"
        );
        assert!(
            !st.parser.invalidate_if_changed(),
            "guard: the sweep sees nothing new"
        );

        run_cycle(None, &st, true).await;
        let fresh = day_view(&st).await;
        assert!(
            !fresh.from_cache,
            "neither the disk entry nor its memory copy survives"
        );
        assert!(fresh.total_cost > old);
    }

    #[test]
    fn every_file_is_swept_hourly_and_in_a_new_day() {
        let now = Local::now().with_hour(12).unwrap();
        let ago = |minutes| Some(now - chrono::TimeDelta::minutes(minutes));
        assert!(full_sweep_due(None, now));
        assert!(!full_sweep_due(ago(59), now));
        assert!(full_sweep_due(ago(60), now));
        assert!(full_sweep_due(ago(-1), now), "the clock went back");
        let midnight = now.with_hour(0).unwrap().with_minute(0).unwrap();
        let before_midnight = Some(midnight - chrono::TimeDelta::minutes(1));
        assert!(full_sweep_due(before_midnight, midnight));
    }

    #[tokio::test]
    async fn an_append_drops_only_the_views_it_can_reach() {
        let dir = TempDir::new().unwrap();
        let claude_dir = dir.path().join("claude");
        let log = claude_dir.join("session.jsonl");
        append(&log, &claude_line(1_000));
        let st = claude_state(&claude_dir, dir.path()).await;
        run_cycle(None, &st, true).await;
        let today = Local::now().date_naive().format("%Y%m%d");
        let version = crate::usage::pricing::PRICING_VERSION;
        let views = [
            format!("usage-view:{version}:claude:day:0:{today}"),
            format!("usage-view:{version}:codex:day:0:{today}"),
            format!("usage-view:{version}:claude:day:-1:19990101"),
        ];
        let store = || {
            for key in &views {
                st.parser
                    .store_cache(key, crate::models::UsagePayload::default());
            }
        };
        let cached = || {
            views
                .each_ref()
                .map(|key| st.parser.check_cache_as_stored(key).is_some())
        };

        store();
        append(&log, &claude_line(50_000));
        run_cycle(None, &st, false).await;
        assert_eq!(cached(), [false, true, true]);

        store();
        append(&claude_dir.join("other.jsonl"), &claude_line(1_000));
        filetime::set_file_mtime(
            &claude_dir,
            filetime::FileTime::from_system_time(
                std::time::SystemTime::now() + Duration::from_secs(2),
            ),
        )
        .unwrap();
        run_cycle(None, &st, false).await;
        assert_eq!(cached(), [false; 3], "a new file drops them all");
    }

    #[tokio::test]
    async fn fetched_prices_apply_at_the_next_sample_not_before() {
        // A Claude model of no known family: its price key is its own, so
        // the fetched table reprices no other test's model.
        let model = "claude-tmpendingprice";
        let dir = TempDir::new().unwrap();
        let claude_dir = dir.path().join("claude");
        append(
            &claude_dir.join("session.jsonl"),
            &model_line(model, 1_000_000),
        );
        let st = claude_state(&claude_dir, dir.path()).await;
        *st.refresh.active_view.lock().unwrap() = Some(("claude".into(), "day".into(), 0));
        run_cycle(None, &st, true).await;
        let day = day_view(&st).await;
        assert!(day.total_tokens > 0, "guard: the fixture's model counts");
        let before = day.total_cost;
        let price = || crate::usage::pricing::calculate_cost(model, 1_000_000, 0, 0, 0, 0, 0);
        let old_price = price();

        // What a price check fetched in a slot.
        let rates = DynamicModelRates {
            input: 1_000.0,
            output: 1_000.0,
            cache_write_5m: 1_000.0,
            cache_write_1h: 1_000.0,
            cache_read: 1_000.0,
        };
        put(
            &st.refresh.pending_pricing,
            HashMap::from([(crate::models::normalized_model_key(model), rates)]),
        );
        assert_eq!(price(), old_price, "not applied before the sample");
        assert_eq!(day_view(&st).await.total_cost, before);

        run_cycle(None, &st, false).await;
        assert!(
            take(&st.refresh.pending_pricing).is_none(),
            "the sample takes them"
        );
        assert!(
            day_view(&st).await.total_cost > before + 1.0,
            "and drops the views priced with the old table"
        );
        assert!(tray_cost(&st).await > before + 1.0, "the tray too");
    }
}
