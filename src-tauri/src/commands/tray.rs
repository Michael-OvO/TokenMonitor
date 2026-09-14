use super::usage_query::{get_provider_data, spawn_cursor_remote_fetch_if_needed};
use super::AppState;
use crate::models::*;
use crate::usage::integrations::UsageIntegrationId;
use chrono::Local;
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
    Custom,
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
    /// Providers shown as bars on the floating ball. Separate from
    /// `bar_providers` (menu-bar/tray) so the two surfaces can diverge.
    #[serde(default)]
    pub float_ball_bar_providers: Vec<String>,
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
            float_ball_bar_providers: vec![
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
    pub(crate) kimi: Option<f64>,
}

impl TrayUtilization {
    pub(crate) fn has_any(self) -> bool {
        self.claude.is_some()
            || self.codex.is_some()
            || self.cursor.is_some()
            || self.kimi.is_some()
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
    pub kimi_util: Option<f64>,
    pub title: String,
    /// `total_cost` already rendered in the user's currency. The float ball is
    /// a separate webview that never loads the settings store, so it cannot
    /// format this itself — it would always print dollars.
    pub cost_text: String,
}

/// Rate-limit providers in display order, mirroring the frontend's
/// `RATE_LIMIT_PROVIDER_ORDER`. Used when the user has not narrowed the
/// selection, so a newly supported provider shows up by default.
const RATE_LIMIT_PROVIDER_ORDER: &[&str] = &["claude", "codex", "cursor", "kimi"];

/// Menu-bar title for a provider, matching `getUsageProviderTitle` on the
/// frontend so the settings preview and the real tray agree.
fn provider_title(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" => Some("Claude Code"),
        "codex" => Some("Codex"),
        "cursor" => Some("Cursor IDE"),
        _ => None,
    }
}

fn utilization_for(utilization: TrayUtilization, provider: &str) -> Option<f64> {
    match provider {
        "claude" => utilization.claude,
        "codex" => utilization.codex,
        "cursor" => utilization.cursor,
        _ => None,
    }
}

fn format_tray_title(config: &TrayConfig, total_cost: f64, utilization: TrayUtilization) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Percentages -- independent of bar_display.
    // Utilization values are already 0-100.
    if config.show_percentages {
        // Follow the same provider selection the bars use. This was a
        // hardcoded claude/codex pair, which silently dropped Cursor and
        // ignored the user's provider choice entirely.
        let providers: Vec<&str> = if config.bar_providers.is_empty() {
            RATE_LIMIT_PROVIDER_ORDER.to_vec()
        } else {
            config.bar_providers.iter().map(String::as_str).collect()
        };

        let entries: Vec<(&str, i64)> = providers
            .iter()
            .filter_map(|provider| {
                let title = provider_title(provider)?;
                let util = utilization_for(utilization, provider)?;
                Some((title, util.round() as i64))
            })
            .collect();

        if !entries.is_empty() {
            if config.percentage_format == PercentageFormat::Compact {
                let joined = entries
                    .iter()
                    .map(|(_, pct)| pct.to_string())
                    .collect::<Vec<_>>()
                    .join(" \u{00b7} ");
                parts.push(joined);
            } else {
                let joined = entries
                    .iter()
                    .map(|(title, pct)| format!("{title} {pct}%"))
                    .collect::<Vec<_>>()
                    .join("  ");
                parts.push(joined);
            }
        }
    }

    // Cost. Rust draws the tray itself, so it has to do the currency
    // conversion the main webview's `formatCost` does — see `usage::money`.
    if config.show_cost {
        parts.push(crate::usage::money::format(
            total_cost,
            config.cost_precision == CostPrecision::Whole,
        ));
    }

    parts.join("  ")
}

