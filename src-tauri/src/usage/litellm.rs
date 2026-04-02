use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Per-million-token rates fetched from LiteLLM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DynamicModelRates {
    pub input: f64,
    pub output: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
}

/// Cached pricing data persisted to disk.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PricingCache {
    fetched_at: u64,
    rates: HashMap<String, DynamicModelRates>,
}

/// Raw entry from the LiteLLM JSON (only the fields we need).
#[derive(Debug, serde::Deserialize)]
struct LiteLLMEntry {
    litellm_provider: Option<String>,
    mode: Option<String>,
    input_cost_per_token: Option<f64>,
    output_cost_per_token: Option<f64>,
    cache_creation_input_token_cost: Option<f64>,
    cache_read_input_token_cost: Option<f64>,
    #[serde(rename = "cache_creation_input_token_cost_above_1hr")]
    cache_creation_1h_cost: Option<f64>,
}

const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

const CACHE_FILENAME: &str = "pricing-cache.json";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60; // 24 hours
const PER_MTOK: f64 = 1_000_000.0;

fn cache_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CACHE_FILENAME)
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Check if the local cache needs refreshing (missing or older than 24h).
pub fn should_refresh(app_data_dir: &Path) -> bool {
    let path = cache_path(app_data_dir);
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<PricingCache>(&content) {
            Ok(cache) => now_epoch().saturating_sub(cache.fetched_at) > CACHE_TTL_SECS,
            Err(_) => true,
        },
        Err(_) => true,
    }
}

/// Load cached dynamic pricing from disk. Returns None if cache is missing or corrupt.
pub fn load_cached(app_data_dir: &Path) -> Option<HashMap<String, DynamicModelRates>> {
    let content = std::fs::read_to_string(cache_path(app_data_dir)).ok()?;
    let cache: PricingCache = serde_json::from_str(&content).ok()?;
    Some(cache.rates)
}

