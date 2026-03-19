use serde::Serialize;

// ── Frontend payload (sent to Svelte via IPC) ──

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
    pub period_label: String,
    pub has_earlier_data: bool,
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

pub fn normalize_claude_model(raw: &str) -> (&str, &str) {
    // Returns (display_name, color_key)
    if raw.contains("opus-4-6") {
        ("Opus 4.6", "opus-4-6")
    } else if raw.contains("opus-4-5") {
        ("Opus 4.5", "opus-4-5")
    } else if raw.contains("sonnet-4-6") {
        ("Sonnet 4.6", "sonnet-4-6")
    } else if raw.contains("sonnet-4-5") {
        ("Sonnet 4.5", "sonnet-4-5")
    } else if raw.contains("sonnet") {
        ("Sonnet", "sonnet")
    } else if raw.contains("haiku-4-5") {
        ("Haiku 4.5", "haiku-4-5")
    } else if raw.contains("haiku") {
        ("Haiku", "haiku")
    } else {
        ("Unknown", "unknown")
    }
}

pub fn normalize_codex_model(raw: &str) -> (String, String) {
    let display_name = raw.trim();
    if display_name.is_empty() {
        return (String::from("Unknown"), String::from("unknown"));
    }

    let normalized_key = display_name.to_ascii_lowercase();
    let normalized_display_name = if normalized_key.starts_with("gpt") {
        format!("GPT{}", &display_name[3..])
    } else {
        display_name.to_string()
    };

    (normalized_display_name, normalized_key)
}

fn is_codex_model_name(raw: &str) -> bool {
    raw.starts_with("gpt")
        || raw.starts_with("o1")
        || raw.starts_with("o3")
        || raw.starts_with("o4")
        || raw.contains("codex")
}

pub fn known_model_from_raw(raw: &str) -> KnownModel {
    if is_codex_model_name(raw) {
        let (display_name, model_key) = normalize_codex_model(raw);
        KnownModel {
            display_name,
            model_key,
        }
    } else {
        let (display_name, model_key) = normalize_claude_model(raw);
        KnownModel {
            display_name: display_name.to_string(),
            model_key: model_key.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_claude_model ──

    #[test]
    fn claude_opus_4_6() {
        assert_eq!(
            normalize_claude_model("claude-opus-4-6-20260301"),
            ("Opus 4.6", "opus-4-6")
        );
    }

    #[test]
    fn claude_opus_4_5() {
        assert_eq!(
            normalize_claude_model("claude-opus-4-5-20250501"),
            ("Opus 4.5", "opus-4-5")
        );
    }

    #[test]
    fn claude_sonnet_4_6() {
        assert_eq!(
            normalize_claude_model("claude-sonnet-4-6-20260301"),
            ("Sonnet 4.6", "sonnet-4-6")
        );
    }

    #[test]
    fn claude_sonnet_4_5() {
        assert_eq!(
            normalize_claude_model("claude-sonnet-4-5-20250514"),
            ("Sonnet 4.5", "sonnet-4-5")
        );
    }

    #[test]
    fn claude_sonnet_generic() {
        assert_eq!(
            normalize_claude_model("claude-3-5-sonnet-20241022"),
            ("Sonnet", "sonnet")
        );
    }

    #[test]
    fn claude_haiku() {
        assert_eq!(
            normalize_claude_model("claude-haiku-4-5-20251001"),
            ("Haiku 4.5", "haiku-4-5")
        );
    }

    #[test]
    fn claude_unknown() {
        assert_eq!(
            normalize_claude_model("some-unknown-model"),
            ("Unknown", "unknown")
        );
    }

    // ── normalize_codex_model ──

    #[test]
    fn codex_gpt_5_4() {
        assert_eq!(
            normalize_codex_model("gpt-5.4-turbo"),
            (String::from("GPT-5.4-turbo"), String::from("gpt-5.4-turbo"))
        );
    }

    #[test]
    fn codex_gpt_5_3() {
        assert_eq!(
            normalize_codex_model("gpt-5.3-codex"),
            (String::from("GPT-5.3-codex"), String::from("gpt-5.3-codex"))
        );
    }

    #[test]
    fn codex_gpt_5_2() {
        assert_eq!(
            normalize_codex_model("gpt-5.2"),
            (String::from("GPT-5.2"), String::from("gpt-5.2"))
        );
    }

    #[test]
    fn codex_gpt_5_1_codex_max() {
        assert_eq!(
            normalize_codex_model("gpt-5.1-codex-max"),
            (
                String::from("GPT-5.1-codex-max"),
                String::from("gpt-5.1-codex-max")
            )
        );
    }

    #[test]
    fn codex_gpt_5_1_codex_mini() {
        assert_eq!(
            normalize_codex_model("gpt-5.1-codex-mini"),
            (
                String::from("GPT-5.1-codex-mini"),
                String::from("gpt-5.1-codex-mini")
            )
        );
    }

    #[test]
    fn codex_gpt_5_1_codex() {
        assert_eq!(
            normalize_codex_model("gpt-5.1-codex"),
            (String::from("GPT-5.1-codex"), String::from("gpt-5.1-codex"))
        );
    }

    #[test]
    fn codex_gpt_5_codex() {
        assert_eq!(
            normalize_codex_model("gpt-5-codex"),
            (String::from("GPT-5-codex"), String::from("gpt-5-codex"))
        );
    }

    #[test]
    fn codex_mini_latest() {
        assert_eq!(
            normalize_codex_model("codex-mini-latest"),
            (
                String::from("codex-mini-latest"),
                String::from("codex-mini-latest")
            )
        );
    }

    #[test]
    fn codex_o4_mini() {
        assert_eq!(
            normalize_codex_model("o4-mini-2025-04-16"),
            (
                String::from("o4-mini-2025-04-16"),
                String::from("o4-mini-2025-04-16")
            )
        );
    }

    #[test]
    fn codex_o3_mini() {
        assert_eq!(
            normalize_codex_model("o3-mini-2025-01-31"),
            (
                String::from("o3-mini-2025-01-31"),
                String::from("o3-mini-2025-01-31")
            )
        );
    }

    #[test]
    fn codex_o3() {
        assert_eq!(
            normalize_codex_model("o3-2025-04-16"),
            (String::from("o3-2025-04-16"), String::from("o3-2025-04-16"))
        );
    }

    #[test]
    fn codex_o1_mini() {
        assert_eq!(
            normalize_codex_model("o1-mini-2024-09-12"),
            (
                String::from("o1-mini-2024-09-12"),
                String::from("o1-mini-2024-09-12")
            )
        );
    }

    #[test]
    fn codex_o1() {
        assert_eq!(
            normalize_codex_model("o1-2024-12-17"),
            (String::from("o1-2024-12-17"), String::from("o1-2024-12-17"))
        );
    }

    #[test]
    fn codex_fallback() {
        assert_eq!(
            normalize_codex_model("some-future-model"),
            (
                String::from("some-future-model"),
                String::from("some-future-model")
            )
        );
    }

    #[test]
    fn known_model_from_raw_uses_dynamic_codex_identity() {
        assert_eq!(
            known_model_from_raw("gpt-5.3-codex"),
            KnownModel {
                display_name: String::from("GPT-5.3-codex"),
                model_key: String::from("gpt-5.3-codex"),
            }
        );
    }
}
