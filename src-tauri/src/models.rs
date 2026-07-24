use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use crate::stats::change::{ChangeStats, ModelChangeSummary};

// ── Frontend payload (sent to Svelte via IPC) ──

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UsageSource {
    Parser,
    Mixed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UsagePayload {
    pub total_cost: f64,
    pub total_tokens: u64,
    /// 5H: number of activity blocks (split by 30-min gaps); others: buckets with nonzero cost
    pub session_count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_5m_tokens: u64,
    pub cache_write_1h_tokens: u64,
    pub web_search_requests: u64,
    pub chart_buckets: Vec<ChartBucket>,
    pub model_breakdown: Vec<ModelSummary>,
    /// 5H period only — current session burn rate and projection
    pub active_block: Option<ActiveBlock>,
    /// 5H period only — cost of the active (or total) block
    pub five_hour_cost: f64,
    pub last_updated: String,
    pub from_cache: bool,
    pub usage_source: UsageSource,
    pub usage_warning: Option<String>,
    pub period_label: String,
    pub has_earlier_data: bool,
    pub change_stats: Option<ChangeStats>,
    pub subagent_stats: Option<crate::stats::subagent::SubagentStats>,
    pub device_breakdown: Option<Vec<DeviceSummary>>,
    pub device_chart_buckets: Option<Vec<ChartBucket>>,
    pub provider_detected: Option<bool>,
    /// True when Cursor remote data is still being fetched asynchronously.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cursor_loading: bool,
}

impl Default for UsagePayload {
    fn default() -> Self {
        Self {
            total_cost: 0.0,
            total_tokens: 0,
            session_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            web_search_requests: 0,
            chart_buckets: Vec::new(),
            model_breakdown: Vec::new(),
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            usage_source: UsageSource::Parser,
            usage_warning: None,
            period_label: String::new(),
            has_earlier_data: false,
            change_stats: None,
            subagent_stats: None,
            device_breakdown: None,
            device_chart_buckets: None,
            provider_detected: None,
            cursor_loading: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChartBucket {
    pub label: String,
    pub sort_key: String,
    pub total: f64,
    pub segments: Vec<ChartSegment>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChartSegment {
    pub model: String,
    pub model_key: String,
    pub cost: f64,
    pub tokens: u64,
    #[serde(default = "default_pricing_available")]
    pub pricing_available: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelSummary {
    pub display_name: String,
    pub model_key: String,
    pub cost: f64,
    pub tokens: u64,
    #[serde(default = "default_pricing_available")]
    pub pricing_available: bool,
    pub change_stats: Option<ModelChangeSummary>,
}

fn default_pricing_available() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActiveBlock {
    pub cost: f64,
    pub burn_rate_per_hour: f64,
    pub projected_cost: f64,
    pub is_active: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct CalendarDay {
    pub day: u32,
    pub cost: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct MonthlyUsagePayload {
    pub year: i32,
    pub month: u32,
    pub days: Vec<CalendarDay>,
    pub total_cost: f64,
    pub usage_source: UsageSource,
    pub usage_warning: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct KnownModel {
    pub display_name: String,
    pub model_key: String,
}

// ── Rate limits (from provider APIs / JSONL) ──

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    pub window_id: String,
    pub label: String,
    pub utilization: f64,
    pub resets_at: Option<String>,
}

impl RateLimitWindow {
    /// Create a window with utilization already expressed as a percentage.
    pub fn new(
        window_id: String,
        label: String,
        utilization: f64,
        resets_at: Option<String>,
    ) -> Self {
        Self {
            window_id,
            label,
            utilization,
            resets_at,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExtraUsageInfo {
    pub is_enabled: bool,
    pub monthly_limit: f64,
    pub used_credits: f64,
    pub utilization: Option<f64>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreditsInfo {
    pub balance: Option<f64>,
    pub has_credits: bool,
    pub unlimited: bool,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRateLimits {
    pub provider: String,
    pub plan_tier: Option<String>,
    pub windows: Vec<RateLimitWindow>,
    pub extra_usage: Option<ExtraUsageInfo>,
    pub credits: Option<CreditsInfo>,
    pub stale: bool,
    pub error: Option<String>,
    pub retry_after_seconds: Option<u64>,
    pub cooldown_until: Option<String>,
    pub fetched_at: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitsPayload {
    pub claude: Option<ProviderRateLimits>,
    pub codex: Option<ProviderRateLimits>,
    pub cursor: Option<ProviderRateLimits>,
    pub kimi: Option<ProviderRateLimits>,
}

// ── Helpers ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamily {
    Anthropic,
    OpenAI,
    Google,
    Moonshot,
    Qwen,
    Glm,
    DeepSeek,
    Cursor,
    Unknown,
}

/// Moonshot's newer bare model names (`k2`, `k3`, `k3-...`). The Kimi Code CLI
/// logs e.g. `kimi-code/k3`, which loses the `kimi` prefix once the provider
/// namespace is stripped, so recognize the bare K-series too.
fn looks_like_moonshot_k_series(normalized: &str) -> bool {
    let Some(rest) = normalized.strip_prefix('k') else {
        return false;
    };
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return false;
    }
    matches!(rest[digits..].chars().next(), None | Some('.') | Some('-'))
}

pub fn detect_model_family(raw: &str) -> ModelFamily {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return ModelFamily::Unknown;
    }

    if normalized.contains("claude")
        || normalized.contains("fable")
        || normalized.contains("mythos")
        || normalized.contains("opus")
        || normalized.contains("sonnet")
        || normalized.contains("haiku")
    {
        return ModelFamily::Anthropic;
    }

    let bytes = normalized.as_bytes();
    let looks_like_openai_o_series =
        bytes.first() == Some(&b'o') && bytes.get(1).is_some_and(|b| b.is_ascii_digit());

    if normalized.starts_with("gpt") || looks_like_openai_o_series || normalized.contains("codex") {
        return ModelFamily::OpenAI;
    }

    if normalized.starts_with("gemini") {
        return ModelFamily::Google;
    }

    if normalized.starts_with("kimi")
        || normalized.contains("moonshot")
        || looks_like_moonshot_k_series(&normalized)
    {
        return ModelFamily::Moonshot;
    }

    if normalized.starts_with("qwen") {
        return ModelFamily::Qwen;
    }

    if normalized.starts_with("glm") || normalized.contains("zhipu") {
        return ModelFamily::Glm;
    }

    if normalized.starts_with("deepseek") {
        return ModelFamily::DeepSeek;
    }

    if normalized.starts_with("composer") {
        return ModelFamily::Cursor;
    }

    ModelFamily::Unknown
}

struct ClaudeFamily {
    lower: &'static str,
    display: &'static str,
    allow_major_only: bool,
    allow_preview: bool,
}

/// Family names for Claude model detection.
const CLAUDE_FAMILIES: &[ClaudeFamily] = &[
    ClaudeFamily {
        lower: "fable",
        display: "Fable",
        allow_major_only: true,
        allow_preview: false,
    },
    ClaudeFamily {
        lower: "mythos",
        display: "Mythos",
        allow_major_only: true,
        allow_preview: true,
    },
    ClaudeFamily {
        lower: "opus",
        display: "Opus",
        allow_major_only: false,
        allow_preview: false,
    },
    ClaudeFamily {
        lower: "sonnet",
        display: "Sonnet",
        allow_major_only: false,
        allow_preview: false,
    },
    ClaudeFamily {
        lower: "haiku",
        display: "Haiku",
        allow_major_only: false,
        allow_preview: false,
    },
];

/// Extract (major, optional minor) version from text immediately after a family
/// name. Most Claude 4.x model IDs use "-{major}-{minor}"; Fable/Mythos use
/// "-{major}".
fn extract_claude_version(
    after_family: &str,
    allow_major_only: bool,
) -> Option<(&str, Option<&str>)> {
    let rest = after_family.strip_prefix('-')?;
    let major_end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if major_end == 0 || major_end > 2 {
        return None;
    }
    let major = &rest[..major_end];
    let after_major = &rest[major_end..];
    let Some(after_dash) = after_major.strip_prefix('-') else {
        return allow_major_only.then_some((major, None));
    };
    let minor_end = after_dash
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after_dash.len());
    if minor_end == 0 || minor_end > 2 {
        return allow_major_only.then_some((major, None));
    }
    Some((major, Some(&after_dash[..minor_end])))
}

fn is_claude_preview_alias(after_family: &str) -> bool {
    after_family == "-preview"
        || after_family.starts_with("-preview-")
        || after_family.starts_with("-preview[")
}

fn looks_like_model_date(token: &str) -> bool {
    token.len() == 8 && token.starts_with("20") && token.bytes().all(|b| b.is_ascii_digit())
}

fn looks_like_provider_revision(token: &str) -> bool {
    token.len() >= 2 && token.starts_with('v') && token[1..].bytes().all(|b| b.is_ascii_digit())
}

fn title_case_token(token: &str) -> String {
    let mut chars = token.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {
            format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
        }
        Some(first) => format!("{first}{}", chars.as_str()),
        None => String::new(),
    }
}

fn normalize_raw_model_identity(raw: &str) -> (String, String) {
    let display_name = raw.trim();
    if display_name.is_empty() {
        return ("Unknown".into(), "unknown".into());
    }

    (display_name.to_string(), display_name.to_ascii_lowercase())
}

fn normalize_unknown_claude_model(
    base: &str,
    suffix_display: &str,
    suffix_key: &str,
) -> (String, String) {
    let Some(pos) = base.find("claude-") else {
        return normalize_raw_model_identity(base);
    };
    let after_claude = &base[pos + "claude-".len()..];
    let mut key_tokens = Vec::new();
    for token in after_claude
        .split(['-', '_', '.', ':'])
        .filter(|token| !token.is_empty())
    {
        if looks_like_model_date(token) || looks_like_provider_revision(token) {
            break;
        }
        if token == "latest" && !key_tokens.is_empty() {
            break;
        }
        key_tokens.push(token);
    }

    if key_tokens.is_empty() {
        return normalize_raw_model_identity(base);
    }

    let display = key_tokens
        .iter()
        .map(|token| title_case_token(token))
        .collect::<Vec<_>>()
        .join(" ");
    let key = key_tokens.join("-");
    (
        format!("{display}{suffix_display}"),
        format!("{key}{suffix_key}"),
    )
}

pub fn normalize_claude_model(raw: &str) -> (String, String) {
    let normalized = raw.trim().to_ascii_lowercase();
    let is_fast = normalized.ends_with("-fast");
    let base = if is_fast {
        &normalized[..normalized.len() - 5]
    } else {
        &normalized
    };
    let suffix_display = if is_fast { " Fast" } else { "" };
    let suffix_key = if is_fast { "-fast" } else { "" };
    for family in CLAUDE_FAMILIES {
        let family_lower = family.lower;
        let family_display = family.display;
        if let Some(pos) = base.find(family_lower) {
            let after = &base[pos + family_lower.len()..];
            if family.allow_preview && is_claude_preview_alias(after) {
                return (
                    format!("{family_display} Preview{suffix_display}"),
                    format!("{family_lower}-preview{suffix_key}"),
                );
            }
            if let Some((major, minor)) = extract_claude_version(after, family.allow_major_only) {
                if let Some(minor) = minor {
                    return (
                        format!("{family_display} {major}.{minor}{suffix_display}"),
                        format!("{family_lower}-{major}-{minor}{suffix_key}"),
                    );
                }
                return (
                    format!("{family_display} {major}{suffix_display}"),
                    format!("{family_lower}-{major}{suffix_key}"),
                );
            }
            return (
                format!("{family_display}{suffix_display}"),
                format!("{family_lower}{suffix_key}"),
            );
        }
    }
    normalize_unknown_claude_model(base, suffix_display, suffix_key)
}

/// Ordered prefix table for Codex/OpenAI models. Each entry:
/// (lowercase_prefix, display_prefix) — if the lowercased model name starts
/// with `lowercase_prefix`, the display name is rewritten with `display_prefix`.
/// The model key is always the full lowercased name.
const CODEX_PREFIXES: &[(&str, &str)] = &[("gpt", "GPT")];

pub fn normalize_codex_model(raw: &str) -> (String, String) {
    let display_name = raw.trim();
    if display_name.is_empty() {
        return ("Unknown".into(), "unknown".into());
    }

    let normalized_key = display_name.to_ascii_lowercase();
    for &(prefix_lower, display_prefix) in CODEX_PREFIXES {
        if normalized_key.starts_with(prefix_lower) {
            let suffix = &display_name[prefix_lower.len()..];
            let display = format!("{display_prefix}{}", suffix.replace('-', " "));
            return (display, normalized_key);
        }
    }

    (display_name.replace('-', " "), normalized_key)
}

fn normalize_prefixed_model(
    raw: &str,
    prefix_lower: &str,
    display_prefix: &str,
) -> (String, String) {
    let display_name = raw.trim();
    if display_name.is_empty() {
        return (String::from("Unknown"), String::from("unknown"));
    }

    let normalized_key = display_name.to_ascii_lowercase();
    let normalized_display_name = if normalized_key.starts_with(prefix_lower) {
        let suffix = &display_name[prefix_lower.len()..];
        format!("{display_prefix}{}", suffix.replace('-', " "))
    } else {
        display_name.replace('-', " ")
    };

    (normalized_display_name, normalized_key)
}

pub fn normalize_generic_model(raw: &str) -> (String, String) {
    match detect_model_family(raw) {
        ModelFamily::Google => normalize_prefixed_model(raw, "gemini", "Gemini"),
        ModelFamily::Moonshot => normalize_prefixed_model(raw, "kimi", "Kimi"),
        ModelFamily::Qwen => normalize_prefixed_model(raw, "qwen", "Qwen"),
        ModelFamily::Glm => normalize_prefixed_model(raw, "glm", "GLM"),
        ModelFamily::DeepSeek => normalize_prefixed_model(raw, "deepseek", "DeepSeek"),
        ModelFamily::Cursor => normalize_prefixed_model(raw, "composer", "Composer"),
        _ => {
            let display_name = raw.trim();
            if display_name.is_empty() {
                return (String::from("Unknown"), String::from("unknown"));
            }
            // Unknown brand: we don't know which '-' separates brand from version,
            // so keep the raw display verbatim and let the user interpret it.
            (display_name.to_string(), display_name.to_ascii_lowercase())
        }
    }
}

pub fn normalize_model(raw: &str) -> (String, String) {
    match detect_model_family(raw) {
        ModelFamily::Anthropic => normalize_claude_model(raw),
        ModelFamily::OpenAI => normalize_codex_model(raw),
        ModelFamily::Google
        | ModelFamily::Moonshot
        | ModelFamily::Qwen
        | ModelFamily::Glm
        | ModelFamily::DeepSeek
        | ModelFamily::Cursor
        | ModelFamily::Unknown => normalize_generic_model(raw),
    }
}

pub fn normalized_model_key(raw: &str) -> String {
    normalize_model(raw).1
}

#[allow(dead_code)]
pub(crate) fn is_codex_model_name(raw: &str) -> bool {
    detect_model_family(raw) == ModelFamily::OpenAI
}

// ── Device usage (per-SSH-host breakdown) ──

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceModelSummary {
    pub display_name: String,
    pub model_key: String,
    pub cost: f64,
    pub tokens: u64,
    #[serde(default = "default_pricing_available")]
    pub pricing_available: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceSummary {
    pub device: String,
    pub total_cost: f64,
    pub total_tokens: u64,
    pub model_breakdown: Vec<DeviceModelSummary>,
    pub is_local: bool,
    pub status: String,
    pub last_synced: Option<String>,
    pub error_message: Option<String>,
    pub cost_percentage: f64,
    pub include_in_stats: bool,
    /// Remote server UTC offset, e.g. `"+0800"`. `None` for local device.
    pub remote_tz: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct DeviceUsagePayload {
    pub devices: Vec<DeviceSummary>,
    pub total_cost: f64,
    pub chart_buckets: Vec<ChartBucket>,
    pub last_updated: String,
    pub period_label: String,
}

impl Default for DeviceUsagePayload {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            total_cost: 0.0,
            chart_buckets: Vec::new(),
            last_updated: Local::now().to_rfc3339(),
            period_label: String::new(),
        }
    }
}

// ── Display-name overrides ──────────────────────────────────────────────────
//
// Some CLIs log a stable *alias* rather than the real model version — Kimi Code
// records every turn as `kimi-for-coding` while the human-facing name
// ("K2.7 Code High Speed") lives only in its `config.toml`. Overrides let us
// show that real name without disturbing the normalized *key*, which pricing
// (`kimi-for-coding` fallback rate) and the chart palette (`startsWith("kimi")`)
// both depend on. Populated once at startup from the CLI's own config.

static MODEL_DISPLAY_OVERRIDES: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();

/// Register display-name overrides keyed by normalized model key. Replaces any
/// previously registered set. Keys are normalized model keys (see
/// [`normalized_model_key`]); values are the display names to show instead.
pub fn set_model_display_overrides(overrides: HashMap<String, String>) {
    let lock = MODEL_DISPLAY_OVERRIDES.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(mut guard) = lock.write() {
        *guard = overrides;
    }
}

fn display_override_for(model_key: &str) -> Option<String> {
    let guard = MODEL_DISPLAY_OVERRIDES.get()?.read().ok()?;
    guard.get(model_key).cloned()
}

pub fn known_model_from_raw(raw: &str) -> KnownModel {
    let (display_name, model_key) = normalize_model(raw);
    let display_name = display_override_for(&model_key).unwrap_or(display_name);
    KnownModel {
        display_name,
        model_key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ══════════════════════════════════════════════════════════════════════
    // normalize_claude_model — every alias branch
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn claude_opus_4_6() {
        let (d, k) = normalize_claude_model("claude-opus-4-6-20260301");
        assert_eq!((d.as_str(), k.as_str()), ("Opus 4.6", "opus-4-6"));
    }

    #[test]
    fn claude_opus_4_5() {
        let (d, k) = normalize_claude_model("claude-opus-4-5-20250501");
        assert_eq!((d.as_str(), k.as_str()), ("Opus 4.5", "opus-4-5"));
    }

    #[test]
    fn claude_sonnet_4_6() {
        let (d, k) = normalize_claude_model("claude-sonnet-4-6-20260301");
        assert_eq!((d.as_str(), k.as_str()), ("Sonnet 4.6", "sonnet-4-6"));
    }

    #[test]
    fn claude_sonnet_4_5() {
        let (d, k) = normalize_claude_model("claude-sonnet-4-5-20250514");
        assert_eq!((d.as_str(), k.as_str()), ("Sonnet 4.5", "sonnet-4-5"));
    }

    #[test]
    fn claude_haiku_4_5() {
        let (d, k) = normalize_claude_model("claude-haiku-4-5-20251001");
        assert_eq!((d.as_str(), k.as_str()), ("Haiku 4.5", "haiku-4-5"));
    }

    #[test]
    fn claude_fable_5() {
        let (d, k) = normalize_claude_model("claude-fable-5");
        assert_eq!((d.as_str(), k.as_str()), ("Fable 5", "fable-5"));
    }

    #[test]
    fn claude_mythos_5() {
        let (d, k) = normalize_claude_model("claude-mythos-5");
        assert_eq!((d.as_str(), k.as_str()), ("Mythos 5", "mythos-5"));
    }

    #[test]
    fn claude_mythos_preview() {
        let (d, k) = normalize_claude_model("claude-mythos-preview");
        assert_eq!(
            (d.as_str(), k.as_str()),
            ("Mythos Preview", "mythos-preview")
        );
    }

    #[test]
    fn claude_unknown_future_family_keeps_identity() {
        let (d, k) = normalize_claude_model("claude-cipher-5-20270101");
        assert_eq!((d.as_str(), k.as_str()), ("Cipher 5", "cipher-5"));
    }

    #[test]
    fn claude_unknown_future_family_bedrock_id_keeps_identity() {
        let (d, k) = normalize_claude_model("us.anthropic.claude-cipher-5-20270101-v1:0");
        assert_eq!((d.as_str(), k.as_str()), ("Cipher 5", "cipher-5"));
    }

    #[test]
    fn claude_haiku_generic() {
        let (d, k) = normalize_claude_model("claude-3-haiku-20240307");
        assert_eq!((d.as_str(), k.as_str()), ("Haiku", "haiku"));
    }

    #[test]
    fn claude_sonnet_generic() {
        let (d, k) = normalize_claude_model("claude-3-5-sonnet-20241022");
        assert_eq!((d.as_str(), k.as_str()), ("Sonnet", "sonnet"));
    }

    #[test]
    fn claude_opus_generic() {
        // A bare "opus" without version digits should match the generic opus alias.
        let (d, k) = normalize_claude_model("claude-3-opus-20240229");
        assert_eq!((d.as_str(), k.as_str()), ("Opus", "opus"));
    }

    #[test]
    fn claude_unknown_model() {
        let (d, k) = normalize_claude_model("some-unknown-model");
        assert_eq!(
            (d.as_str(), k.as_str()),
            ("some-unknown-model", "some-unknown-model")
        );
    }

    // ── substring specificity: longer pattern wins over shorter ──

    #[test]
    fn claude_sonnet_4_5_not_generic_sonnet() {
        // "sonnet-4-5" must match before "sonnet"
        let (d, k) = normalize_claude_model("claude-sonnet-4-5-20250514");
        assert_eq!(k.as_str(), "sonnet-4-5");
        assert_eq!(d.as_str(), "Sonnet 4.5");
    }

    #[test]
    fn claude_haiku_4_5_not_generic_haiku() {
        let (d, k) = normalize_claude_model("haiku-4-5-latest");
        assert_eq!(k.as_str(), "haiku-4-5");
        assert_eq!(d.as_str(), "Haiku 4.5");
    }

    #[test]
    fn claude_opus_4_6_not_generic_opus() {
        let (d, k) = normalize_claude_model("opus-4-6-latest");
        assert_eq!(k.as_str(), "opus-4-6");
        assert_eq!(d.as_str(), "Opus 4.6");
    }

    // ── forward-compatible: future versions auto-detected ──

    #[test]
    fn claude_opus_4_7() {
        let (d, k) = normalize_claude_model("claude-opus-4-7-20260501");
        assert_eq!((d.as_str(), k.as_str()), ("Opus 4.7", "opus-4-7"));
    }

    #[test]
    fn claude_sonnet_5_0_future() {
        let (d, k) = normalize_claude_model("claude-sonnet-5-0-20270101");
        assert_eq!((d.as_str(), k.as_str()), ("Sonnet 5.0", "sonnet-5-0"));
    }

    #[test]
    fn claude_haiku_4_7_future() {
        let (d, k) = normalize_claude_model("claude-haiku-4-7-20260601");
        assert_eq!((d.as_str(), k.as_str()), ("Haiku 4.7", "haiku-4-7"));
    }

    #[test]
    fn claude_opus_4_7_bare() {
        let (d, k) = normalize_claude_model("opus-4-7");
        assert_eq!((d.as_str(), k.as_str()), ("Opus 4.7", "opus-4-7"));
    }

    #[test]
    fn claude_fable_5_bare() {
        let (d, k) = normalize_claude_model("fable-5");
        assert_eq!((d.as_str(), k.as_str()), ("Fable 5", "fable-5"));
    }

    // ── fast mode ──

    #[test]
    fn claude_opus_4_6_fast() {
        let (d, k) = normalize_claude_model("claude-opus-4-6-fast");
        assert_eq!((d.as_str(), k.as_str()), ("Opus 4.6 Fast", "opus-4-6-fast"));
    }

    #[test]
    fn claude_sonnet_4_6_fast() {
        let (d, k) = normalize_claude_model("claude-sonnet-4-6-fast");
        assert_eq!(
            (d.as_str(), k.as_str()),
            ("Sonnet 4.6 Fast", "sonnet-4-6-fast")
        );
    }

    #[test]
    fn claude_opus_bare_fast() {
        let (d, k) = normalize_claude_model("opus-4-7-fast");
        assert_eq!((d.as_str(), k.as_str()), ("Opus 4.7 Fast", "opus-4-7-fast"));
    }

    #[test]
    fn claude_haiku_fast() {
        let (d, k) = normalize_claude_model("claude-haiku-4-5-fast");
        assert_eq!(
            (d.as_str(), k.as_str()),
            ("Haiku 4.5 Fast", "haiku-4-5-fast")
        );
    }

    // ── edge cases ──

    #[test]
    fn claude_empty_string() {
        let (d, k) = normalize_claude_model("");
        assert_eq!((d.as_str(), k.as_str()), ("Unknown", "unknown"));
    }

    #[test]
    fn claude_whitespace_only() {
        let (d, k) = normalize_claude_model("   ");
        assert_eq!((d.as_str(), k.as_str()), ("Unknown", "unknown"));
    }

    #[test]
    fn claude_case_insensitive() {
        let (d, k) = normalize_claude_model("Claude-Sonnet-4-5-20250514");
        assert_eq!((d.as_str(), k.as_str()), ("Sonnet 4.5", "sonnet-4-5"));
    }

    #[test]
    fn claude_leading_trailing_whitespace() {
        let (d, k) = normalize_claude_model("  claude-opus-4-6-20260301  ");
        assert_eq!((d.as_str(), k.as_str()), ("Opus 4.6", "opus-4-6"));
    }

    #[test]
    fn claude_mixed_case_haiku() {
        let (d, k) = normalize_claude_model("CLAUDE-HAIKU-4-5");
        assert_eq!((d.as_str(), k.as_str()), ("Haiku 4.5", "haiku-4-5"));
    }

    // ══════════════════════════════════════════════════════════════════════
    // normalize_codex_model — GPT prefix + identity passthrough
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn codex_gpt_5_4() {
        assert_eq!(
            normalize_codex_model("gpt-5.4-turbo"),
            ("GPT 5.4 turbo".into(), "gpt-5.4-turbo".into())
        );
    }

    #[test]
    fn codex_gpt_5_3() {
        assert_eq!(
            normalize_codex_model("gpt-5.3-codex"),
            ("GPT 5.3 codex".into(), "gpt-5.3-codex".into())
        );
    }

    #[test]
    fn codex_gpt_5_2() {
        assert_eq!(
            normalize_codex_model("gpt-5.2"),
            ("GPT 5.2".into(), "gpt-5.2".into())
        );
    }

    #[test]
    fn codex_gpt_5_1_codex_max() {
        assert_eq!(
            normalize_codex_model("gpt-5.1-codex-max"),
            ("GPT 5.1 codex max".into(), "gpt-5.1-codex-max".into())
        );
    }

    #[test]
    fn codex_gpt_5_1_codex_mini() {
        assert_eq!(
            normalize_codex_model("gpt-5.1-codex-mini"),
            ("GPT 5.1 codex mini".into(), "gpt-5.1-codex-mini".into())
        );
    }

    #[test]
    fn codex_gpt_5_1_codex() {
        assert_eq!(
            normalize_codex_model("gpt-5.1-codex"),
            ("GPT 5.1 codex".into(), "gpt-5.1-codex".into())
        );
    }

    #[test]
    fn codex_gpt_5_codex() {
        assert_eq!(
            normalize_codex_model("gpt-5-codex"),
            ("GPT 5 codex".into(), "gpt-5-codex".into())
        );
    }

    #[test]
    fn codex_mini_latest() {
        assert_eq!(
            normalize_codex_model("codex-mini-latest"),
            ("codex mini latest".into(), "codex-mini-latest".into())
        );
    }

    #[test]
    fn codex_o4_mini() {
        assert_eq!(
            normalize_codex_model("o4-mini-2025-04-16"),
            ("o4 mini 2025 04 16".into(), "o4-mini-2025-04-16".into())
        );
    }

    #[test]
    fn codex_o3_mini() {
        assert_eq!(
            normalize_codex_model("o3-mini-2025-01-31"),
            ("o3 mini 2025 01 31".into(), "o3-mini-2025-01-31".into())
        );
    }

    #[test]
    fn codex_o3() {
        assert_eq!(
            normalize_codex_model("o3-2025-04-16"),
            ("o3 2025 04 16".into(), "o3-2025-04-16".into())
        );
    }

    #[test]
    fn codex_o1_mini() {
        assert_eq!(
            normalize_codex_model("o1-mini-2024-09-12"),
            ("o1 mini 2024 09 12".into(), "o1-mini-2024-09-12".into())
        );
    }

    #[test]
    fn codex_o1() {
        assert_eq!(
            normalize_codex_model("o1-2024-12-17"),
            ("o1 2024 12 17".into(), "o1-2024-12-17".into())
        );
    }

    #[test]
    fn codex_fallback() {
        assert_eq!(
            normalize_codex_model("some-future-model"),
            ("some future model".into(), "some-future-model".into())
        );
    }

    // ── codex edge cases ──

    #[test]
    fn codex_empty_string() {
        assert_eq!(
            normalize_codex_model(""),
            ("Unknown".into(), "unknown".into())
        );
    }

    #[test]
    fn codex_whitespace_only() {
        assert_eq!(
            normalize_codex_model("   "),
            ("Unknown".into(), "unknown".into())
        );
    }

    #[test]
    fn codex_gpt_uppercase_preserves_suffix_case() {
        // Input has mixed case after the "gpt" prefix — suffix casing is preserved
        // in display, while key is always lowercased.
        assert_eq!(
            normalize_codex_model("GPT-5.4-Turbo"),
            ("GPT 5.4 Turbo".into(), "gpt-5.4-turbo".into())
        );
    }

    #[test]
    fn codex_leading_trailing_whitespace() {
        assert_eq!(
            normalize_codex_model("  gpt-5.2  "),
            ("GPT 5.2".into(), "gpt-5.2".into())
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // normalize_generic_model — non-Anthropic, non-OpenAI families
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn generic_gemini() {
        assert_eq!(
            normalize_generic_model("gemini-2.5-pro"),
            ("Gemini 2.5 pro".into(), "gemini-2.5-pro".into())
        );
    }

    #[test]
    fn generic_kimi() {
        assert_eq!(
            normalize_generic_model("kimi-k2"),
            ("Kimi k2".into(), "kimi-k2".into())
        );
    }

    #[test]
    fn generic_qwen() {
        // "qwen3-coder" has no hyphen immediately after the brand prefix
        // ("qwen"), so the suffix "3-coder" becomes "3 coder".
        assert_eq!(
            normalize_generic_model("qwen3-coder"),
            ("Qwen3 coder".into(), "qwen3-coder".into())
        );
    }

    #[test]
    fn generic_glm() {
        assert_eq!(
            normalize_generic_model("glm-4.5"),
            ("GLM 4.5".into(), "glm-4.5".into())
        );
    }

    #[test]
    fn generic_deepseek() {
        assert_eq!(
            normalize_generic_model("deepseek-chat"),
            ("DeepSeek chat".into(), "deepseek-chat".into())
        );
    }

    #[test]
    fn generic_composer() {
        assert_eq!(
            normalize_generic_model("composer-1"),
            ("Composer 1".into(), "composer-1".into())
        );
    }

    #[test]
    fn generic_completely_unknown() {
        // Unknown brand: we can't safely reformat hyphens, so keep verbatim.
        assert_eq!(
            normalize_generic_model("my-custom-model"),
            ("my-custom-model".into(), "my-custom-model".into())
        );
    }

    #[test]
    fn generic_empty_string() {
        assert_eq!(
            normalize_generic_model(""),
            ("Unknown".into(), "unknown".into())
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // normalize_model — dispatch routing
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn dispatch_routes_anthropic_to_claude() {
        let (d, k) = normalize_model("claude-sonnet-4-5-20250514");
        assert_eq!((d.as_str(), k.as_str()), ("Sonnet 4.5", "sonnet-4-5"));
    }

    #[test]
    fn dispatch_routes_new_anthropic_family_to_claude() {
        let (d, k) = normalize_model("claude-fable-5");
        assert_eq!((d.as_str(), k.as_str()), ("Fable 5", "fable-5"));
    }

    #[test]
    fn dispatch_routes_future_anthropic_family_without_unknown_key() {
        let (d, k) = normalize_model("claude-cipher-5-20270101");
        assert_eq!((d.as_str(), k.as_str()), ("Cipher 5", "cipher-5"));
    }

    #[test]
    fn dispatch_routes_openai_to_codex() {
        let (d, k) = normalize_model("gpt-5.3-codex");
        assert_eq!((d.as_str(), k.as_str()), ("GPT 5.3 codex", "gpt-5.3-codex"));
    }

    #[test]
    fn dispatch_routes_o_series_to_codex() {
        let (d, k) = normalize_model("o3-2025-04-16");
        assert_eq!((d.as_str(), k.as_str()), ("o3 2025 04 16", "o3-2025-04-16"));
    }

    #[test]
    fn dispatch_routes_gemini_to_generic() {
        let (d, k) = normalize_model("gemini-2.5-pro");
        assert_eq!(
            (d.as_str(), k.as_str()),
            ("Gemini 2.5 pro", "gemini-2.5-pro")
        );
    }

    #[test]
    fn dispatch_routes_deepseek_to_generic() {
        let (d, k) = normalize_model("deepseek-chat");
        assert_eq!((d.as_str(), k.as_str()), ("DeepSeek chat", "deepseek-chat"));
    }

    #[test]
    fn dispatch_routes_unknown_to_generic() {
        let (d, k) = normalize_model("my-custom-model");
        assert_eq!(
            (d.as_str(), k.as_str()),
            ("my-custom-model", "my-custom-model")
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // normalized_model_key — returns only the key
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn model_key_for_claude() {
        assert_eq!(normalized_model_key("claude-opus-4-6-20260301"), "opus-4-6");
    }

    #[test]
    fn model_key_for_codex() {
        assert_eq!(normalized_model_key("gpt-5.1-codex"), "gpt-5.1-codex");
    }

    #[test]
    fn model_key_for_generic() {
        assert_eq!(normalized_model_key("gemini-2.5-pro"), "gemini-2.5-pro");
    }

    // ══════════════════════════════════════════════════════════════════════
    // detect_model_family
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn detects_anthropic_family() {
        assert_eq!(detect_model_family("claude-3-opus"), ModelFamily::Anthropic);
        assert_eq!(
            detect_model_family("claude-sonnet-4-5"),
            ModelFamily::Anthropic
        );
        assert_eq!(detect_model_family("fable-5"), ModelFamily::Anthropic);
        assert_eq!(detect_model_family("mythos-5"), ModelFamily::Anthropic);
        assert_eq!(detect_model_family("haiku-latest"), ModelFamily::Anthropic);
    }

    #[test]
    fn detects_openai_family() {
        assert_eq!(detect_model_family("gpt-5.3-codex"), ModelFamily::OpenAI);
        assert_eq!(detect_model_family("o3-2025-04-16"), ModelFamily::OpenAI);
        assert_eq!(detect_model_family("o1-mini"), ModelFamily::OpenAI);
        assert_eq!(
            detect_model_family("codex-mini-latest"),
            ModelFamily::OpenAI
        );
    }

    #[test]
    fn detects_non_openai_non_anthropic_model_families() {
        assert_eq!(detect_model_family("gemini-2.5-pro"), ModelFamily::Google);
        assert_eq!(detect_model_family("kimi-k2"), ModelFamily::Moonshot);
        assert_eq!(detect_model_family("qwen3-coder"), ModelFamily::Qwen);
        assert_eq!(detect_model_family("glm-4.5"), ModelFamily::Glm);
        assert_eq!(detect_model_family("deepseek-chat"), ModelFamily::DeepSeek);
        assert_eq!(detect_model_family("composer-1"), ModelFamily::Cursor);
    }

    #[test]
    fn detects_unknown_family() {
        assert_eq!(detect_model_family("my-custom-model"), ModelFamily::Unknown);
    }

    #[test]
    fn detects_empty_as_unknown() {
        assert_eq!(detect_model_family(""), ModelFamily::Unknown);
    }

    #[test]
    fn detects_moonshot_by_keyword() {
        assert_eq!(
            detect_model_family("some-moonshot-model"),
            ModelFamily::Moonshot
        );
    }

    #[test]
    fn detects_moonshot_bare_k_series() {
        // Kimi Code CLI logs `kimi-code/k3`; after the provider namespace is
        // stripped the bare K-series name must still classify as Moonshot.
        assert_eq!(detect_model_family("k3"), ModelFamily::Moonshot);
        assert_eq!(detect_model_family("k2"), ModelFamily::Moonshot);
        assert_eq!(detect_model_family("k3-instruct"), ModelFamily::Moonshot);
        // A leading "k" alone (or non-numeric suffix) must not match.
        assert_ne!(detect_model_family("kotlin-model"), ModelFamily::Moonshot);
    }

    #[test]
    fn detects_zhipu_as_glm() {
        assert_eq!(detect_model_family("zhipu-chat"), ModelFamily::Glm);
    }

    // ══════════════════════════════════════════════════════════════════════
    // known_model_from_raw
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn known_model_from_raw_uses_dynamic_codex_identity() {
        assert_eq!(
            known_model_from_raw("gpt-5.3-codex"),
            KnownModel {
                display_name: "GPT 5.3 codex".into(),
                model_key: "gpt-5.3-codex".into(),
            }
        );
    }

    #[test]
    fn generic_models_keep_identity_without_being_forced_into_claude_or_codex() {
        assert_eq!(
            known_model_from_raw("glm-4.5"),
            KnownModel {
                display_name: "GLM 4.5".into(),
                model_key: "glm-4.5".into(),
            }
        );
        assert_eq!(
            known_model_from_raw("gemini-2.5-pro"),
            KnownModel {
                display_name: "Gemini 2.5 pro".into(),
                model_key: "gemini-2.5-pro".into(),
            }
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // is_codex_model_name
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn is_codex_true_for_gpt() {
        assert!(is_codex_model_name("gpt-5.3-codex"));
    }

    #[test]
    fn is_codex_true_for_o_series() {
        assert!(is_codex_model_name("o3-2025-04-16"));
    }

    #[test]
    fn is_codex_false_for_anthropic() {
        assert!(!is_codex_model_name("claude-sonnet-4-5"));
    }

    #[test]
    fn is_codex_false_for_unknown() {
        assert!(!is_codex_model_name("my-custom-model"));
    }
}