/// Must stay in sync with `src/lib/providerMetadata.ts` `primaryWindowId`.
fn primary_window_id(provider: &str) -> &'static str {
    match provider {
        "claude" => "five_hour",
        "codex" => "primary",
        "cursor" => "first_party",
        "kimi" => "five_hour",
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
        kimi: primary_window_utilization(
            "kimi",
            payload.and_then(|rate_limits| rate_limits.kimi.as_ref()),
        ),
    }
}

fn merge_tray_utilization(current: TrayUtilization, patch: TrayUtilization) -> TrayUtilization {
    TrayUtilization {
        claude: patch.claude.or(current.claude),
        codex: patch.codex.or(current.codex),
        cursor: patch.cursor.or(current.cursor),
        kimi: patch.kimi.or(current.kimi),
    }
}

fn current_daily_total_cost(state: &AppState) -> f64 {
    let enabled = state
        .enabled_integrations
        .read()
        .map(|ids| ids.clone())
        .unwrap_or_else(|poisoned| poisoned.into_inner().clone());
    enabled
        .iter()
        .map(|integration_id| {
            get_provider_data(&state.parser, integration_id.as_str(), "day", 0)
                .map(|payload| payload.total_cost)
                .unwrap_or(0.0)
        })
        .sum()
}

/// Decide which daily cost the tray should show, and what to remember next.
///
/// While a Cursor remote refetch is needed, never paint an undercount: keep the
/// previous non-zero cost when the cold recompute is lower (full `$0` *or* a
/// partial drop after Cursor falls out of the sum). Also avoid poisoning `last`
/// with a pending zero/undercount.
fn resolve_tray_daily_cost_display(
    usage_access: bool,
    computed: f64,
    needs_cursor_remote: bool,
    last: Option<f64>,
) -> (f64, Option<f64>) {
    if !usage_access {
        return (0.0, None);
    }
    if needs_cursor_remote {
        if let Some(previous) = last.filter(|value| *value > 0.0) {
            if computed < previous {
                return (previous, last);
            }
        }
        if computed <= 0.0 {
            return (computed, last);
        }
    }
    (computed, Some(computed))
}

fn current_daily_total_cost_if_allowed(state: &AppState) -> f64 {
    let last = state
        .last_tray_daily_cost
        .lock()
        .ok()
        .and_then(|guard| *guard);
    let today = Local::now().date_naive();
    let (display, next_last) = resolve_tray_daily_cost_display(
        state.usage_access_enabled(),
        if state.usage_access_enabled() {
            current_daily_total_cost(state)
        } else {
            0.0
        },
        state.parser.needs_cursor_remote_fetch(Some(today)),
        last,
    );
    if let Ok(mut guard) = state.last_tray_daily_cost.lock() {
        *guard = next_last;
    }
    display
}

