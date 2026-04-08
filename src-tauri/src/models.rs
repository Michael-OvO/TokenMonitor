use chrono::Local;
use serde::Serialize;

use crate::stats::change::{ChangeStats, ModelChangeSummary};

// ── Frontend payload (sent to Svelte via IPC) ──

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UsageSource {
    Ccusage,
    Parser,
    Mixed,
}

#[derive(Debug, Serialize, Clone)]
pub struct UsagePayload {
    pub total_cost: f64,
    pub total_tokens: u64,
    pub session_count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub chart_buckets: Vec<ChartBucket>,
    pub model_breakdown: Vec<ModelSummary>,
    pub active_block: Option<ActiveBlock>,
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
}

impl Default for UsagePayload {
    fn default() -> Self {
        Self {
            total_cost: 0.0,
            total_tokens: 0,
            session_count: 0,
            input_tokens: 0,
            output_tokens: 0,
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
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct ChartBucket {
    pub label: String,
    pub sort_key: String,
    pub total: f64,
    pub segments: Vec<ChartSegment>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ChartSegment {
    pub model: String,
    pub model_key: String,
    pub cost: f64,
    pub tokens: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct ModelSummary {
    pub display_name: String,
    pub model_key: String,
    pub cost: f64,
    pub tokens: u64,
    pub change_stats: Option<ModelChangeSummary>,
}

#[derive(Debug, Serialize, Clone)]
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
pub struct ProviderRateLimits {
    pub provider: String,
    pub plan_tier: Option<String>,
    pub windows: Vec<RateLimitWindow>,
    pub extra_usage: Option<ExtraUsageInfo>,
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
    Unknown,
}

pub fn detect_model_family(raw: &str) -> ModelFamily {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return ModelFamily::Unknown;
    }

    if normalized.contains("claude")
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

    if normalized.starts_with("kimi") || normalized.contains("moonshot") {
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

    ModelFamily::Unknown
}

/// Ordered alias table for Claude models. Longer/more-specific patterns come
/// before shorter ones so that the first `contains` hit wins.
/// Each entry: (substring_pattern, display_name, model_key).
const CLAUDE_ALIASES: &[(&str, &str, &str)] = &[
    ("opus-4-6", "Opus 4.6", "opus-4-6"),
    ("opus-4-5", "Opus 4.5", "opus-4-5"),
    ("sonnet-4-6", "Sonnet 4.6", "sonnet-4-6"),
    ("sonnet-4-5", "Sonnet 4.5", "sonnet-4-5"),
    ("haiku-4-5", "Haiku 4.5", "haiku-4-5"),
    ("haiku", "Haiku", "haiku"),
    ("sonnet", "Sonnet", "sonnet"),
    ("opus", "Opus", "opus"),
];

pub fn normalize_claude_model(raw: &str) -> (String, String) {
    let normalized = raw.trim().to_ascii_lowercase();
    for &(pattern, display, key) in CLAUDE_ALIASES {
        if normalized.contains(pattern) {
            return (display.into(), key.into());
        }
    }
    ("Unknown".into(), "unknown".into())
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
            let display = format!("{display_prefix}{}", &display_name[prefix_lower.len()..]);
            return (display, normalized_key);
        }
    }

    (display_name.to_string(), normalized_key)
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
        format!("{display_prefix}{}", &display_name[prefix_lower.len()..])
    } else {
        display_name.to_string()
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
        _ => {
            let display_name = raw.trim();
            if display_name.is_empty() {
                return (String::from("Unknown"), String::from("unknown"));
            }
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

#[derive(Debug, Serialize, Clone)]
pub struct DeviceModelSummary {
    pub display_name: String,
    pub model_key: String,
    pub cost: f64,
    pub tokens: u64,
}

#[derive(Debug, Serialize, Clone)]
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

pub fn known_model_from_raw(raw: &str) -> KnownModel {
    let (display_name, model_key) = normalize_model(raw);
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
        assert_eq!((d.as_str(), k.as_str()), ("Unknown", "unknown"));
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
            ("GPT-5.4-turbo".into(), "gpt-5.4-turbo".into())
        );
    }

    #[test]
    fn codex_gpt_5_3() {
        assert_eq!(
            normalize_codex_model("gpt-5.3-codex"),
            ("GPT-5.3-codex".into(), "gpt-5.3-codex".into())
        );
    }

    #[test]
    fn codex_gpt_5_2() {
        assert_eq!(
            normalize_codex_model("gpt-5.2"),
            ("GPT-5.2".into(), "gpt-5.2".into())
        );
    }

    #[test]
    fn codex_gpt_5_1_codex_max() {
        assert_eq!(
            normalize_codex_model("gpt-5.1-codex-max"),
            ("GPT-5.1-codex-max".into(), "gpt-5.1-codex-max".into())
        );
    }

    #[test]
    fn codex_gpt_5_1_codex_mini() {
        assert_eq!(
            normalize_codex_model("gpt-5.1-codex-mini"),
            ("GPT-5.1-codex-mini".into(), "gpt-5.1-codex-mini".into())
        );
    }

    #[test]
    fn codex_gpt_5_1_codex() {
        assert_eq!(
            normalize_codex_model("gpt-5.1-codex"),
            ("GPT-5.1-codex".into(), "gpt-5.1-codex".into())
        );
    }

    #[test]
    fn codex_gpt_5_codex() {
        assert_eq!(
            normalize_codex_model("gpt-5-codex"),
            ("GPT-5-codex".into(), "gpt-5-codex".into())
        );
    }

    #[test]
    fn codex_mini_latest() {
        assert_eq!(
            normalize_codex_model("codex-mini-latest"),
            ("codex-mini-latest".into(), "codex-mini-latest".into())
        );
    }

    #[test]
    fn codex_o4_mini() {
        assert_eq!(
            normalize_codex_model("o4-mini-2025-04-16"),
            ("o4-mini-2025-04-16".into(), "o4-mini-2025-04-16".into())
        );
    }

    #[test]
    fn codex_o3_mini() {
        assert_eq!(
            normalize_codex_model("o3-mini-2025-01-31"),
            ("o3-mini-2025-01-31".into(), "o3-mini-2025-01-31".into())
        );
    }

    #[test]
    fn codex_o3() {
        assert_eq!(
            normalize_codex_model("o3-2025-04-16"),
            ("o3-2025-04-16".into(), "o3-2025-04-16".into())
        );
    }

    #[test]
    fn codex_o1_mini() {
        assert_eq!(
            normalize_codex_model("o1-mini-2024-09-12"),
            ("o1-mini-2024-09-12".into(), "o1-mini-2024-09-12".into())
        );
    }

    #[test]
    fn codex_o1() {
        assert_eq!(
            normalize_codex_model("o1-2024-12-17"),
            ("o1-2024-12-17".into(), "o1-2024-12-17".into())
        );
    }

    #[test]
    fn codex_fallback() {
        assert_eq!(
            normalize_codex_model("some-future-model"),
            ("some-future-model".into(), "some-future-model".into())
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
            ("GPT-5.4-Turbo".into(), "gpt-5.4-turbo".into())
        );
    }

    #[test]
    fn codex_leading_trailing_whitespace() {
        assert_eq!(
            normalize_codex_model("  gpt-5.2  "),
            ("GPT-5.2".into(), "gpt-5.2".into())
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // normalize_generic_model — non-Anthropic, non-OpenAI families
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn generic_gemini() {
        assert_eq!(
            normalize_generic_model("gemini-2.5-pro"),
            ("Gemini-2.5-pro".into(), "gemini-2.5-pro".into())
        );
    }

    #[test]
    fn generic_kimi() {
        assert_eq!(
            normalize_generic_model("kimi-k2"),
            ("Kimi-k2".into(), "kimi-k2".into())
        );
    }

    #[test]
    fn generic_qwen() {
        assert_eq!(
            normalize_generic_model("qwen3-coder"),
            ("Qwen3-coder".into(), "qwen3-coder".into())
        );
    }

    #[test]
    fn generic_glm() {
        assert_eq!(
            normalize_generic_model("glm-4.5"),
            ("GLM-4.5".into(), "glm-4.5".into())
        );
    }

    #[test]
    fn generic_deepseek() {
        assert_eq!(
            normalize_generic_model("deepseek-chat"),
            ("DeepSeek-chat".into(), "deepseek-chat".into())
        );
    }

    #[test]
    fn generic_completely_unknown() {
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
    fn dispatch_routes_openai_to_codex() {
        let (d, k) = normalize_model("gpt-5.3-codex");
        assert_eq!((d.as_str(), k.as_str()), ("GPT-5.3-codex", "gpt-5.3-codex"));
    }

    #[test]
    fn dispatch_routes_o_series_to_codex() {
        let (d, k) = normalize_model("o3-2025-04-16");
        assert_eq!((d.as_str(), k.as_str()), ("o3-2025-04-16", "o3-2025-04-16"));
    }

    #[test]
    fn dispatch_routes_gemini_to_generic() {
        let (d, k) = normalize_model("gemini-2.5-pro");
        assert_eq!(
            (d.as_str(), k.as_str()),
            ("Gemini-2.5-pro", "gemini-2.5-pro")
        );
    }

    #[test]
    fn dispatch_routes_deepseek_to_generic() {
        let (d, k) = normalize_model("deepseek-chat");
        assert_eq!((d.as_str(), k.as_str()), ("DeepSeek-chat", "deepseek-chat"));
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
                display_name: "GPT-5.3-codex".into(),
                model_key: "gpt-5.3-codex".into(),
            }
        );
    }

    #[test]
    fn generic_models_keep_identity_without_being_forced_into_claude_or_codex() {
        assert_eq!(
            known_model_from_raw("glm-4.5"),
            KnownModel {
                display_name: "GLM-4.5".into(),
                model_key: "glm-4.5".into(),
            }
        );
        assert_eq!(
            known_model_from_raw("gemini-2.5-pro"),
            KnownModel {
                display_name: "Gemini-2.5-pro".into(),
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
