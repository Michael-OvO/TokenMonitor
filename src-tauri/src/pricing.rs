pub const PRICING_VERSION: &str = "2026-03-15";

struct ModelRates {
    input: f64,
    output: f64,
    cache_write: f64,
    cache_read: f64,
}

pub fn calculate_cost(
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
) -> f64 {
    let rates = get_rates(model);
    let mtok = 1_000_000.0;
    (input_tokens as f64 / mtok) * rates.input
        + (output_tokens as f64 / mtok) * rates.output
        + (cache_creation_tokens as f64 / mtok) * rates.cache_write
        + (cache_read_tokens as f64 / mtok) * rates.cache_read
}

fn get_rates(model: &str) -> ModelRates {
    // ── Claude models (most-specific first) ─────────────────────────────────

    if model.contains("opus-4-6") {
        return ModelRates { input: 5.00, output: 25.00, cache_write: 6.25, cache_read: 0.50 };
    }
    if model.contains("opus-4-5") {
        return ModelRates { input: 5.00, output: 25.00, cache_write: 6.25, cache_read: 0.50 };
    }
    if model.contains("opus-4-1") {
        return ModelRates { input: 15.00, output: 75.00, cache_write: 18.75, cache_read: 1.50 };
    }
    if model.contains("opus-4") {
        return ModelRates { input: 15.00, output: 75.00, cache_write: 18.75, cache_read: 1.50 };
    }
    if model.contains("sonnet-4-6") {
        return ModelRates { input: 3.00, output: 15.00, cache_write: 3.75, cache_read: 0.30 };
    }
    if model.contains("sonnet-4-5") {
        return ModelRates { input: 3.00, output: 15.00, cache_write: 3.75, cache_read: 0.30 };
    }
    if model.contains("haiku-4-5") {
        return ModelRates { input: 1.00, output: 5.00, cache_write: 1.25, cache_read: 0.10 };
    }
    if model.contains("haiku-3-5") {
        return ModelRates { input: 0.80, output: 4.00, cache_write: 1.00, cache_read: 0.08 };
    }

    // Claude family catchalls (before OpenAI checks).
    // Use versioned prefixes so future major-version models fall through to
    // get_fallback_rates() instead of picking up stale Claude 3/4 rates.
    if model.contains("sonnet-4") || model.contains("3-5-sonnet") || model.contains("3-7-sonnet")
        || model.contains("3-sonnet") || model.contains("sonnet-3")
    {
        return ModelRates { input: 3.00, output: 15.00, cache_write: 3.75, cache_read: 0.30 };
    }
    // Broad sonnet catchall — catches "claude-3-7-sonnet-..." and similar patterns
    // where the version prefix appears before "sonnet".
    if model.contains("sonnet") {
        return ModelRates { input: 3.00, output: 15.00, cache_write: 3.75, cache_read: 0.30 };
    }
    // Haiku-3 catchall (original Claude 3 Haiku pricing).
    if model.contains("haiku-3") || model.contains("3-haiku") {
        return ModelRates { input: 0.25, output: 1.25, cache_write: 0.30, cache_read: 0.03 };
    }

    // ── OpenAI / Codex models ────────────────────────────────────────────────

    if model.contains("gpt-5.4") {
        return ModelRates { input: 2.50, output: 15.00, cache_write: 2.50, cache_read: 0.25 };
    }
    if model.contains("gpt-5.3-codex") {
        return ModelRates { input: 1.75, output: 14.00, cache_write: 1.75, cache_read: 0.175 };
    }
    if model.contains("gpt-5.2-codex") {
        return ModelRates { input: 1.75, output: 14.00, cache_write: 1.75, cache_read: 0.175 };
    }
    if model.contains("gpt-5.2") {
        return ModelRates { input: 1.75, output: 14.00, cache_write: 1.75, cache_read: 0.175 };
    }
    if model.contains("gpt-5.1-codex-max") {
        return ModelRates { input: 1.25, output: 10.00, cache_write: 1.25, cache_read: 0.125 };
    }
    if model.contains("gpt-5.1-codex-mini") {
        return ModelRates { input: 0.25, output: 2.00, cache_write: 0.25, cache_read: 0.025 };
    }
    if model.contains("gpt-5.1-codex") {
        return ModelRates { input: 1.25, output: 10.00, cache_write: 1.25, cache_read: 0.125 };
    }
    if model.contains("codex-mini-latest") {
        return ModelRates { input: 1.50, output: 6.00, cache_write: 1.50, cache_read: 0.375 };
    }
    if model.contains("gpt-5-codex") {
        return ModelRates { input: 1.25, output: 10.00, cache_write: 1.25, cache_read: 0.125 };
    }
    if model.contains("gpt-5-mini") {
        return ModelRates { input: 0.25, output: 2.00, cache_write: 0.25, cache_read: 0.025 };
    }
    if model.contains("gpt-5-nano") {
        return ModelRates { input: 0.05, output: 0.40, cache_write: 0.05, cache_read: 0.005 };
    }
    if model.contains("gpt-5.1") {
        return ModelRates { input: 1.25, output: 10.00, cache_write: 1.25, cache_read: 0.125 };
    }
    if model.contains("gpt-5") {
        return ModelRates { input: 1.25, output: 10.00, cache_write: 1.25, cache_read: 0.125 };
    }

    // ── o-series (starts_with, most-specific first) ──────────────────────────

    if model.starts_with("o4-mini") {
        return ModelRates { input: 1.10, output: 4.40, cache_write: 1.10, cache_read: 0.275 };
    }
    if model.starts_with("o3-mini") {
        return ModelRates { input: 1.10, output: 4.40, cache_write: 1.10, cache_read: 0.55 };
    }
    if model.starts_with("o3") {
        return ModelRates { input: 2.00, output: 8.00, cache_write: 2.00, cache_read: 0.50 };
    }
    if model.starts_with("o1-mini") {
        return ModelRates { input: 1.10, output: 4.40, cache_write: 1.10, cache_read: 0.55 };
    }
    if model.starts_with("o1") {
        return ModelRates { input: 15.00, output: 60.00, cache_write: 15.00, cache_read: 7.50 };
    }

    // ── Fuzzy fallback ───────────────────────────────────────────────────────
    get_fallback_rates(model)
}