fn ensure_tray_cursor_remote_fresh(app: &tauri::AppHandle, state: &AppState) {
    if !state.usage_access_enabled() {
        return;
    }
    let today = Local::now().date_naive();
    spawn_cursor_remote_fetch_if_needed(app, state, Some(today));
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

/// Push the current title/icon onto the tray icon.
///
/// Every caller is async and therefore runs on a tokio worker, but the handle
/// `tray_by_id` hands back owns the platform status item and is reference
/// counted non-atomically (`Rc`) behind Tauri's `Send` wrapper. Cloning and
/// dropping it off the main thread races the main thread's own clones, and a
/// lost decrement drops the last reference on the worker — which tears the
/// NSStatusItem down from a non-main thread and trips an AppKit assertion
/// (`EXC_BREAKPOINT` inside `-[NSStatusBar removeStatusItem:]`). So hop to the
/// main thread before touching the handle at all.
fn apply_tray_presentation(
    app: &tauri::AppHandle,
    config: &TrayConfig,
    total_cost: f64,
    utilization: TrayUtilization,
    update_available: bool,
) {
    let app_handle = app.clone();
    let config = config.clone();
    let _ = app.run_on_main_thread(move || {
        apply_tray_presentation_on_main_thread(
            &app_handle,
            &config,
            total_cost,
            utilization,
            update_available,
        );
    });
}

/// Main-thread half of [`apply_tray_presentation`]. Never call directly.
fn apply_tray_presentation_on_main_thread(
    app: &tauri::AppHandle,
    config: &TrayConfig,
    total_cost: f64,
    utilization: TrayUtilization,
    update_available: bool,
) {
    let title = format_tray_title(config, total_cost, utilization);

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
                utilization.cursor,
                utilization.kimi,
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

pub(crate) async fn apply_tray_title_now(app: &tauri::AppHandle, state: &AppState) {
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

pub async fn sync_tray_title(app: &tauri::AppHandle, state: &AppState) {
    ensure_tray_cursor_remote_fresh(app, state);
    apply_tray_title_now(app, state).await;
}

/// Refresh the tray title after a Cursor remote fetch without spawning another fetch.
pub async fn refresh_tray_title_after_cursor_fetch(app: &tauri::AppHandle, state: &AppState) {
    apply_tray_title_now(app, state).await;
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
                kimi: None,
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
    ensure_tray_cursor_remote_fresh(&app, &state);
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

/// Validate the ids the frontend sends for the enabled Header Tabs. Order is
/// preserved, duplicates dropped; an empty list is rejected because the
/// settings UI never lets the last tab be disabled.
pub(crate) fn parse_enabled_integrations(
    ids: &[String],
) -> Result<Vec<UsageIntegrationId>, String> {
    let mut parsed = Vec::new();
    for id in ids {
        let integration = UsageIntegrationId::parse(id)
            .ok_or_else(|| format!("Unknown usage integration: {id}"))?;
        if !parsed.contains(&integration) {
            parsed.push(integration);
        }
    }
    if parsed.is_empty() {
        return Err(String::from(
            "At least one usage integration must stay enabled",
        ));
    }
    Ok(parsed)
}

/// Mirror of Settings → Header Tabs: only these integrations count toward
/// the menu-bar cost. Resyncs the tray title immediately.
#[tauri::command]
pub async fn set_enabled_integrations(
    ids: Vec<String>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let parsed = parse_enabled_integrations(&ids)?;
    {
        let mut current = state
            .enabled_integrations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = parsed;
    }
    sync_tray_title(&app, &state).await;
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
        title: format_tray_title(&config, total_cost, utilization),
        cost_text: crate::usage::money::format_compact(total_cost),
        config,
        total_cost,
        claude_util: utilization.claude,
        codex_util: utilization.codex,
        cursor_util: utilization.cursor,
        kimi_util: utilization.kimi,
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
    fn tray_cost_holdover_keeps_last_while_cursor_remote_pending() {
        let (display, next) = resolve_tray_daily_cost_display(true, 0.0, true, Some(12.34));
        assert_eq!(display, 12.34);
        assert_eq!(next, Some(12.34));
    }

    #[test]
    fn tray_cost_holdover_keeps_last_on_partial_undercount() {
        // Claude/Codex still contribute a little after Cursor drops out of the sum.
        let (display, next) = resolve_tray_daily_cost_display(true, 3.0, true, Some(47.0));
        assert_eq!(display, 47.0);
        assert_eq!(next, Some(47.0));
    }

    #[test]
    fn tray_cost_holdover_does_not_poison_last_with_pending_zero() {
        let (display, next) = resolve_tray_daily_cost_display(true, 0.0, true, None);
        assert_eq!(display, 0.0);
        assert_eq!(next, None);
    }

    #[test]
    fn tray_cost_holdover_updates_when_compute_succeeds() {
        let (display, next) = resolve_tray_daily_cost_display(true, 8.5, false, Some(12.34));
        assert_eq!(display, 8.5);
        assert_eq!(next, Some(8.5));
    }

    #[test]
    fn tray_cost_holdover_clears_when_usage_access_disabled() {
        let (display, next) = resolve_tray_daily_cost_display(false, 0.0, true, Some(12.34));
        assert_eq!(display, 0.0);
        assert_eq!(next, None);
    }

    fn utils(claude: Option<f64>, codex: Option<f64>, cursor: Option<f64>) -> TrayUtilization {
        TrayUtilization {
            claude,
            codex,
            cursor,
            kimi: None,
        }
    }

    #[test]
    fn format_tray_title_returns_empty_string_when_hidden() {
        let config = TrayConfig {
            show_cost: false,
            ..TrayConfig::default()
        };
        assert_eq!(
            format_tray_title(&config, 12.34, utils(None, None, None)),
            ""
        );
    }

    #[test]
    fn format_tray_title_formats_cost_when_visible() {
        let _guard = crate::usage::money::CurrencyGuard::new("USD");
        let config = TrayConfig::default(); // show_cost: true, cost_precision: "full"
        assert_eq!(
            format_tray_title(&config, 12.345, utils(None, None, None)),
            "$12.35"
        );
    }

    /// The tray is drawn by Rust, so it never saw the currency the user picked
    /// in Settings: a EUR user got `€4.60` in the popover and `$5.00` in the
    /// menu bar at the same moment.
    #[test]
    fn format_tray_title_follows_the_selected_currency() {
        let _guard = crate::usage::money::CurrencyGuard::new("EUR");
        crate::usage::exchange_rates::set_exchange_rates(
            [("EUR".to_string(), 0.92)].into_iter().collect(),
        );
        let config = TrayConfig::default();
        assert_eq!(
            format_tray_title(&config, 5.0, utils(None, None, None)),
            "\u{20ac}4.60"
        );
    }

    #[test]
    fn format_tray_title_whole_cost() {
        let _guard = crate::usage::money::CurrencyGuard::new("USD");
        let config = TrayConfig {
            cost_precision: CostPrecision::Whole,
            ..TrayConfig::default()
        };
        assert_eq!(
            format_tray_title(&config, 12.345, utils(None, None, None)),
            "$12"
        );
    }

    #[test]
    fn format_tray_title_compact_percentages() {
        let _guard = crate::usage::money::CurrencyGuard::new("USD");
        let config = TrayConfig {
            show_percentages: true,
            ..TrayConfig::default()
        };
        assert_eq!(
            format_tray_title(&config, 5.0, utils(Some(72.0), Some(35.0), None)),
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
            format_tray_title(&config, 0.0, utils(Some(72.0), Some(35.0), None)),
            "Claude Code 72%  Codex 35%"
        );
    }

    /// Cursor used to be dropped on the floor here: the title was built from a
    /// hardcoded claude/codex pair, so a third provider could never appear.
    #[test]
    fn format_tray_title_includes_every_provider() {
        let config = TrayConfig {
            show_percentages: true,
            percentage_format: PercentageFormat::Verbose,
            show_cost: false,
            ..TrayConfig::default()
        };
        assert_eq!(
            format_tray_title(&config, 0.0, utils(Some(72.0), Some(35.0), Some(9.0))),
            "Claude Code 72%  Codex 35%  Cursor IDE 9%"
        );

        let compact = TrayConfig {
            percentage_format: PercentageFormat::Compact,
            ..config
        };
        assert_eq!(
            format_tray_title(&compact, 0.0, utils(Some(72.0), Some(35.0), Some(9.0))),
            "72 \u{00b7} 35 \u{00b7} 9"
        );
    }

    /// Percentages follow the provider selection the bars use, so unticking a
    /// provider in settings removes its number too.
    #[test]
    fn format_tray_title_honours_the_provider_selection() {
        let config = TrayConfig {
            show_percentages: true,
            percentage_format: PercentageFormat::Verbose,
            show_cost: false,
            bar_providers: vec!["cursor".to_string(), "claude".to_string()],
            ..TrayConfig::default()
        };
        assert_eq!(
            format_tray_title(&config, 0.0, utils(Some(72.0), Some(35.0), Some(9.0))),
            "Cursor IDE 9%  Claude Code 72%"
        );
    }

    /// A provider with no data yet is skipped rather than rendered as 0%.
    #[test]
    fn format_tray_title_skips_providers_without_data() {
        let config = TrayConfig {
            show_percentages: true,
            percentage_format: PercentageFormat::Verbose,
            show_cost: false,
            ..TrayConfig::default()
        };
        assert_eq!(
            format_tray_title(&config, 0.0, utils(Some(72.0), None, Some(9.0))),
            "Claude Code 72%  Cursor IDE 9%"
        );
    }

    #[test]
    fn merge_tray_utilization_keeps_existing_values_when_patch_is_empty() {
        let current = TrayUtilization {
            claude: Some(72.0),
            codex: Some(35.0),
            cursor: None,
            kimi: None,
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
            kimi: None,
        };
        let patch = TrayUtilization {
            claude: None,
            codex: Some(41.0),
            cursor: None,
            kimi: None,
        };

        assert_eq!(
            merge_tray_utilization(current, patch),
            TrayUtilization {
                claude: Some(72.0),
                codex: Some(41.0),
                cursor: None,
                kimi: None,
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
            kimi: None,
        };

        assert_eq!(
            tray_utilization_from_rate_limits(Some(&payload)),
            TrayUtilization {
                claude: Some(72.0),
                codex: Some(35.0),
                cursor: None,
                kimi: None,
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
                kimi: None,
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
    fn cursor_uses_first_party_window() {
        let rate_limits = make_provider(
            "cursor",
            vec![
                make_window("first_party", 55.0, None),
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

    #[test]
    fn parse_enabled_integrations_validates_and_dedupes() {
        assert!(parse_enabled_integrations(&[]).is_err());
        assert!(parse_enabled_integrations(&["gemini".to_string()]).is_err());
        assert_eq!(
            parse_enabled_integrations(&[
                "codex".to_string(),
                "codex".to_string(),
                "claude".to_string()
            ])
            .unwrap(),
            vec![UsageIntegrationId::Codex, UsageIntegrationId::Claude]
        );
    }

    #[test]
    fn current_daily_total_cost_sums_only_enabled_integrations() {
        use crate::usage::parser::UsageParser;
        use std::sync::Arc;
        use tempfile::TempDir;

        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let now = chrono::Local::now();
        let ts = now.to_rfc3339();

        std::fs::write(
            claude_dir.path().join("session.jsonl"),
            format!(
                r#"{{"type":"assistant","timestamp":"{ts}","message":{{"model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":1000,"output_tokens":500}},"stop_reason":"end_turn"}}}}"#
            ),
        )
        .unwrap();
        let codex_day_dir = codex_dir
            .path()
            .join(now.format("%Y").to_string())
            .join(now.format("%m").to_string())
            .join(now.format("%d").to_string());
        std::fs::create_dir_all(&codex_day_dir).unwrap();
        std::fs::write(
            codex_day_dir.join("session.jsonl"),
            format!(
                "{{\"type\":\"turn_context\",\"payload\":{{\"cwd\":\"/tmp/demo\",\"model\":\"gpt-5-codex\"}}}}\n{{\"type\":\"event_msg\",\"timestamp\":\"{ts}\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{\"input_tokens\":800,\"output_tokens\":400,\"reasoning_output_tokens\":0,\"cached_input_tokens\":0}}}}}}}}"
            ),
        )
        .unwrap();

        let mut state = AppState::new();
        state.parser = Arc::new(UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        ));

        let everything = current_daily_total_cost(&state);
        let claude_only = get_provider_data(&state.parser, "claude", "day", 0)
            .unwrap()
            .total_cost;
        assert!(claude_only > 0.0, "fixture must produce Claude cost today");
        assert!(everything > claude_only, "fixture must include Codex cost");

        *state.enabled_integrations.write().unwrap() = vec![UsageIntegrationId::Claude];
        assert!((current_daily_total_cost(&state) - claude_only).abs() < 1e-9);
    }
}
