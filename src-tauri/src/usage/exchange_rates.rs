use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

static EXCHANGE_RATES: OnceLock<RwLock<HashMap<String, f64>>> = OnceLock::new();

const API_URL: &str = "https://api.frankfurter.dev/v1/latest?from=USD&to=EUR,GBP,JPY,CNY";

const CACHE_FILENAME: &str = "exchange-rates-cache.json";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60; // 24 hours

#[derive(Debug, Serialize, Deserialize)]
struct ExchangeRateCache {
    fetched_at: u64,
    rates: HashMap<String, f64>,
}

#[derive(Debug, Deserialize)]
struct FrankfurterResponse {
    rates: HashMap<String, f64>,
}

fn cache_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CACHE_FILENAME)
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn should_refresh(app_data_dir: &Path) -> bool {
    let path = cache_path(app_data_dir);
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<ExchangeRateCache>(&content) {
            Ok(cache) => now_epoch().saturating_sub(cache.fetched_at) > CACHE_TTL_SECS,
            Err(_) => true,
        },
        Err(_) => true,
    }
}

pub fn load_cached(app_data_dir: &Path) -> Option<HashMap<String, f64>> {
    let content = std::fs::read_to_string(cache_path(app_data_dir)).ok()?;
    let cache: ExchangeRateCache = serde_json::from_str(&content).ok()?;
    Some(cache.rates)
}

pub async fn fetch_and_cache(app_data_dir: &Path) -> Result<HashMap<String, f64>, String> {
    let body = reqwest::get(API_URL)
        .await
        .map_err(|e| format!("Exchange rate HTTP fetch failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Exchange rate read body failed: {e}"))?;

    let response: FrankfurterResponse =
        serde_json::from_str(&body).map_err(|e| format!("Exchange rate JSON parse failed: {e}"))?;

    let rates = response.rates;

    let cache = ExchangeRateCache {
        fetched_at: now_epoch(),
        rates: rates.clone(),
    };
    let json = serde_json::to_string(&cache).map_err(|e| format!("serialize: {e}"))?;

    let path = cache_path(app_data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create cache directory {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, json).map_err(|e| format!("write cache: {e}"))?;

    Ok(rates)
}

pub fn set_exchange_rates(rates: HashMap<String, f64>) {
    let lock = EXCHANGE_RATES.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(mut guard) = lock.write() {
        *guard = rates;
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn get_rate(currency: &str) -> Option<f64> {
    let lock = EXCHANGE_RATES.get()?;
    let guard = lock.read().ok()?;
    guard.get(currency).copied()
}

pub fn get_all_rates() -> HashMap<String, f64> {
    let Some(lock) = EXCHANGE_RATES.get() else {
        return HashMap::new();
    };
    lock.read().map(|g| g.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trip() {
        let dir = tempfile::tempdir().unwrap();

        assert!(should_refresh(dir.path()));

        let mut rates = HashMap::new();
        rates.insert("EUR".to_string(), 0.85);
        rates.insert("JPY".to_string(), 159.0);

        let cache = ExchangeRateCache {
            fetched_at: now_epoch(),
            rates: rates.clone(),
        };
        let json = serde_json::to_string(&cache).unwrap();
        std::fs::write(cache_path(dir.path()), json).unwrap();

        assert!(!should_refresh(dir.path()));

        let loaded = load_cached(dir.path()).unwrap();
        assert_eq!(loaded.get("EUR"), Some(&0.85));
        assert_eq!(loaded.get("JPY"), Some(&159.0));
    }

    #[test]
    fn stale_cache_triggers_refresh() {
        let dir = tempfile::tempdir().unwrap();

        let cache = ExchangeRateCache {
            fetched_at: now_epoch() - CACHE_TTL_SECS - 1,
            rates: HashMap::new(),
        };
        let json = serde_json::to_string(&cache).unwrap();
        std::fs::write(cache_path(dir.path()), json).unwrap();

        assert!(should_refresh(dir.path()));
    }

    #[test]
    fn corrupt_cache_triggers_refresh() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(cache_path(dir.path()), "not json").unwrap();
        assert!(should_refresh(dir.path()));
    }

    #[test]
    fn global_rates_set_and_get() {
        let mut rates = HashMap::new();
        rates.insert("GBP".to_string(), 0.74);
        set_exchange_rates(rates);
        assert_eq!(get_rate("GBP"), Some(0.74));
        assert_eq!(get_rate("NOPE"), None);
    }

    #[test]
    fn get_all_rates_returns_empty_before_init() {
        // OnceLock may already be initialized from another test in this process,
        // so we just verify it doesn't panic and returns a map.
        let all = get_all_rates();
        assert!(all.is_empty() || all.contains_key("GBP"));
    }
}
