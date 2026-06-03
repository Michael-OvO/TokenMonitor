pub mod calendar;
pub mod config;
pub mod float_ball;
pub mod logging;
pub mod period;
pub mod ssh;
pub mod statusline;
pub mod tray;
pub mod updater;
pub mod usage_io;
pub mod usage_query;

pub use tray::sync_tray_title;

use crate::models::*;
use crate::statusline::windows::ClaudePlanTier;
use crate::usage::integrations::UsageIntegrationSelection;
use crate::usage::parser::{UsageParser, UsageQueryDebugReport};
use crate::usage::payload_disk_cache::PayloadDiskCache;
use crate::usage::ssh_remote::{SshCacheManager, SshHostConfig};
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::RwLock;

#[allow(dead_code)]
pub struct AppState {
    pub parser: Arc<UsageParser>,
    pub refresh_interval: Arc<RwLock<u64>>,
    pub tray_config: Arc<RwLock<tray::TrayConfig>>,
    pub(crate) tray_utilization: Arc<RwLock<tray::TrayUtilization>>,
    pub last_usage_debug: Arc<RwLock<Option<UsageDebugReport>>>,
    pub cached_rate_limits: Arc<RwLock<Option<RateLimitsPayload>>>,
    pub glass_enabled: Arc<RwLock<bool>>,
    pub float_ball_state: Arc<RwLock<float_ball::FloatBallState>>,
    pub ssh_hosts: Arc<RwLock<Vec<SshHostConfig>>>,
    pub ssh_cache: Arc<RwLock<Option<SshCacheManager>>>,
    pub updater: Arc<RwLock<crate::updater::UpdaterState>>,
    /// When true, the main window blur handler skips hiding once.
    /// Set by commands that cause transient focus loss (float ball, dock icon, etc.).
    pub suppress_auto_hide: Arc<AtomicBool>,
    /// When false, the background loop skips rate-limit refresh. The flag
    /// is retained for backwards compatibility with the existing toggle in
    /// Settings, but rate-limit fetches no longer touch any system credential
    /// store — they only read the local statusline event file and the JSONL
    /// parser cache, so leaving it on by default is safe.
    pub rate_limits_enabled: Arc<AtomicBool>,
    /// Gates local Claude/Codex session-log reads until the frontend has shown
    /// the first-run local access disclosure. Existing installs enable this at
    /// bootstrap; new installs flip it after the welcome card is dismissed.
    pub usage_access_enabled: Arc<AtomicBool>,
    /// Plan tier used to compute the rolling-window utilization percentages.
    /// Updated by `set_claude_plan_tier`; defaults to `Pro` for new installs.
    pub claude_plan_tier: Arc<RwLock<ClaudePlanTier>>,
    pub payload_disk_cache: Arc<RwLock<Option<PayloadDiskCache>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            parser: Arc::new(UsageParser::new()),
            refresh_interval: Arc::new(RwLock::new(30)),
            tray_config: Arc::new(RwLock::new(tray::TrayConfig::default())),
            tray_utilization: Arc::new(RwLock::new(tray::TrayUtilization::default())),
            last_usage_debug: Arc::new(RwLock::new(None)),
            cached_rate_limits: Arc::new(RwLock::new(None)),
            glass_enabled: Arc::new(RwLock::new(true)),
            float_ball_state: Arc::new(RwLock::new(float_ball::FloatBallState::default())),
            ssh_hosts: Arc::new(RwLock::new(Vec::new())),
            ssh_cache: Arc::new(RwLock::new(None)),
            updater: Arc::new(RwLock::new(crate::updater::UpdaterState::new())),
            suppress_auto_hide: Arc::new(AtomicBool::new(false)),
            rate_limits_enabled: Arc::new(AtomicBool::new(false)),
            usage_access_enabled: Arc::new(AtomicBool::new(false)),
            claude_plan_tier: Arc::new(RwLock::new(ClaudePlanTier::default())),
            payload_disk_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Drop the persistent payload disk cache for every usage view.
    ///
    /// The disk cache has no TTL: once a per-view payload is written it is
    /// re-served on every in-memory cache miss until the file is deleted. The
    /// background refresh loops clear the in-memory payload cache as soon as
    /// local source logs change, but unless the disk entries are dropped too a
    /// stale per-provider payload (e.g. `usage-view:claude:day:0:<date>`) keeps
    /// being served all day — freezing that tab while the `all` view (whose disk
    /// entries are cleared by the cursor/SSH paths, or never persisted while
    /// `cursor_loading`) keeps refreshing. Mirrors the SSH-sync path: clearing
    /// the `usage-view:` prefix forces the next fetch to recompute from fresh
    /// logs. The cold-start fallback is unaffected — the recompute re-persists.
    pub(crate) async fn clear_payload_disk_cache(&self) {
        if let Some(ref disk_cache) = *self.payload_disk_cache.read().await {
            disk_cache.clear_prefix("usage-view:");
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDebugReport {
    pub request_kind: String,
    pub requested_provider: String,
    pub period: Option<String>,
    pub offset: Option<i32>,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub queries: Vec<UsageQueryDebugReport>,
}

pub(crate) async fn set_last_usage_debug(state: &AppState, report: UsageDebugReport) {
    let mut current = state.last_usage_debug.write().await;
    *current = Some(report);
}

fn capture_query_debug(parser: &UsageParser) -> Result<UsageQueryDebugReport, String> {
    parser
        .last_query_debug()
        .ok_or_else(|| String::from("Usage debug report was not available"))
}

pub(crate) fn maybe_capture_query_debug(
    parser: &UsageParser,
    payload: &UsagePayload,
) -> Result<Option<UsageQueryDebugReport>, String> {
    if payload.from_cache {
        Ok(None)
    } else {
        capture_query_debug(parser).map(Some)
    }
}

pub(crate) fn parse_usage_selection(provider: &str) -> Result<UsageIntegrationSelection, String> {
    UsageIntegrationSelection::parse(provider)
        .ok_or_else(|| format!("Unknown usage integration: {provider}"))
}
