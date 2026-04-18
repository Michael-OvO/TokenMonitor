#[cfg_attr(not(test), allow(dead_code))]
pub const PRICING_VERSION: &str = "2026-03-15";

use crate::models::{detect_model_family, ModelFamily};
use crate::usage::litellm::DynamicModelRates;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

struct ModelRates {
    input: f64,
    output: f64,
    cache_write_5m: f64,
    cache_write_1h: f64,
    cache_read: f64,
}

// ── Dynamic pricing from LiteLLM ────────────────────────────────────────────

static DYNAMIC_PRICING: OnceLock<RwLock<HashMap<String, DynamicModelRates>>> = OnceLock::new();

/// Replace the dynamic pricing table. Called from startup and async refresh.
pub fn set_dynamic_pricing(rates: HashMap<String, DynamicModelRates>) {
    let lock = DYNAMIC_PRICING.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(mut guard) = lock.write() {
        *guard = rates;
    }
}

/// Look up dynamic pricing for a normalized model key.
fn lookup_dynamic(model_key: &str) -> Option<ModelRates> {
    let lock = DYNAMIC_PRICING.get()?;
    let guard = lock.read().ok()?;
    let r = guard.get(model_key)?;
    Some(ModelRates {
        input: r.input,
        output: r.output,
        cache_write_5m: r.cache_write_5m,
        cache_write_1h: r.cache_write_1h,
        cache_read: r.cache_read,
    })
}

/// Web search cost: $10 per 1,000 searches ($0.01 per search).
const WEB_SEARCH_COST_PER_REQUEST: f64 = 0.01;

#[cfg_attr(not(test), allow(dead_code))]
pub fn calculate_cost(
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_5m_tokens: u64,
    cache_creation_1h_tokens: u64,
    cache_read_tokens: u64,
    web_search_requests: u64,
) -> f64 {
    let rates = get_rates(model);
    apply_rates(
        &rates,
        input_tokens,
        output_tokens,
        cache_creation_5m_tokens,
        cache_creation_1h_tokens,
        cache_read_tokens,
    ) + web_search_requests as f64 * WEB_SEARCH_COST_PER_REQUEST
}

/// Like `calculate_cost`, but accepts a model key that is already lowercase.
/// Avoids a redundant `to_ascii_lowercase` allocation when the caller has
/// already normalized the model name (e.g. from `normalize_model`).
pub fn calculate_cost_for_key(
    model_key: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_5m_tokens: u64,
    cache_creation_1h_tokens: u64,
    cache_read_tokens: u64,
    web_search_requests: u64,
) -> f64 {
    let rates = get_rates_for_key(model_key);
    apply_rates(
        &rates,
        input_tokens,
        output_tokens,
        cache_creation_5m_tokens,
        cache_creation_1h_tokens,
        cache_read_tokens,
    ) + web_search_requests as f64 * WEB_SEARCH_COST_PER_REQUEST
}

fn apply_rates(
    rates: &ModelRates,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_5m_tokens: u64,
    cache_creation_1h_tokens: u64,
    cache_read_tokens: u64,
) -> f64 {
    let mtok = 1_000_000.0;
    (input_tokens as f64 / mtok) * rates.input
        + (output_tokens as f64 / mtok) * rates.output
        + (cache_creation_5m_tokens as f64 / mtok) * rates.cache_write_5m
        + (cache_creation_1h_tokens as f64 / mtok) * rates.cache_write_1h
        + (cache_read_tokens as f64 / mtok) * rates.cache_read
}

/// Build a ModelRates from base input price.  Cache multipliers follow
/// Claude Code's `/cost` convention: both 5m and 1h tiers use 1.25x,
/// cache read = 0.1x.  (The Anthropic API charges 2x for 1h, but Claude
/// Code bills everything at the 5m rate, so we match that for consistency.)
const fn claude_rates(input: f64, output: f64) -> ModelRates {
    ModelRates {
        input,
        output,
        cache_write_5m: input * 1.25,
        cache_write_1h: input * 1.25,
        cache_read: input * 0.1,
    }
}