fn get_fallback_rates(model: &str) -> ModelRates {
    if model.contains("opus") {
        return ModelRates { input: 5.00, output: 25.00, cache_write: 6.25, cache_read: 0.50 };
    }
    if model.contains("sonnet") {
        return ModelRates { input: 3.00, output: 15.00, cache_write: 3.75, cache_read: 0.30 };
    }
    if model.contains("haiku") {
        return ModelRates { input: 1.00, output: 5.00, cache_write: 1.25, cache_read: 0.10 };
    }
    if model.contains("codex-mini") {
        return ModelRates { input: 0.25, output: 2.00, cache_write: 0.25, cache_read: 0.025 };
    }
    if model.contains("codex") || model.contains("gpt-5") {
        return ModelRates { input: 1.25, output: 10.00, cache_write: 1.25, cache_read: 0.125 };
    }
    // Starts with o + ASCII digit
    let bytes = model.as_bytes();
    if bytes.first() == Some(&b'o') && bytes.get(1).map_or(false, |b| b.is_ascii_digit()) {
        return ModelRates { input: 1.10, output: 4.40, cache_write: 1.10, cache_read: 0.275 };
    }
    // Completely unknown — default to Sonnet rates
    ModelRates { input: 3.00, output: 15.00, cache_write: 3.75, cache_read: 0.30 }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const M: u64 = 1_000_000;

    fn cost(model: &str, input: u64, output: u64) -> f64 {
        calculate_cost(model, input, output, 0, 0)
    }

    fn cost_cache(model: &str, cache_write: u64, cache_read: u64) -> f64 {
        calculate_cost(model, 0, 0, cache_write, cache_read)
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
    fn claude_cache_tokens() {
        // Sonnet 4.6: cache_write $3.75 + cache_read $0.30 = $4.05
        assert!(approx_eq(cost_cache("claude-sonnet-4-6-20260101", M, M), 4.05));
    }

    #[test]
    fn opus_4_1_higher_pricing() {
        assert!(approx_eq(cost("claude-opus-4-1-20250401", M, M), 90.00));
    }

    #[test]
    fn opus_4_no_minor_version() {
        // "claude-opus-4-20250401" contains "opus-4" but not "opus-4-1" / "opus-4-5" / "opus-4-6"
        assert!(approx_eq(cost("claude-opus-4-20250401", M, M), 90.00));
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
        // gpt-5.4: cache_write $2.50 + cache_read $0.25 = $2.75
        assert!(approx_eq(cost_cache("gpt-5.4", M, M), 2.75));
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
        // "gpt-5.1-codex" should NOT hit gpt-5.1-codex-mini
        assert!(approx_eq(cost("gpt-5.1-codex", M, M), 11.25));
    }

    #[test]
    fn gpt_5_mini_pricing() {
        assert!(approx_eq(cost("gpt-5-mini", M, M), 2.25));
    }

    #[test]
    fn o3_cache_rates() {
        // o3: cache_write $2.00 + cache_read $0.50 = $2.50
        assert!(approx_eq(cost_cache("o3", M, M), 2.50));
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
    fn completely_unknown_falls_back_to_sonnet() {
        assert!(approx_eq(cost("totally-unknown-model", M, M), 18.00));
    }

    #[test]
    fn zero_tokens_zero_cost() {
        assert!(approx_eq(calculate_cost("claude-sonnet-4-6", 0, 0, 0, 0), 0.00));
    }

    #[test]
    fn pricing_version_is_set() {
        assert_eq!(PRICING_VERSION, "2026-03-15");
    }
}
