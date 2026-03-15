use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Claude ccusage JSON output ──

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeDailyResponse {
    pub daily: Vec<ClaudeDayEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeDayEntry {
    pub date: String,
    #[serde(rename = "totalTokens", default)]
    pub total_tokens: u64,
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    #[serde(rename = "modelBreakdowns", default)]
    pub model_breakdowns: Vec<ClaudeModelBreakdown>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeModelBreakdown {
    #[serde(rename = "modelName")]
    pub model_name: String,
    pub cost: f64,
    #[serde(rename = "inputTokens", default)]
    pub input_tokens: u64,
    #[serde(rename = "outputTokens", default)]
    pub output_tokens: u64,
    #[serde(rename = "cacheCreationTokens", default)]
    pub cache_creation_tokens: u64,
    #[serde(rename = "cacheReadTokens", default)]
    pub cache_read_tokens: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeMonthlyResponse {
    pub monthly: Vec<ClaudeMonthEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeMonthEntry {
    pub month: String,
    #[serde(rename = "totalTokens", default)]
    pub total_tokens: u64,
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    #[serde(rename = "modelBreakdowns", default)]
    pub model_breakdowns: Vec<ClaudeModelBreakdown>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeBlocksResponse {
    pub blocks: Vec<ClaudeBlockEntry>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct ClaudeBlockEntry {
    #[serde(rename = "startTime")]
    pub start_time: String,
    #[serde(rename = "endTime")]
    pub end_time: String,
    #[serde(rename = "isActive", default)]
    pub is_active: bool,
    #[serde(rename = "isGap", default)]
    pub is_gap: bool,
    #[serde(default)]
    pub entries: u32,
    #[serde(rename = "costUSD", default)]
    pub cost_usd: f64,
    #[serde(rename = "totalTokens", default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(rename = "burnRate")]
    pub burn_rate: Option<BurnRate>,
    pub projection: Option<Projection>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BurnRate {
    #[serde(rename = "costPerHour", default)]
    pub cost_per_hour: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Projection {
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
}

// ── Codex @ccusage/codex JSON output ──

#[derive(Debug, Deserialize, Clone)]
pub struct CodexDailyResponse {
    pub daily: Vec<CodexDayEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CodexDayEntry {
    pub date: String,
    #[serde(rename = "totalTokens", default)]
    pub total_tokens: u64,
    #[serde(rename = "costUSD", default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub models: HashMap<String, CodexModelUsage>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CodexModelUsage {
    #[serde(rename = "inputTokens", default)]
    pub input_tokens: u64,
    #[serde(rename = "outputTokens", default)]
    pub output_tokens: u64,
    #[serde(rename = "reasoningOutputTokens", default)]
    pub reasoning_output_tokens: u64,
    #[serde(rename = "totalTokens", default)]
    pub total_tokens: u64,
}

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
pub struct SetupStatus {
    pub ready: bool,
    pub installing: bool,
    pub error: Option<String>,
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
        assert_eq!(normalize_codex_model("o4-mini-2025-04-16"), ("o4-mini", "o4mini"));
    }

    #[test]
    fn codex_o3_mini() {
        assert_eq!(normalize_codex_model("o3-mini-2025-01-31"), ("o3-mini", "o3mini"));
    }

    #[test]
    fn codex_o3() {
        assert_eq!(normalize_codex_model("o3-2025-04-16"), ("o3", "o3"));
    }

    #[test]
    fn codex_o1_mini() {
        assert_eq!(normalize_codex_model("o1-mini-2024-09-12"), ("o1-mini", "o1mini"));
    }

    #[test]
    fn codex_o1() {
        assert_eq!(normalize_codex_model("o1-2024-12-17"), ("o1", "o1"));
    }

    #[test]
    fn codex_fallback() {
        assert_eq!(normalize_codex_model("some-future-model"), ("some-future-model", "codex"));
    }

    // ── Serde deserialization ──

    #[test]
    fn deserialize_claude_daily_response() {
        let json = r#"{
            "daily": [{
                "date": "2025-03-15",
                "totalTokens": 5000,
                "totalCost": 1.23,
                "modelBreakdowns": [{
                    "modelName": "claude-sonnet-4-6",
                    "cost": 1.23,
                    "inputTokens": 3000,
                    "outputTokens": 2000,
                    "cacheCreationTokens": 100,
                    "cacheReadTokens": 50
                }]
            }]
        }"#;
        let resp: ClaudeDailyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.daily.len(), 1);
        assert_eq!(resp.daily[0].total_tokens, 5000);
        assert_eq!(resp.daily[0].model_breakdowns[0].cache_creation_tokens, 100);
    }

    #[test]
    fn deserialize_claude_daily_defaults() {
        let json = r#"{"daily": [{"date": "2025-01-01"}]}"#;
        let resp: ClaudeDailyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.daily[0].total_tokens, 0);
        assert_eq!(resp.daily[0].total_cost, 0.0);
        assert!(resp.daily[0].model_breakdowns.is_empty());
    }

    #[test]
    fn deserialize_claude_blocks_response() {
        let json = r#"{
            "blocks": [{
                "startTime": "2025-03-15T10:00:00Z",
                "endTime": "2025-03-15T11:00:00Z",
                "isActive": true,
                "isGap": false,
                "entries": 5,
                "costUSD": 2.50,
                "totalTokens": 10000,
                "models": ["claude-sonnet-4-6"],
                "burnRate": {"costPerHour": 0.50},
                "projection": {"totalCost": 5.00}
            }]
        }"#;
        let resp: ClaudeBlocksResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.blocks.len(), 1);
        assert!(resp.blocks[0].is_active);
        assert_eq!(resp.blocks[0].burn_rate.as_ref().unwrap().cost_per_hour, 0.50);
    }

    #[test]
    fn deserialize_claude_monthly_response() {
        let json = r#"{
            "monthly": [{
                "month": "2025-03",
                "totalTokens": 100000,
                "totalCost": 15.50,
                "modelBreakdowns": []
            }]
        }"#;
        let resp: ClaudeMonthlyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.monthly[0].month, "2025-03");
        assert_eq!(resp.monthly[0].total_cost, 15.50);
    }

    #[test]
    fn deserialize_codex_daily_response() {
        let json = r#"{
            "daily": [{
                "date": "Mar 01, 2026",
                "totalTokens": 8000,
                "costUSD": 0.95,
                "models": {
                    "gpt-5.4-turbo": {
                        "inputTokens": 3000,
                        "outputTokens": 2000,
                        "reasoningOutputTokens": 500,
                        "totalTokens": 5500
                    }
                }
            }]
        }"#;
        let resp: CodexDailyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.daily[0].cost_usd, 0.95);
        let model = resp.daily[0].models.get("gpt-5.4-turbo").unwrap();
        assert_eq!(model.reasoning_output_tokens, 500);
    }

    #[test]
    fn deserialize_codex_daily_defaults() {
        let json = r#"{"daily": [{"date": "Mar 01, 2026"}]}"#;
        let resp: CodexDailyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.daily[0].total_tokens, 0);
        assert_eq!(resp.daily[0].cost_usd, 0.0);
        assert!(resp.daily[0].models.is_empty());
    }
}