/// OpenAI/o-series don't have a 1h tier — set 1h = 5m (same rate).
const fn openai_rates(input: f64, output: f64, cache_write: f64, cache_read: f64) -> ModelRates {
    ModelRates {
        input,
        output,
        cache_write_5m: cache_write,
        cache_write_1h: cache_write,
        cache_read,
    }
}

const fn zero_rates() -> ModelRates {
    ModelRates {
        input: 0.0,
        output: 0.0,
        cache_write_5m: 0.0,
        cache_write_1h: 0.0,
        cache_read: 0.0,
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn get_rates(model: &str) -> ModelRates {
    let normalized = model.trim().to_ascii_lowercase();
    get_rates_for_key(&normalized)
}

/// Look up pricing for an already-lowercase model key.
///
/// Checks dynamic LiteLLM pricing first, then falls back to hardcoded rates.
fn get_rates_for_key(model: &str) -> ModelRates {
    // Dynamic pricing from LiteLLM (refreshed on startup, cached 24h).
    if let Some(rates) = lookup_dynamic(model) {
        return rates;
    }

    // ── Hardcoded fallback (always available) ───────────────────────────────
    //
    // Claude models: handled entirely by get_fallback_rates() using family-level
    // detection (opus/sonnet/haiku). LiteLLM dynamic pricing above provides
    // per-version accuracy; the family fallback uses latest known rates.

    // ── OpenAI / Codex models ────────────────────────────────────────────────

    if model.contains("gpt-5.4") {
        return openai_rates(2.50, 15.00, 2.50, 0.25);
    }
    if model.contains("gpt-5.3-codex") {
        return openai_rates(1.75, 14.00, 1.75, 0.175);
    }
    if model.contains("gpt-5.2-codex") {
        return openai_rates(1.75, 14.00, 1.75, 0.175);
    }
    if model.contains("gpt-5.2") {
        return openai_rates(1.75, 14.00, 1.75, 0.175);
    }
    if model.contains("gpt-5.1-codex-max") {
        return openai_rates(1.25, 10.00, 1.25, 0.125);
    }
    if model.contains("gpt-5.1-codex-mini") {
        return openai_rates(0.25, 2.00, 0.25, 0.025);
    }
    if model.contains("gpt-5.1-codex") {
        return openai_rates(1.25, 10.00, 1.25, 0.125);
    }
    if model.contains("codex-mini-latest") {
        return openai_rates(1.50, 6.00, 1.50, 0.375);
    }
    if model.contains("gpt-5-codex") {
        return openai_rates(1.25, 10.00, 1.25, 0.125);
    }
    if model.contains("gpt-5-mini") {
        return openai_rates(0.25, 2.00, 0.25, 0.025);
    }
    if model.contains("gpt-5-nano") {
        return openai_rates(0.05, 0.40, 0.05, 0.005);
    }
    if model.contains("gpt-5.1") {
        return openai_rates(1.25, 10.00, 1.25, 0.125);
    }
    if model.contains("gpt-5") {
        return openai_rates(1.25, 10.00, 1.25, 0.125);
    }

    // ── o-series (starts_with, most-specific first) ──────────────────────────

    if model.starts_with("o4-mini") {
        return openai_rates(1.10, 4.40, 1.10, 0.275);
    }
    if model.starts_with("o3-mini") {
        return openai_rates(1.10, 4.40, 1.10, 0.55);
    }
    if model.starts_with("o3") {
        return openai_rates(2.00, 8.00, 2.00, 0.50);
    }
    if model.starts_with("o1-mini") {
        return openai_rates(1.10, 4.40, 1.10, 0.55);
    }
    if model.starts_with("o1") {
        return openai_rates(15.00, 60.00, 15.00, 7.50);
    }

    // ── Fuzzy fallback ───────────────────────────────────────────────────────
    get_fallback_rates(model)
}

fn get_fallback_rates(model: &str) -> ModelRates {
    match detect_model_family(model) {
        ModelFamily::Anthropic => {
            if model.contains("opus") {
                if model.contains("fast") {
                    claude_rates(30.00, 150.00)
                } else {
                    claude_rates(5.00, 25.00)
                }
            } else if model.contains("sonnet") {
                claude_rates(3.00, 15.00)
            } else if model.contains("haiku") {
                claude_rates(1.00, 5.00)
            } else {
                claude_rates(3.00, 15.00)
            }
        }
        ModelFamily::OpenAI => {
            if model.contains("codex-mini") {
                return openai_rates(0.25, 2.00, 0.25, 0.025);
            }
            if model.contains("codex") || model.contains("gpt-5") {
                return openai_rates(1.25, 10.00, 1.25, 0.125);
            }

            let bytes = model.as_bytes();
            if bytes.first() == Some(&b'o') && bytes.get(1).is_some_and(|b| b.is_ascii_digit()) {
                return openai_rates(1.10, 4.40, 1.10, 0.275);
            }

            openai_rates(1.25, 10.00, 1.25, 0.125)
        }
        ModelFamily::Google
        | ModelFamily::Moonshot
        | ModelFamily::Qwen
        | ModelFamily::Glm
        | ModelFamily::DeepSeek
        | ModelFamily::Unknown => zero_rates(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const M: u64 = 1_000_000;

    fn cost(model: &str, input: u64, output: u64) -> f64 {
        calculate_cost(model, input, output, 0, 0, 0, 0)
    }

    fn cost_cache_5m(model: &str, cache_write: u64, cache_read: u64) -> f64 {
        calculate_cost(model, 0, 0, cache_write, 0, cache_read, 0)
    }

    fn cost_cache_1h(model: &str, cache_write: u64, cache_read: u64) -> f64 {
        calculate_cost(model, 0, 0, 0, cache_write, cache_read, 0)
    }

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn opus_4_6_pricing() {
        assert!(approx_eq(cost("claude-opus-4-6-20260215", M, M), 30.00));
    }

    #[test]
    fn sonnet_4_6_pricing() {
        assert!(approx_eq(cost("claude-sonnet-4-6-20260101", M, M), 18.00));
    }

    #[test]
    fn haiku_4_5_pricing() {
        assert!(approx_eq(cost("claude-haiku-4-5-20260101", M, M), 6.00));
    }

    #[test]
    fn claude_5m_cache_tokens() {
        // Sonnet 4.6: 5m cache_write $3.75 + cache_read $0.30 = $4.05
        assert!(approx_eq(
            cost_cache_5m("claude-sonnet-4-6-20260101", M, M),
            4.05
        ));
    }

    #[test]
    fn claude_1h_cache_tokens() {
        // Sonnet 4.6: 1h uses same 1.25x rate as 5m → $3.75 + cache_read $0.30 = $4.05
        assert!(approx_eq(
            cost_cache_1h("claude-sonnet-4-6-20260101", M, M),
            4.05
        ));
    }

    #[test]
    fn opus_1h_cache_tokens() {
        // Opus 4.6: 1h uses same 1.25x rate as 5m → $6.25 + cache_read $0.50 = $6.75
        assert!(approx_eq(cost_cache_1h("claude-opus-4-6", M, M), 6.75));
    }

    #[test]
    fn opus_4_1_falls_back_to_family() {
        // Without LiteLLM, opus-4-1 uses the family fallback rate ($5/$25).
        assert!(approx_eq(cost("claude-opus-4-1-20250401", M, M), 30.00));
    }

    #[test]
    fn opus_4_bare_falls_back_to_family() {
        // "opus-4" without minor version → family fallback.
        assert!(approx_eq(cost("claude-opus-4-20250401", M, M), 30.00));
    }

    #[test]
    fn opus_4_6_fast_uses_6x_pricing() {
        // Fast mode: $30/$150 → (30+150)/M = $180/M
        assert!(approx_eq(cost("claude-opus-4-6-fast", M, M), 180.00));
    }

    #[test]
    fn sonnet_3_7_hits_sonnet_catchall() {
        assert!(approx_eq(cost("claude-3-7-sonnet-20250219", M, M), 18.00));
    }

    #[test]
    fn gpt_5_4_pricing() {
        assert!(approx_eq(cost("gpt-5.4", M, M), 17.50));
    }

    #[test]
    fn gpt_5_3_codex_pricing() {
        assert!(approx_eq(cost("gpt-5.3-codex", M, M), 15.75));
    }

    #[test]
    fn gpt_5_1_codex_mini_pricing() {
        assert!(approx_eq(cost("gpt-5.1-codex-mini", M, M), 2.25));
    }

    #[test]
    fn o4_mini_pricing() {
        assert!(approx_eq(cost("o4-mini-2025-04-16", M, M), 5.50));
    }

    #[test]
    fn o3_pricing() {
        assert!(approx_eq(cost("o3-2025-04-16", M, M), 10.00));
    }

    #[test]
    fn o3_mini_pricing() {
        assert!(approx_eq(cost("o3-mini-2025-01-31", M, M), 5.50));
    }

    #[test]
    fn o1_pricing() {
        assert!(approx_eq(cost("o1-2024-12-17", M, M), 75.00));
    }

    #[test]
    fn o1_mini_pricing() {
        assert!(approx_eq(cost("o1-mini-2024-09-12", M, M), 5.50));
    }

    #[test]
    fn openai_cached_input_tokens() {
        // gpt-5.4: cache_write $2.50 + cache_read $0.25 = $2.75 (no 1h tier)
        assert!(approx_eq(cost_cache_5m("gpt-5.4", M, M), 2.75));
        assert!(approx_eq(cost_cache_1h("gpt-5.4", M, M), 2.75));
    }

    #[test]
    fn codex_mini_latest_pricing() {
        assert!(approx_eq(cost("codex-mini-latest", M, M), 7.50));
    }

    #[test]
    fn gpt_5_base_pricing() {
        assert!(approx_eq(cost("gpt-5", M, M), 11.25));
    }

    #[test]
    fn gpt_5_1_codex_not_mini() {
        assert!(approx_eq(cost("gpt-5.1-codex", M, M), 11.25));
    }

    #[test]
    fn gpt_5_mini_pricing() {
        assert!(approx_eq(cost("gpt-5-mini", M, M), 2.25));
    }

    #[test]
    fn o3_cache_rates() {
        // o3: cache_write $2.00 + cache_read $0.50 = $2.50
        assert!(approx_eq(cost_cache_5m("o3", M, M), 2.50));
    }

    #[test]
    fn unknown_opus_falls_back_to_latest() {
        assert!(approx_eq(cost("claude-opus-5-0-20270101", M, M), 30.00));
    }

    #[test]
    fn unknown_sonnet_falls_back() {
        assert!(approx_eq(cost("claude-sonnet-5-0", M, M), 18.00));
    }

    #[test]
    fn unknown_haiku_falls_back() {
        assert!(approx_eq(cost("claude-haiku-5-0", M, M), 6.00));
    }

    #[test]
    fn unknown_codex_mini_falls_back() {
        assert!(approx_eq(cost("gpt-6-codex-mini", M, M), 2.25));
    }

    #[test]
    fn unknown_codex_falls_back() {
        assert!(approx_eq(cost("gpt-6-codex", M, M), 11.25));
    }

    #[test]
    fn unknown_o_series_falls_back() {
        assert!(approx_eq(cost("o5-mini-2026-01-01", M, M), 5.50));
    }

    #[test]
    fn unsupported_family_defaults_to_zero_until_priced() {
        assert!(approx_eq(cost("gemini-2.5-pro", M, M), 0.00));
        assert!(approx_eq(cost("totally-unknown-model", M, M), 0.00));
    }

    #[test]
    fn zero_tokens_zero_cost() {
        assert!(approx_eq(
            calculate_cost("claude-sonnet-4-6", 0, 0, 0, 0, 0, 0),
            0.00
        ));
    }

    #[test]
    fn pricing_version_is_set() {
        assert_eq!(PRICING_VERSION, "2026-03-15");
    }
}
