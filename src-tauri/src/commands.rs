pub mod calendar;
pub mod config;
pub mod float_ball;
pub mod logging;
pub mod period;
pub mod ssh;
pub mod tray;
pub mod updater;
pub mod usage_query;

pub use tray::sync_tray_title;

use crate::models::*;
use crate::usage::integrations::UsageIntegrationSelection;
use crate::usage::parser::{UsageParser, UsageQueryDebugReport};
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
    /// When false, the background loop skips rate-limit refresh so that we
    /// don't poke macOS Keychain until the user has opted into live rate
    /// limits. Starts off; the frontend flips it via `set_rate_limits_enabled`
    /// during bootstrap and whenever the user toggles the setting.
    pub rate_limits_enabled: Arc<AtomicBool>,
    /// Gates local Claude/Codex session-log reads until the frontend has shown
    /// the first-run local access disclosure. Existing installs enable this at
    /// bootstrap; new installs flip it after the welcome card is dismissed.
    pub usage_access_enabled: Arc<AtomicBool>,
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
