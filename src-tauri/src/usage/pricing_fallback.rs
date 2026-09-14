//! Static model pricing table — edit `pricing_fallback.json` to update rates.
//!
//! Loaded at compile time via `include_str!` so it works offline with zero
//! network dependency. Lookup runs after dynamic LiteLLM/OpenRouter pricing
//! and before the inline GPT/Claude hardcoded blocks in `pricing.rs`.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

const FALLBACK_JSON: &str = include_str!("pricing_fallback.json");

#[derive(Debug, Deserialize)]
struct FallbackTable {
    #[serde(default)]
    aliases: HashMap<String, String>,
    models: HashMap<String, FallbackRates>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct FallbackRates {
    input: f64,
    output: f64,
    #[serde(default)]
    cache_write_5m: Option<f64>,
    #[serde(default)]
    cache_write_1h: Option<f64>,
    #[serde(default)]
    cache_read: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct FallbackModelRates {
    pub input: f64,
    pub output: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
}

struct ParsedFallback {
    aliases: HashMap<String, String>,
    models: HashMap<String, FallbackModelRates>,
}

static TABLE: OnceLock<ParsedFallback> = OnceLock::new();

fn parsed_table() -> &'static ParsedFallback {
    TABLE.get_or_init(|| {
        let raw: FallbackTable =
            serde_json::from_str(FALLBACK_JSON).expect("pricing_fallback.json must be valid JSON");
        let models = raw
            .models
            .into_iter()
            .filter_map(|(key, entry)| normalize_rates(&key, entry).map(|r| (key, r)))
            .collect();
        ParsedFallback {
            aliases: raw.aliases,
            models,
        }
    })
}

fn normalize_rates(key: &str, entry: FallbackRates) -> Option<FallbackModelRates> {
    if entry.input <= 0.0 || entry.output <= 0.0 {
        tracing::warn!(
            model = key,
            "pricing_fallback: skipping model with non-positive rates"
        );
        return None;
    }

    let cache_write_5m = entry.cache_write_5m.unwrap_or(entry.input * 1.25);
    let cache_write_1h = entry.cache_write_1h.unwrap_or(cache_write_5m);
    let cache_read = entry.cache_read.unwrap_or(entry.input * 0.1);

    Some(FallbackModelRates {
        input: entry.input,
        output: entry.output,
        cache_write_5m,
        cache_write_1h,
        cache_read,
    })
}

/// Resolve a normalized model key against the static fallback table.
pub fn lookup(model_key: &str) -> Option<FallbackModelRates> {
    let table = parsed_table();
    let key = model_key.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }

    if let Some(rates) = table.models.get(&key) {
        return Some(*rates);
    }

    let resolved = table.aliases.get(&key)?;
    table.models.get(resolved).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_composer_rates_from_json() {
        let rates = lookup("composer-2.5").expect("composer-2.5");
        assert!((rates.input - 0.5).abs() < 1e-9);
        assert!((rates.output - 2.5).abs() < 1e-9);
        assert!((rates.cache_read - 0.2).abs() < 1e-9);
    }

    #[test]
    fn resolves_aliases() {
        let direct = lookup("composer-2.5").unwrap();
        let alias = lookup("composer-latest").unwrap();
        assert!((direct.input - alias.input).abs() < 1e-9);
        assert!((direct.output - alias.output).abs() < 1e-9);
    }

    #[test]
    fn deepseek_alias_maps_to_v4_flash() {
        let rates = lookup("deepseek-chat").expect("deepseek-chat alias");
        assert!((rates.input - 0.14).abs() < 1e-9);
    }

    #[test]
    fn grok_and_gemini_entries_exist() {
        assert!(lookup("grok-4.3").is_some());
        assert!(lookup("grok-4.5").is_some());
        assert!(lookup("grok-4.5-fast").is_some());
        assert!(lookup("cursor-grok-4.5-high-fast").is_some());
        let grok46 = lookup("grok-4.6").expect("grok-4.6");
        assert!((grok46.input - 2.0).abs() < 1e-9);
        assert!((grok46.output - 6.0).abs() < 1e-9);
        assert!((grok46.cache_read - 0.5).abs() < 1e-9);
        let grok46_fast = lookup("grok-4.6-fast").expect("grok-4.6-fast");
        assert!((grok46_fast.input - 4.0).abs() < 1e-9);
        assert!((grok46_fast.output - 18.0).abs() < 1e-9);
        assert!((grok46_fast.cache_read - 1.0).abs() < 1e-9);
        assert!(lookup("cursor-grok-4.6-high-fast").is_some());
        assert!(lookup("cursor-grok-4.6-xhigh-fast").is_some());
        assert!(lookup("cursor-grok-4.6-xhigh").is_some());
        assert!(lookup("gemini-2.5-pro").is_some());
    }

    #[test]
    fn unknown_model_returns_none() {
        assert!(lookup("totally-unknown-model").is_none());
    }
}