/// Fetch pricing from LiteLLM GitHub, parse, normalize, and cache to disk.
///
/// Returns the parsed rates HashMap, or an error string.
pub async fn fetch_and_cache(
    app_data_dir: &Path,
) -> Result<HashMap<String, DynamicModelRates>, String> {
    let body = reqwest::get(LITELLM_URL)
        .await
        .map_err(|e| format!("HTTP fetch failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    let raw: HashMap<String, serde_json::Value> =
        serde_json::from_str(&body).map_err(|e| format!("JSON parse failed: {e}"))?;

    let rates = parse_litellm_json(&raw);

    // Persist to disk.
    let cache = PricingCache {
        fetched_at: now_epoch(),
        rates: rates.clone(),
    };
    let json = serde_json::to_string(&cache).map_err(|e| format!("serialize: {e}"))?;

    let path = cache_path(app_data_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, json).map_err(|e| format!("write cache: {e}"))?;

    Ok(rates)
}

/// Parse the raw LiteLLM JSON into a normalized rates HashMap.
///
/// Keys are normalized model keys (matching `normalize_model` output from models.rs).
/// Values are per-million-token rates.
fn parse_litellm_json(
    raw: &HashMap<String, serde_json::Value>,
) -> HashMap<String, DynamicModelRates> {
    use crate::models::normalized_model_key;

    let mut rates: HashMap<String, DynamicModelRates> = HashMap::new();

    for (model_name, value) in raw {
        // Skip the template entry.
        if model_name == "sample_spec" {
            continue;
        }

        let entry: LiteLLMEntry = match serde_json::from_value(value.clone()) {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Only include direct API providers (no bedrock/azure/vertex prefixes).
        let provider = match &entry.litellm_provider {
            Some(p) => p.as_str(),
            None => continue,
        };
        if provider != "anthropic" && provider != "openai" {
            continue;
        }

        // Only chat/responses models (skip embeddings, image_generation, etc.).
        let mode = entry.mode.as_deref().unwrap_or("");
        if mode != "chat" && mode != "responses" {
            continue;
        }

        // Must have at least input + output pricing.
        let input_per_token = match entry.input_cost_per_token {
            Some(v) if v > 0.0 => v,
            _ => continue,
        };
        let output_per_token = match entry.output_cost_per_token {
            Some(v) if v > 0.0 => v,
            _ => continue,
        };

        // Convert per-token to per-million-token.
        let input = input_per_token * PER_MTOK;
        let output = output_per_token * PER_MTOK;

        let cache_write_5m = entry
            .cache_creation_input_token_cost
            .map(|v| v * PER_MTOK)
            .unwrap_or_else(|| {
                // Default: Anthropic 1.25x, OpenAI same as input.
                if provider == "anthropic" {
                    input * 1.25
                } else {
                    input
                }
            });

        let cache_write_1h = entry
            .cache_creation_1h_cost
            .map(|v| v * PER_MTOK)
            .unwrap_or(cache_write_5m); // No 1h tier → same as 5m.

        let cache_read = entry
            .cache_read_input_token_cost
            .map(|v| v * PER_MTOK)
            .unwrap_or_else(|| {
                // Default: Anthropic 0.1x, OpenAI 0.25x.
                if provider == "anthropic" {
                    input * 0.1
                } else {
                    input * 0.25
                }
            });

        let model_rates = DynamicModelRates {
            input,
            output,
            cache_write_5m,
            cache_write_1h,
            cache_read,
        };

        // Normalize to the same key format used by pricing.rs.
        let key = normalized_model_key(model_name);
        if key == "unknown" {
            // Also store under the raw lowercase name for OpenAI models
            // whose full name IS the key (e.g., "gpt-5.4", "o3-mini-2025-01-31").
            let raw_key = model_name.trim().to_ascii_lowercase();
            rates.entry(raw_key).or_insert_with(|| model_rates.clone());
        }
        // Always store under the normalized key (may overwrite, last wins is fine).
        rates.insert(key, model_rates);
    }

    rates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(input: f64, output: f64) -> serde_json::Value {
        serde_json::json!({
            "litellm_provider": "anthropic",
            "mode": "chat",
            "input_cost_per_token": input,
            "output_cost_per_token": output,
        })
    }

    fn make_openai_entry(input: f64, output: f64) -> serde_json::Value {
        serde_json::json!({
            "litellm_provider": "openai",
            "mode": "chat",
            "input_cost_per_token": input,
            "output_cost_per_token": output,
            "cache_creation_input_token_cost": input,
            "cache_read_input_token_cost": input * 0.25,
        })
    }

    #[test]
    fn parses_claude_model_pricing() {
        let mut raw = HashMap::new();
        raw.insert(
            "claude-sonnet-4-6".to_string(),
            make_entry(0.000003, 0.000015),
        );

        let rates = parse_litellm_json(&raw);
        let r = rates.get("sonnet-4-6").expect("should have sonnet-4-6");
        assert!((r.input - 3.0).abs() < 0.001);
        assert!((r.output - 15.0).abs() < 0.001);
        // Default cache: 1.25x input for 5m, same for 1h (no 1h field).
        assert!((r.cache_write_5m - 3.75).abs() < 0.001);
        assert!((r.cache_read - 0.3).abs() < 0.001);
    }

    #[test]
    fn parses_openai_model_pricing() {
        let mut raw = HashMap::new();
        raw.insert(
            "gpt-5.4".to_string(),
            make_openai_entry(0.0000025, 0.000015),
        );

        let rates = parse_litellm_json(&raw);
        let r = rates.get("gpt-5.4").expect("should have gpt-5.4");
        assert!((r.input - 2.5).abs() < 0.001);
        assert!((r.output - 15.0).abs() < 0.001);
    }

    #[test]
    fn skips_non_chat_models() {
        let mut raw = HashMap::new();
        raw.insert(
            "text-embedding-3-small".to_string(),
            serde_json::json!({
                "litellm_provider": "openai",
                "mode": "embedding",
                "input_cost_per_token": 0.00000002,
                "output_cost_per_token": 0.0,
            }),
        );

        let rates = parse_litellm_json(&raw);
        assert!(rates.is_empty());
    }

    #[test]
    fn skips_bedrock_providers() {
        let mut raw = HashMap::new();
        raw.insert(
            "anthropic.claude-opus-4-6-v1".to_string(),
            serde_json::json!({
                "litellm_provider": "bedrock_converse",
                "mode": "chat",
                "input_cost_per_token": 0.000005,
                "output_cost_per_token": 0.000025,
            }),
        );

        let rates = parse_litellm_json(&raw);
        assert!(rates.is_empty());
    }

    #[test]
    fn skips_sample_spec() {
        let mut raw = HashMap::new();
        raw.insert("sample_spec".to_string(), serde_json::json!({}));

        let rates = parse_litellm_json(&raw);
        assert!(rates.is_empty());
    }

    #[test]
    fn cache_ttl_check() {
        let dir = tempfile::tempdir().unwrap();

        // No cache file → should refresh.
        assert!(should_refresh(dir.path()));

        // Write a fresh cache.
        let cache = PricingCache {
            fetched_at: now_epoch(),
            rates: HashMap::new(),
        };
        let json = serde_json::to_string(&cache).unwrap();
        std::fs::write(cache_path(dir.path()), json).unwrap();
        assert!(!should_refresh(dir.path()));

        // Write a stale cache (>24h ago).
        let old_cache = PricingCache {
            fetched_at: now_epoch() - CACHE_TTL_SECS - 1,
            rates: HashMap::new(),
        };
        let json = serde_json::to_string(&old_cache).unwrap();
        std::fs::write(cache_path(dir.path()), json).unwrap();
        assert!(should_refresh(dir.path()));
    }
}
