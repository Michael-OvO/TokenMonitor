use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

use super::persistence;
use super::state::UpdateInfo;
use crate::commands::AppState;

/// Returns true if `candidate` is strictly newer than `current` by semver.
/// Uses a simple dotted-numeric comparison. Rejects candidates that look like
/// prereleases (contain `-`), per the "stable-only" design decision.
#[allow(dead_code)]
pub fn is_newer(current: &str, candidate: &str) -> bool {
    if candidate.contains('-') {
        return false;
    }
    let parse =
        |s: &str| -> Option<Vec<u64>> { s.split('.').map(|p| p.parse::<u64>().ok()).collect() };
    match (parse(current), parse(candidate)) {
        (Some(c), Some(n)) => n > c,
        _ => false,
    }
}

#[allow(dead_code)]
const INITIAL_DELAY: Duration = Duration::from_secs(10);
#[allow(dead_code)]
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);
#[allow(dead_code)]
const BACKOFF_MIN: Duration = Duration::from_secs(12 * 3600);
#[allow(dead_code)]
const BACKOFF_MAX: Duration = Duration::from_secs(24 * 3600);

/// Spawn the background updater task. Called from `lib.rs` setup().
#[allow(dead_code)]
pub fn spawn<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(INITIAL_DELAY).await;
        let mut backoff: Option<Duration> = None;
        loop {
            let enabled = {
                let state = app.state::<AppState>();
                let guard = state.updater.read().await;
                guard.auto_check_enabled
            };
            if enabled {
                match run_check(&app).await {
                    Ok(_) => backoff = None,
                    Err(e) => {
                        tracing::warn!("Updater check failed: {e}");
                        backoff = Some(match backoff {
                            None => BACKOFF_MIN,
                            Some(prev) => (prev * 2).min(BACKOFF_MAX),
                        });
                    }
                }
            }
            let sleep_for = backoff.unwrap_or(CHECK_INTERVAL);
            tokio::time::sleep(sleep_for).await;
        }
    });
}

/// Execute a single update check and update state. Exposed for `check_now`.
#[allow(dead_code)]
pub async fn run_check<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let check_result = updater.check().await;

    let state = app.state::<AppState>();
    let mut guard = state.updater.write().await;
    guard.last_check = Some(chrono::Utc::now());

    match check_result {
        Ok(Some(update)) => {
            let pub_date = update.date.and_then(|d| {
                chrono::DateTime::parse_from_rfc3339(&d.to_string())
                    .ok()
                    .map(|dt| dt.with_timezone(&chrono::Utc))
            });
            let info = UpdateInfo {
                version: update.version.clone(),
                current_version: update.current_version.clone(),
                notes: update.body.clone(),
                pub_date,
            };
            guard.available = Some(info);
            guard.last_check_error = None;

            // Do not fire an OS notification here. On macOS, a background
            // notification can surface a system permission prompt while the app
            // is hidden and before our own disclosure UI. The in-app update
            // banner emitted below is permission-free.

            let _ = persistence::save(app, &guard);
            drop(guard);
            let _ = app.emit("updater://status-changed", ());
            Ok(())
        }
        Ok(None) => {
            guard.available = None;
            guard.last_check_error = None;
            let _ = persistence::save(app, &guard);
            drop(guard);
            let _ = app.emit("updater://status-changed", ());
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            // A missing `latest.json` (404) is the normal state before the first
            // manifest has been published — don't surface it as an error. DNS
            // and other network failures ARE real and worth reporting.
            let is_missing_manifest = msg.contains("Could not fetch a valid release JSON");
            if is_missing_manifest {
                guard.available = None;
                guard.last_check_error = None;
            } else {
                guard.last_check_error = Some(msg.clone());
            }
            let _ = persistence::save(app, &guard);
            drop(guard);
            let _ = app.emit("updater://status-changed", ());
            Err(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch() {
        assert!(is_newer("0.7.2", "0.7.3"));
    }

    #[test]
    fn newer_minor() {
        assert!(is_newer("0.7.2", "0.8.0"));
    }

    #[test]
    fn newer_major() {
        assert!(is_newer("0.7.2", "1.0.0"));
    }

    #[test]
    fn same_version_is_not_newer() {
        assert!(!is_newer("0.7.2", "0.7.2"));
    }

    #[test]
    fn older_is_not_newer() {
        assert!(!is_newer("0.7.2", "0.7.1"));
    }

    #[test]
    fn prerelease_is_rejected() {
        assert!(!is_newer("0.7.2", "0.8.0-beta.1"));
    }

    #[test]
    fn malformed_is_not_newer() {
        assert!(!is_newer("0.7.2", "garbage"));
        assert!(!is_newer("garbage", "0.8.0"));
    }
}
