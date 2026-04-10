use std::collections::HashMap;

use super::litellm::DynamicModelRates;

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/models";
const PER_MTOK: f64 = 1_000_000.0;
/// Sanity upper bound: reject per-million-token rates above $500.
const MAX_RATE_PER_MTOK: f64 = 500.0;

#[derive(Debug, serde::Deserialize)]
struct OpenRouterResponse {
    data: Vec<OpenRouterModel>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenRouterModel {
    id: String,
    pricing: Option<OpenRouterPricing>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenRouterPricing {
    prompt: Option<String>,
    completion: Option<String>,
    input_cache_read: Option<String>,
    input_cache_write: Option<String>,
}

/// Fetch model pricing from the OpenRouter API and parse into normalized rates.
pub async fn fetch_openrouter() -> Result<HashMap<String, DynamicModelRates>, String> {
    let body = reqwest::get(OPENROUTER_URL)
        .await
        .map_err(|e| format!("OpenRouter HTTP fetch failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("OpenRouter read body failed: {e}"))?;

    let response: OpenRouterResponse =
        serde_json::from_str(&body).map_err(|e| format!("OpenRouter JSON parse failed: {e}"))?;

    Ok(parse_openrouter_models(&response.data))
}

fn parse_price(s: &Option<String>) -> Option<f64> {
    s.as_ref()?.parse::<f64>().ok()
}

fn parse_openrouter_models(models: &[OpenRouterModel]) -> HashMap<String, DynamicModelRates> {
    use crate::models::normalized_model_key;

    let mut rates: HashMap<String, DynamicModelRates> = HashMap::new();

    for model in models {
        let pricing = match &model.pricing {
            Some(p) => p,
            None => continue,
        };

        let input_per_token = match parse_price(&pricing.prompt) {
            Some(v) if v > 0.0 => v,
            _ => continue,
        };
        let output_per_token = match parse_price(&pricing.completion) {
            Some(v) if v > 0.0 => v,
            _ => continue,
        };

        let input = input_per_token * PER_MTOK;
        let output = output_per_token * PER_MTOK;

        let cache_read = parse_price(&pricing.input_cache_read)
            .filter(|&v| v > 0.0)
            .map(|v| v * PER_MTOK)
            .unwrap_or(input * 0.1);

        let cache_write_5m = parse_price(&pricing.input_cache_write)
            .filter(|&v| v > 0.0)
            .map(|v| v * PER_MTOK)
            .unwrap_or(input * 1.25);

        // OpenRouter doesn't distinguish 5m/1h tiers.
        let cache_write_1h = cache_write_5m;

        if input > MAX_RATE_PER_MTOK
            || output > MAX_RATE_PER_MTOK
            || cache_write_5m > MAX_RATE_PER_MTOK
            || cache_read > MAX_RATE_PER_MTOK
        {
            continue;
        }

        let model_rates = DynamicModelRates {
            input,
            output,
            cache_write_5m,
            cache_write_1h,
            cache_read,
        };

        // OpenRouter IDs are "provider/model-name" — extract the model part.
        let model_name = model.id.rsplit('/').next().unwrap_or(&model.id);

        let key = normalized_model_key(model_name);
        if key == "unknown" {
            let raw_key = model_name.trim().to_ascii_lowercase();
            rates.entry(raw_key).or_insert(model_rates);
        } else {
            rates.insert(key, model_rates);
        }
    }

    rates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_model(id: &str, prompt: &str, completion: &str) -> OpenRouterModel {
        OpenRouterModel {
            id: id.to_string(),
            pricing: Some(OpenRouterPricing {
                prompt: Some(prompt.to_string()),
                completion: Some(completion.to_string()),
                input_cache_read: None,
                input_cache_write: None,
            }),
        }
    }

    fn make_model_with_cache(
        id: &str,
        prompt: &str,
        completion: &str,
        cache_read: &str,
        cache_write: &str,
    ) -> OpenRouterModel {
        OpenRouterModel {
            id: id.to_string(),
            pricing: Some(OpenRouterPricing {
                prompt: Some(prompt.to_string()),
                completion: Some(completion.to_string()),
                input_cache_read: Some(cache_read.to_string()),
                input_cache_write: Some(cache_write.to_string()),
            }),
        }
    }

    #[test]
    fn parses_claude_model_from_openrouter() {
        let models = vec![make_model(
            "anthropic/claude-sonnet-4-5",
            "0.000003",
            "0.000015",
        )];

        let rates = parse_openrouter_models(&models);
        let r = rates.get("sonnet-4-5").expect("should have sonnet-4-5");
        assert!((r.input - 3.0).abs() < 0.001);
        assert!((r.output - 15.0).abs() < 0.001);
    }

    #[test]
    fn parses_openai_model_from_openrouter() {
        let models = vec![make_model("openai/gpt-5.4", "0.0000025", "0.000015")];

        let rates = parse_openrouter_models(&models);
        let r = rates.get("gpt-5.4").expect("should have gpt-5.4");
        assert!((r.input - 2.5).abs() < 0.001);
        assert!((r.output - 15.0).abs() < 0.001);
    }

    #[test]
    fn parses_chinese_model_from_openrouter() {
        let models = vec![make_model("zhipu/glm-4-plus", "0.000001", "0.000002")];

        let rates = parse_openrouter_models(&models);
        // normalized_model_key for "glm-4-plus" should be "glm-4-plus"
        let r = rates.get("glm-4-plus").expect("should have glm-4-plus");
        assert!((r.input - 1.0).abs() < 0.001);
        assert!((r.output - 2.0).abs() < 0.001);
    }

    #[test]
    fn uses_explicit_cache_pricing() {
        let models = vec![make_model_with_cache(
            "anthropic/claude-sonnet-4-5",
            "0.000003",
            "0.000015",
            "0.0000003",
            "0.00000375",
        )];

        let rates = parse_openrouter_models(&models);
        let r = rates.get("sonnet-4-5").expect("should have sonnet-4-5");
        assert!((r.cache_read - 0.3).abs() < 0.001);
        assert!((r.cache_write_5m - 3.75).abs() < 0.001);
    }

    #[test]
    fn skips_free_models() {
        let models = vec![make_model("meta/llama-free", "0", "0")];

        let rates = parse_openrouter_models(&models);
        assert!(rates.is_empty());
    }

    #[test]
    fn skips_models_without_pricing() {
        let models = vec![OpenRouterModel {
            id: "test/no-pricing".to_string(),
            pricing: None,
        }];

        let rates = parse_openrouter_models(&models);
        assert!(rates.is_empty());
    }

    #[test]
    fn rejects_rates_above_sanity_limit() {
        // $600/Mtok input exceeds the $500 cap.
        let models = vec![make_model("test/expensive", "0.0006", "0.0001")];

        let rates = parse_openrouter_models(&models);
        assert!(rates.is_empty());
    }

    #[test]
    fn strips_provider_prefix_from_id() {
        let models = vec![make_model(
            "deepseek/deepseek-chat",
            "0.000001",
            "0.000002",
        )];

        let rates = parse_openrouter_models(&models);
        assert!(rates.contains_key("deepseek-chat"));
    }
}
