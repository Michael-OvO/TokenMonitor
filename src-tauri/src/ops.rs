//! Vendor-facing operational values — edit `ops.json` when providers change
//! published plan numbers, API hosts, or the LiteLLM pricing-cache TTL.
//!
//! Loaded at compile time via `include_str!` (same pattern as
//! `usage/pricing_fallback.json`). UI sizes, HTTP status codes, and protocol
//! constants stay in code.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

const OPS_JSON: &str = include_str!("ops.json");

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PlanBudget {
    pub five_hour_tokens: u64,
    pub weekly_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct Urls {
    anthropic_usage: String,
    anthropic_account: String,
    cursor_official_api: String,
    cursor_dashboard: String,
    cursor_ide: String,
    openrouter_models: String,
    litellm_prices: String,
    frankfurter_latest: String,
}

#[derive(Debug, Deserialize)]
struct Ttls {
    litellm_cache: u64,
}

// `plan_tier_costs_usd` is deliberately absent here: the frontend imports it
// straight from `ops.json` (`src/lib/providerMetadata.ts`) and no Rust code
// reads it. Serde ignores the unknown key; the test below guards its shape.
#[derive(Debug, Deserialize)]
struct OpsFile {
    claude_plan_budgets: HashMap<String, PlanBudget>,
    urls: Urls,
    ttls_secs: Ttls,
}

struct Ops {
    claude_plan_budgets: HashMap<String, PlanBudget>,
    urls: Urls,
    litellm_cache_ttl_secs: u64,
}

static OPS: OnceLock<Ops> = OnceLock::new();

fn ops() -> &'static Ops {
    OPS.get_or_init(|| {
        let raw: OpsFile = serde_json::from_str(OPS_JSON).expect("ops.json must be valid JSON");
        Ops {
            claude_plan_budgets: raw.claude_plan_budgets,
            urls: raw.urls,
            litellm_cache_ttl_secs: raw.ttls_secs.litellm_cache,
        }
    })
}

pub fn claude_plan_budget(id: &str) -> PlanBudget {
    ops()
        .claude_plan_budgets
        .get(id)
        .copied()
        .unwrap_or_else(|| panic!("ops.json missing claude_plan_budgets.{id}"))
}

pub fn anthropic_usage_url() -> &'static str {
    ops().urls.anthropic_usage.as_str()
}

pub fn anthropic_account_url() -> &'static str {
    ops().urls.anthropic_account.as_str()
}

pub fn cursor_official_api_base() -> &'static str {
    ops().urls.cursor_official_api.as_str()
}

pub fn cursor_dashboard_api_base() -> &'static str {
    ops().urls.cursor_dashboard.as_str()
}

pub fn cursor_ide_api_base() -> &'static str {
    ops().urls.cursor_ide.as_str()
}

pub fn openrouter_models_url() -> &'static str {
    ops().urls.openrouter_models.as_str()
}

pub fn litellm_prices_url() -> &'static str {
    ops().urls.litellm_prices.as_str()
}

pub fn frankfurter_latest_url() -> &'static str {
    ops().urls.frankfurter_latest.as_str()
}

pub fn litellm_cache_ttl_secs() -> u64 {
    ops().litellm_cache_ttl_secs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_claude_pro_budget() {
        let pro = claude_plan_budget("pro");
        assert_eq!(pro.five_hour_tokens, 200_000);
        assert_eq!(pro.weekly_tokens, 7_000_000);
    }

    #[test]
    fn loads_urls_and_ttls() {
        assert!(anthropic_usage_url().starts_with("https://api.anthropic.com/"));
        assert_eq!(litellm_cache_ttl_secs(), 7 * 24 * 60 * 60);
    }

    /// Only the frontend reads `plan_tier_costs_usd`, so guard the key here —
    /// a rename or a dropped tier would otherwise surface as a UI-only break.
    #[test]
    fn ops_json_keeps_plan_tier_costs_for_the_frontend() {
        let raw: serde_json::Value = serde_json::from_str(OPS_JSON).unwrap();
        assert_eq!(raw["plan_tier_costs_usd"]["claude"]["Pro"], 20.0);
        assert_eq!(raw["plan_tier_costs_usd"]["codex"]["Plus"], 20.0);
    }
}
