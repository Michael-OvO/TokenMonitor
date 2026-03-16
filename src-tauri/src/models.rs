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

// ── Helpers ──

pub fn normalize_claude_model(raw: &str) -> (&str, &str) {
    // Returns (display_name, color_key)
    if raw.contains("opus-4-6") {
        ("Opus 4.6", "opus")
    } else if raw.contains("opus-4-5") {
        ("Opus 4.5", "opus")
    } else if raw.contains("sonnet-4-6") {
        ("Sonnet 4.6", "sonnet")
    } else if raw.contains("sonnet") {
        ("Sonnet", "sonnet")
    } else if raw.contains("haiku") {
        ("Haiku 4.5", "haiku")
    } else {
        ("Unknown", "unknown")
    }
}

pub fn normalize_codex_model(raw: &str) -> (&str, &str) {
    if raw.contains("5.4") {
        ("GPT-5.4", "gpt54")
    } else if raw.contains("5.3") {
        ("GPT-5.3 Codex", "gpt53")
    } else if raw.contains("5.2") {
        ("GPT-5.2", "gpt52")
    } else if raw.starts_with("o4-mini") {
        ("o4-mini", "o4mini")
    } else if raw.starts_with("o3-mini") {
        ("o3-mini", "o3mini")
    } else if raw.starts_with("o3") {
        ("o3", "o3")
    } else if raw.starts_with("o1-mini") {
        ("o1-mini", "o1mini")
    } else if raw.starts_with("o1") {
        ("o1", "o1")
    } else {
        (raw, "codex")
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
            ("Opus 4.6", "opus")
        );
    }

    #[test]
    fn claude_opus_4_5() {
        assert_eq!(
            normalize_claude_model("claude-opus-4-5-20250501"),
            ("Opus 4.5", "opus")
        );
    }

    #[test]
    fn claude_sonnet_4_6() {
        assert_eq!(
            normalize_claude_model("claude-sonnet-4-6-20260301"),
            ("Sonnet 4.6", "sonnet")
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
            ("Haiku 4.5", "haiku")
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
        assert_eq!(normalize_codex_model("gpt-5.4-turbo"), ("GPT-5.4", "gpt54"));
    }

    #[test]
    fn codex_gpt_5_3() {
        assert_eq!(
            normalize_codex_model("gpt-5.3-codex"),
            ("GPT-5.3 Codex", "gpt53")
        );
    }

    #[test]
    fn codex_gpt_5_2() {
        assert_eq!(normalize_codex_model("gpt-5.2"), ("GPT-5.2", "gpt52"));
    }

    #[test]
    fn codex_o4_mini() {
        assert_eq!(
            normalize_codex_model("o4-mini-2025-04-16"),
            ("o4-mini", "o4mini")
        );
    }

    #[test]
    fn codex_o3_mini() {
        assert_eq!(
            normalize_codex_model("o3-mini-2025-01-31"),
            ("o3-mini", "o3mini")
        );
    }

    #[test]
    fn codex_o3() {
        assert_eq!(normalize_codex_model("o3-2025-04-16"), ("o3", "o3"));
    }

    #[test]
    fn codex_o1_mini() {
        assert_eq!(
            normalize_codex_model("o1-mini-2024-09-12"),
            ("o1-mini", "o1mini")
        );
    }

    #[test]
    fn codex_o1() {
        assert_eq!(normalize_codex_model("o1-2024-12-17"), ("o1", "o1"));
    }

    #[test]
    fn codex_fallback() {
        assert_eq!(
            normalize_codex_model("some-future-model"),
            ("some-future-model", "codex")
        );
    }
}
