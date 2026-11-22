use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_AGE_MINUTES: i64 = 30;
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResetWatch {
    pub reset_chance24h_percent: u8,
    pub reset_chance48h_percent: u8,
    pub confidence: String,
    pub observed_at: String,
    pub expires_at: String,
}

#[derive(Deserialize)]
struct ForecastResponse {
    probabilities: Probabilities,
    confidence: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct Probabilities {
    rounded_24h: u8,
    rounded_48h: u8,
}

fn active_forecast(raw: ForecastResponse, now: DateTime<Utc>) -> Option<ResetWatch> {
    let observed = DateTime::parse_from_rfc3339(&raw.updated_at).ok()?;
    let expires = observed.checked_add_signed(ChronoDuration::minutes(MAX_AGE_MINUTES))?;
    let probabilities = raw.probabilities;
    let current = observed <= now + ChronoDuration::minutes(5) && expires > now;
    let valid =
        probabilities.rounded_24h <= probabilities.rounded_48h && probabilities.rounded_48h <= 100;
    (current && valid).then(|| ResetWatch {
        reset_chance24h_percent: probabilities.rounded_24h,
        reset_chance48h_percent: probabilities.rounded_48h,
        confidence: raw.confidence,
        observed_at: observed.to_rfc3339(),
        expires_at: expires.to_rfc3339(),
    })
}

fn retry_delay(header: Option<&str>, now: DateTime<Utc>) -> Duration {
    let seconds = header.and_then(|value| {
        value.parse::<u64>().ok().or_else(|| {
            DateTime::parse_from_rfc2822(value)
                .ok()
                .map(|until| (until.with_timezone(&Utc) - now).num_seconds().max(0) as u64)
        })
    });
    Duration::from_secs(seconds.unwrap_or_default()).max(CACHE_TTL)
}

async fn fetch(url: &str) -> Result<(Option<ResetWatch>, Duration), String> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!(
            "TokenMonitor/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/TzJ2006/TokenMonitor)"
        ))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let delay = retry_delay(
            response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            Utc::now(),
        );
        return Ok((None, delay));
    }
    let forecast: ForecastResponse = response
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    Ok((active_forecast(forecast, Utc::now()), CACHE_TTL))
}

struct CachedForecast {
    fetched_at: Instant,
    retry_after: Duration,
    forecast: Option<ResetWatch>,
}

#[tauri::command]
pub async fn get_codex_reset_watch() -> Option<ResetWatch> {
    static CACHE: Mutex<Option<CachedForecast>> = Mutex::const_new(None);
    let mut cache = CACHE.lock().await;
    if let Some(cached) = cache.as_ref() {
        if cached.fetched_at.elapsed() < cached.retry_after {
            return cached.forecast.clone().filter(|forecast| {
                DateTime::parse_from_rfc3339(&forecast.expires_at)
                    .is_ok_and(|expires| expires > Utc::now())
            });
        }
    }
    let (forecast, retry_after) = fetch(crate::ops::codex_reset_forecast_url())
        .await
        .unwrap_or_else(|error| {
            tracing::debug!("codex reset forecast unavailable: {error}");
            (None, CACHE_TTL)
        });
    *cache = Some(CachedForecast {
        fetched_at: Instant::now(),
        retry_after,
        forecast: forecast.clone(),
    });
    forecast
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: serde_json::Value) -> Option<ResetWatch> {
        let now = "2026-10-21T06:00:00Z".parse().unwrap();
        serde_json::from_value(value)
            .ok()
            .and_then(|raw| active_forecast(raw, now))
    }

    fn forecast() -> serde_json::Value {
        serde_json::json!({
            "probabilities": {"rounded_24h": 22, "rounded_48h": 39},
            "confidence": "low",
            "updated_at": "2026-10-21T05:50:00Z"
        })
    }

    #[test]
    fn reads_documented_probabilities_and_serializes_for_the_frontend() {
        let forecast = parse(forecast()).unwrap();
        assert_eq!(forecast.reset_chance24h_percent, 22);
        assert_eq!(forecast.reset_chance48h_percent, 39);
        assert_eq!(forecast.confidence, "low");
        assert_eq!(forecast.expires_at, "2026-10-21T06:20:00+00:00");
        let json = serde_json::to_value(forecast).unwrap();
        assert_eq!(json["resetChance24hPercent"], 22);
        assert_eq!(json["resetChance48hPercent"], 39);
    }

    #[test]
    fn rejects_stale_future_and_malformed_timestamps() {
        for timestamp in ["2026-10-21T05:30:00Z", "2026-10-21T06:06:00Z", "bad"] {
            let mut raw = forecast();
            raw["updated_at"] = timestamp.into();
            assert!(parse(raw).is_none(), "{timestamp}");
        }
    }

    #[test]
    fn rejects_missing_invalid_and_decreasing_probabilities() {
        for probabilities in [
            serde_json::json!({"rounded_24h": 40, "rounded_48h": 39}),
            serde_json::json!({"rounded_24h": 22, "rounded_48h": 101}),
            serde_json::json!({"rounded_24h": -1, "rounded_48h": 39}),
            serde_json::json!({"rounded_24h": null, "rounded_48h": 39}),
            serde_json::json!({"rounded_24h": 22}),
        ] {
            let mut raw = forecast();
            raw["probabilities"] = probabilities;
            assert!(parse(raw).is_none());
        }
        assert!(parse(serde_json::json!({"available": false})).is_none());
    }

    #[test]
    fn honors_retry_after_without_polling_faster_than_the_cache() {
        let now = "2026-10-21T06:00:00Z".parse().unwrap();
        assert_eq!(retry_delay(Some("3600"), now), Duration::from_secs(3600));
        assert_eq!(
            retry_delay(Some("Wed, 21 Oct 2026 07:00:00 GMT"), now),
            Duration::from_secs(3600)
        );
        for header in [None, Some("bad"), Some("60")] {
            assert_eq!(retry_delay(header, now), CACHE_TTL);
        }
    }

    #[tokio::test]
    async fn fetches_forecasts_and_falls_back_on_http_and_payload_failures() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let mut valid = forecast();
        valid["updated_at"] = Utc::now().to_rfc3339().into();
        for (status, body, expected_chance, expected_delay) in [
            ("200 OK", valid.to_string(), Some(22), CACHE_TTL),
            (
                "200 OK",
                forecast().to_string().replace("2026-10-21", "2020-01-01"),
                None,
                CACHE_TTL,
            ),
            ("200 OK", "not json".into(), None, CACHE_TTL),
            ("503 Service Unavailable", "{}".into(), None, CACHE_TTL),
            (
                "429 Too Many Requests",
                "{}".into(),
                None,
                Duration::from_secs(3600),
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                }
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nRetry-After: 3600\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                String::from_utf8(request).unwrap()
            });
            let (forecast, delay) = fetch(&format!("http://{address}/api/forecast"))
                .await
                .unwrap_or((None, CACHE_TTL));
            assert_eq!(
                forecast.map(|value| value.reset_chance24h_percent),
                expected_chance,
                "{status}"
            );
            assert_eq!(delay, expected_delay, "{status}");
            let request = tokio::time::timeout(TIMEOUT, server)
                .await
                .unwrap()
                .unwrap();
            assert!(request.starts_with("GET /api/forecast HTTP/1.1"));
            assert!(request.contains("TokenMonitor/"));
            assert!(request.contains("https://github.com/TzJ2006/TokenMonitor"));
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        assert!(fetch(&format!("http://{address}/api/forecast"))
            .await
            .is_err());
    }
}
