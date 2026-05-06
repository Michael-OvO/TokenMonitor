use super::usage_query::get_provider_data;
use super::AppState;
use crate::models::*;
use crate::usage::integrations::all_usage_integrations;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
#[cfg(target_os = "macos")]
use tauri::Runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarDisplay {
    Off,
    Single,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PercentageFormat {
    Compact,
    Verbose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CostPrecision {
    Whole,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayConfig {
    pub bar_display: BarDisplay,
    pub bar_provider: String, // "claude" | "codex"
    #[serde(default)]
    pub bar_providers: Vec<String>,
    pub show_percentages: bool,
    pub percentage_format: PercentageFormat,
    pub show_cost: bool,
    pub cost_precision: CostPrecision,
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self {
            bar_display: BarDisplay::Both,
            bar_provider: "claude".to_string(),
            bar_providers: vec![
                "claude".to_string(),
                "codex".to_string(),
                "cursor".to_string(),
            ],
            show_percentages: false,
            percentage_format: PercentageFormat::Compact,
            show_cost: true,
            cost_precision: CostPrecision::Full,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct TrayUtilization {
    pub(crate) claude: Option<f64>,
    pub(crate) codex: Option<f64>,
    pub(crate) cursor: Option<f64>,
}

impl TrayUtilization {
    pub(crate) fn has_any(self) -> bool {
        self.claude.is_some() || self.codex.is_some() || self.cursor.is_some()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusWidgetSummary {
    pub config: TrayConfig,
    pub total_cost: f64,
    pub claude_util: Option<f64>,
    pub codex_util: Option<f64>,
    pub cursor_util: Option<f64>,
    pub title: String,
}

fn format_tray_title(
    config: &TrayConfig,
    total_cost: f64,
    claude_util: Option<f64>,
    codex_util: Option<f64>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Percentages -- independent of bar_display.
    // Utilization values are already 0-100.
    if config.show_percentages {
        if let (Some(c), Some(x)) = (claude_util, codex_util) {
            let c_pct = c.round() as i64;
            let x_pct = x.round() as i64;
            if config.percentage_format == PercentageFormat::Compact {
                parts.push(format!("{} \u{00b7} {}", c_pct, x_pct));
            } else {
                parts.push(format!("Claude Code {}%  Codex {}%", c_pct, x_pct));
            }
        } else if let Some(c) = claude_util {
            let pct = c.round() as i64;
            if config.percentage_format == PercentageFormat::Compact {
                parts.push(format!("{}", pct));
            } else {
                parts.push(format!("Claude Code {}%", pct));
            }
        } else if let Some(x) = codex_util {
            let pct = x.round() as i64;
            if config.percentage_format == PercentageFormat::Compact {
                parts.push(format!("{}", pct));
            } else {
                parts.push(format!("Codex {}%", pct));
            }
        }
    }

    // Cost
    if config.show_cost {
        if config.cost_precision == CostPrecision::Whole {
            parts.push(format!("${}", total_cost.round() as i64));
        } else {
            parts.push(format!("${:.2}", total_cost));
        }
    }

    parts.join("  ")
}

/// Must stay in sync with `src/lib/providerMetadata.ts` `primaryWindowId`.
fn primary_window_id(provider: &str) -> &'static str {
    match provider {
        "claude" => "five_hour",
        "codex" => "primary",
        "cursor" => "auto_composer",
        _ => "primary",
    }
}

fn expired_window_grace_ms(provider: &str) -> i64 {
    match provider {
        "codex" => 60_000,
        _ => 0,
    }
}

fn is_window_expired(provider: &str, resets_at: Option<&str>) -> bool {
    let grace_ms = expired_window_grace_ms(provider);
    if grace_ms == 0 {
        return false;
    }
    let Some(resets_at) = resets_at else {
        return false;
    };
    let Ok(reset_time) = chrono::DateTime::parse_from_rfc3339(resets_at) else {
        tracing::warn!(provider, resets_at, "Failed to parse resets_at as RFC 3339");
        return false;
    };
    let deadline = reset_time + chrono::Duration::milliseconds(grace_ms);
    chrono::Utc::now() >= deadline
}

fn primary_window_utilization(
    provider: &str,
    rate_limits: Option<&ProviderRateLimits>,
) -> Option<f64> {
    let windows = &rate_limits?.windows;
    let target_id = primary_window_id(provider);
    if let Some(w) = windows
        .iter()
        .find(|w| w.window_id == target_id && !is_window_expired(provider, w.resets_at.as_deref()))
    {
        return Some(w.utilization);
    }
    // Fallback: any non-expired window (matches main app behavior)
    windows
        .iter()
        .find(|w| !is_window_expired(provider, w.resets_at.as_deref()))
        .map(|w| w.utilization)
}

pub(crate) fn tray_utilization_from_rate_limits(
    payload: Option<&RateLimitsPayload>,
) -> TrayUtilization {
    TrayUtilization {
        claude: primary_window_utilization(
            "claude",
            payload.and_then(|rate_limits| rate_limits.claude.as_ref()),
        ),
        codex: primary_window_utilization(
            "codex",
            payload.and_then(|rate_limits| rate_limits.codex.as_ref()),
        ),
        cursor: primary_window_utilization(
            "cursor",
            payload.and_then(|rate_limits| rate_limits.cursor.as_ref()),
        ),
    }
}

fn merge_tray_utilization(current: TrayUtilization, patch: TrayUtilization) -> TrayUtilization {
    TrayUtilization {
        claude: patch.claude.or(current.claude),
        codex: patch.codex.or(current.codex),
        cursor: patch.cursor.or(current.cursor),
    }
}

fn current_daily_total_cost(state: &AppState) -> f64 {
    all_usage_integrations()
        .iter()
        .map(|integration_id| {
            get_provider_data(&state.parser, integration_id.as_str(), "day", 0)
                .map(|payload| payload.total_cost)
                .unwrap_or(0.0)
        })
        .sum()
}

fn usage_access_enabled(state: &AppState) -> bool {
    state
        .usage_access_enabled
        .load(std::sync::atomic::Ordering::SeqCst)
}

fn current_daily_total_cost_if_allowed(state: &AppState) -> f64 {
    if usage_access_enabled(state) {
        current_daily_total_cost(state)
    } else {
        0.0
    }
}

fn should_update_tray_icon(config: &TrayConfig, utilization: TrayUtilization) -> bool {
    config.bar_display == BarDisplay::Off || utilization.has_any()
}

#[cfg(target_os = "macos")]
fn tray_status_item_is_dark<R: Runtime>(tray: &tauri::tray::TrayIcon<R>) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAppearanceCustomization, NSAppearanceNameAccessibilityHighContrastAqua,
        NSAppearanceNameAccessibilityHighContrastDarkAqua,
        NSAppearanceNameAccessibilityHighContrastVibrantDark,
        NSAppearanceNameAccessibilityHighContrastVibrantLight, NSAppearanceNameAqua,
        NSAppearanceNameDarkAqua, NSAppearanceNameVibrantDark, NSAppearanceNameVibrantLight,
    };
    use objc2_foundation::NSArray;

    tray.with_inner_tray_icon(|inner| {
        let mtm = MainThreadMarker::new()
            .expect("tray icon appearance lookup must run on the main thread");
        let status_item = inner.ns_status_item()?;
        let button = status_item.button(mtm)?;

        let (
            aqua,
            dark_aqua,
            vibrant_dark,
            vibrant_light,
            high_contrast_aqua,
            high_contrast_dark_aqua,
            high_contrast_vibrant_light,
            high_contrast_vibrant_dark,
        ) = unsafe {
            (
                NSAppearanceNameAqua,
                NSAppearanceNameDarkAqua,
                NSAppearanceNameVibrantDark,
                NSAppearanceNameVibrantLight,
                NSAppearanceNameAccessibilityHighContrastAqua,
                NSAppearanceNameAccessibilityHighContrastDarkAqua,
                NSAppearanceNameAccessibilityHighContrastVibrantLight,
                NSAppearanceNameAccessibilityHighContrastVibrantDark,
            )
        };

        let appearance_names = NSArray::from_slice(&[
            dark_aqua,
            vibrant_dark,
            high_contrast_dark_aqua,
            high_contrast_vibrant_dark,
            aqua,
            vibrant_light,
            high_contrast_aqua,
            high_contrast_vibrant_light,
        ]);

        button
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(&appearance_names)
            .map(|matched| {
                let matched = &*matched;
                matched == dark_aqua
                    || matched == vibrant_dark
                    || matched == high_contrast_dark_aqua
                    || matched == high_contrast_vibrant_dark
            })
    })
    .ok()
    .flatten()
    .unwrap_or_else(crate::tray::render::is_menu_bar_dark)
}

fn apply_tray_presentation(
    app: &tauri::AppHandle,
    config: &TrayConfig,
    total_cost: f64,
    utilization: TrayUtilization,
    update_available: bool,
) {
    let title = format_tray_title(config, total_cost, utilization.claude, utilization.codex);

    if let Some(tray) = app.tray_by_id("main-tray") {
        // macOS: set_title() shows text beside the icon in the menu bar.
        // Windows/Linux: set_title() is a noop, but set_tooltip() works cross-platform.
        let _ = tray.set_title(Some(&title));
        let _ = tray.set_tooltip(Some(&format!("TokenMonitor: {title}")));

        if should_update_tray_icon(config, utilization) {
            let base_icon = include_bytes!("../../icons/tray-icon@2x.rgba");
            #[cfg(target_os = "macos")]
            let dark_bar = tray_status_item_is_dark(&tray);
            #[cfg(not(target_os = "macos"))]
            let dark_bar = crate::tray::render::is_menu_bar_dark();
            let (icon_buf, w, h, use_template) = crate::tray::render::render_tray_icon(
                base_icon,
                config,
                utilization.claude,
                utilization.codex,
                dark_bar,
                update_available,
            );
            let expected_size = (w * h * 4) as usize;
            if icon_buf.len() == expected_size {
                let icon = tauri::image::Image::new_owned(icon_buf, w, h);
                let _ = tray.set_icon(Some(icon));
                let _ = tray.set_icon_as_template(use_template);
            }
        }
    }
}

fn emit_status_widget_updated(app: &tauri::AppHandle) {
    let _ = app.emit("status-widget-updated", ());
}

pub(crate) async fn patch_tray_utilization(
    state: &AppState,
    patch: TrayUtilization,
) -> TrayUtilization {
    let mut current = state.tray_utilization.write().await;
    *current = merge_tray_utilization(*current, patch);
    *current
}

async fn current_tray_utilization(state: &AppState) -> TrayUtilization {
    let current = *state.tray_utilization.read().await;
    if current.has_any() {
        return current;
    }

    let cached = state.cached_rate_limits.read().await;
    tray_utilization_from_rate_limits(cached.as_ref())
}

pub async fn sync_tray_title(app: &tauri::AppHandle, state: &AppState) {
    let config = state.tray_config.read().await.clone();
    let total_cost = current_daily_total_cost_if_allowed(state);
    let utilization = current_tray_utilization(state).await;
    let update_available = {
        let guard = state.updater.read().await;
        guard.should_show_banner()
    };
    apply_tray_presentation(app, &config, total_cost, utilization, update_available);
    emit_status_widget_updated(app);
}

#[tauri::command]
pub async fn set_tray_config(
    config: TrayConfig,
    claude_util: Option<f64>,
    codex_util: Option<f64>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut current = state.tray_config.write().await;
        *current = config.clone();
    }

    let utilization = if claude_util.is_some() || codex_util.is_some() {
        patch_tray_utilization(
            &state,
            TrayUtilization {
                claude: claude_util,
                codex: codex_util,
                cursor: None,
            },
        )
        .await
    } else {
        current_tray_utilization(&state).await
    };

    let update_available = {
        let guard = state.updater.read().await;
        guard.should_show_banner()
    };
    apply_tray_presentation(
        &app,
        &config,
        current_daily_total_cost_if_allowed(&state),
        utilization,
        update_available,
    );
    emit_status_widget_updated(&app);

    Ok(())
}

#[tauri::command]
pub async fn get_status_widget_summary(
    state: tauri::State<'_, AppState>,
) -> Result<StatusWidgetSummary, String> {
    let config = state.tray_config.read().await.clone();
    let utilization = current_tray_utilization(&state).await;
    let total_cost = current_daily_total_cost_if_allowed(&state);

    Ok(StatusWidgetSummary {
        title: format_tray_title(&config, total_cost, utilization.claude, utilization.codex),
        config,
        total_cost,
        claude_util: utilization.claude,
        codex_util: utilization.codex,
        cursor_util: utilization.cursor,
    })
}

#[cfg(test)]
pub(crate) fn current_daily_total_cost_for_test(state: &AppState) -> f64 {
    current_daily_total_cost(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_tray_title_returns_empty_string_when_hidden() {
        let config = TrayConfig {
            show_cost: false,
            ..TrayConfig::default()
        };
        assert_eq!(format_tray_title(&config, 12.34, None, None), "");
    }

    #[test]
    fn format_tray_title_formats_cost_when_visible() {
        let config = TrayConfig::default(); // show_cost: true, cost_precision: "full"
        assert_eq!(format_tray_title(&config, 12.345, None, None), "$12.35");
    }

    #[test]
    fn format_tray_title_whole_cost() {
        let config = TrayConfig {
            cost_precision: CostPrecision::Whole,
            ..TrayConfig::default()
        };
        assert_eq!(format_tray_title(&config, 12.345, None, None), "$12");
    }

    #[test]
    fn format_tray_title_compact_percentages() {
        let config = TrayConfig {
            show_percentages: true,
            ..TrayConfig::default()
        };
        assert_eq!(
            format_tray_title(&config, 5.0, Some(72.0), Some(35.0)),
            "72 \u{00b7} 35  $5.00"
        );
    }

    #[test]
    fn format_tray_title_verbose_percentages() {
        let config = TrayConfig {
            show_percentages: true,
            percentage_format: PercentageFormat::Verbose,
            show_cost: false,
            ..TrayConfig::default()
        };
        assert_eq!(
            format_tray_title(&config, 0.0, Some(72.0), Some(35.0)),
            "Claude Code 72%  Codex 35%"
        );
    }

    #[test]
    fn merge_tray_utilization_keeps_existing_values_when_patch_is_empty() {
        let current = TrayUtilization {
            claude: Some(72.0),
            codex: Some(35.0),
            cursor: None,
        };

        assert_eq!(
            merge_tray_utilization(current, TrayUtilization::default()),
            current
        );
    }

    #[test]
    fn merge_tray_utilization_updates_only_present_providers() {
        let current = TrayUtilization {
            claude: Some(72.0),
            codex: Some(35.0),
            cursor: None,
        };
        let patch = TrayUtilization {
            claude: None,
            codex: Some(41.0),
            cursor: None,
        };

        assert_eq!(
            merge_tray_utilization(current, patch),
            TrayUtilization {
                claude: Some(72.0),
                codex: Some(41.0),
                cursor: None,
            }
        );
    }

    #[test]
    fn tray_utilization_from_rate_limits_extracts_primary_windows() {
        let payload = RateLimitsPayload {
            claude: Some(ProviderRateLimits {
                provider: "claude".to_string(),
                plan_tier: Some("Max 5x".to_string()),
                windows: vec![RateLimitWindow {
                    window_id: "five_hour".to_string(),
                    label: "Session (5hr)".to_string(),
                    utilization: 72.0,
                    resets_at: None,
                }],
                extra_usage: None,
                credits: None,
                stale: false,
                error: None,
                retry_after_seconds: None,
                cooldown_until: None,
                fetched_at: "2026-03-18T00:00:00Z".to_string(),
            }),
            codex: Some(ProviderRateLimits {
                provider: "codex".to_string(),
                plan_tier: Some("Pro".to_string()),
                windows: vec![RateLimitWindow {
                    window_id: "primary".to_string(),
                    label: "Session (5hr)".to_string(),
                    utilization: 35.0,
                    resets_at: None,
                }],
                extra_usage: None,
                credits: None,
                stale: false,
                error: None,
                retry_after_seconds: None,
                cooldown_until: None,
                fetched_at: "2026-03-18T00:00:00Z".to_string(),
            }),
            cursor: None,
        };

        assert_eq!(
            tray_utilization_from_rate_limits(Some(&payload)),
            TrayUtilization {
                claude: Some(72.0),
                codex: Some(35.0),
                cursor: None,
            }
        );
    }

    #[test]
    fn should_update_tray_icon_skips_bar_overwrite_without_data() {
        let config = TrayConfig::default();

        assert!(!should_update_tray_icon(
            &config,
            TrayUtilization::default()
        ));
        assert!(should_update_tray_icon(
            &config,
            TrayUtilization {
                claude: Some(72.0),
                codex: None,
                cursor: None,
            }
        ));
        assert!(should_update_tray_icon(
            &TrayConfig {
                bar_display: BarDisplay::Off,
                ..TrayConfig::default()
            },
            TrayUtilization::default(),
        ));
    }

    fn make_provider(provider: &str, windows: Vec<RateLimitWindow>) -> ProviderRateLimits {
        ProviderRateLimits {
            provider: provider.to_string(),
            plan_tier: None,
            windows,
            extra_usage: None,
            credits: None,
            stale: false,
            error: None,
            retry_after_seconds: None,
            cooldown_until: None,
            fetched_at: "2026-03-18T00:00:00Z".to_string(),
        }
    }

    fn make_window(window_id: &str, utilization: f64, resets_at: Option<&str>) -> RateLimitWindow {
        RateLimitWindow {
            window_id: window_id.to_string(),
            label: window_id.to_string(),
            utilization,
            resets_at: resets_at.map(|s| s.to_string()),
        }
    }

    #[test]
    fn primary_window_utilization_selects_by_window_id_not_index() {
        let rate_limits = make_provider(
            "claude",
            vec![
                make_window("seven_day", 10.0, None),
                make_window("five_hour", 42.0, None),
            ],
        );
        assert_eq!(
            primary_window_utilization("claude", Some(&rate_limits)),
            Some(42.0)
        );
    }

    #[test]
    fn primary_window_utilization_falls_back_when_id_missing() {
        let rate_limits = make_provider("claude", vec![make_window("seven_day", 10.0, None)]);
        assert_eq!(
            primary_window_utilization("claude", Some(&rate_limits)),
            Some(10.0)
        );
    }

    #[test]
    fn codex_expired_window_returns_none() {
        let rate_limits = make_provider(
            "codex",
            vec![make_window("primary", 35.0, Some("2020-01-01T00:00:00Z"))],
        );
        assert_eq!(
            primary_window_utilization("codex", Some(&rate_limits)),
            None
        );
    }

    #[test]
    fn claude_ignores_expired_window_grace_period() {
        let rate_limits = make_provider(
            "claude",
            vec![make_window("five_hour", 72.0, Some("2020-01-01T00:00:00Z"))],
        );
        assert_eq!(
            primary_window_utilization("claude", Some(&rate_limits)),
            Some(72.0)
        );
    }

    #[test]
    fn cursor_uses_auto_composer_window() {
        let rate_limits = make_provider(
            "cursor",
            vec![
                make_window("auto_composer", 55.0, None),
                make_window("api", 80.0, None),
            ],
        );
        assert_eq!(
            primary_window_utilization("cursor", Some(&rate_limits)),
            Some(55.0)
        );
    }

    #[test]
    fn cursor_with_only_extra_usage_returns_none() {
        let mut rate_limits = make_provider("cursor", vec![]);
        rate_limits.extra_usage = Some(ExtraUsageInfo {
            is_enabled: true,
            monthly_limit: 500.0,
            used_credits: 100.0,
            utilization: Some(20.0),
        });
        assert_eq!(
            primary_window_utilization("cursor", Some(&rate_limits)),
            None
        );
    }

    #[test]
    fn codex_multi_window_selects_primary() {
        let rate_limits = make_provider(
            "codex",
            vec![
                make_window("primary", 5.0, Some("2099-01-01T00:00:00Z")),
                make_window("secondary", 36.0, Some("2099-06-01T00:00:00Z")),
            ],
        );
        assert_eq!(
            primary_window_utilization("codex", Some(&rate_limits)),
            Some(5.0)
        );
    }
}
