use crate::models::ProviderRateLimits;
use chrono::{DateTime, Duration, Local, Utc};
use reqwest::header::{HeaderMap, RETRY_AFTER};

use super::RateLimitFetchError;

pub(crate) fn parse_retry_after_seconds(headers: &HeaderMap, now: DateTime<Utc>) -> Option<u64> {
    let raw = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();

    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(seconds);
    }

    let retry_at = DateTime::parse_from_rfc2822(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            DateTime::parse_from_rfc3339(raw)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        })?;

    let remaining = retry_at.signed_duration_since(now).num_seconds();
    Some(remaining.max(0) as u64)
}

fn parse_header_datetime(headers: &HeaderMap, name: &str) -> Option<DateTime<Utc>> {
    let raw = headers.get(name)?.to_str().ok()?.trim();

    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            DateTime::parse_from_rfc2822(raw)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        })
}

fn parse_reset_datetimes(headers: &HeaderMap) -> Vec<DateTime<Utc>> {
    [
        "anthropic-ratelimit-requests-reset",
        "anthropic-ratelimit-tokens-reset",
        "anthropic-ratelimit-input-tokens-reset",
        "anthropic-ratelimit-output-tokens-reset",
    ]
    .into_iter()
    .filter_map(|name| parse_header_datetime(headers, name))
    .collect()
}

pub(crate) fn cooldown_metadata(
    headers: &HeaderMap,
    now: DateTime<Utc>,
) -> (Option<u64>, Option<String>) {
    let retry_after_until = parse_retry_after_seconds(headers, now)
        .map(|seconds| now + Duration::seconds(seconds as i64));
    let reset_until = parse_reset_datetimes(headers).into_iter().max();
    let cooldown_until = match (retry_after_until, reset_until) {
        (Some(retry_after_until), Some(reset_until)) => Some(retry_after_until.max(reset_until)),
        (Some(retry_after_until), None) => Some(retry_after_until),
        (None, Some(reset_until)) => Some(reset_until),
        (None, None) => None,
    };

    let retry_after_seconds = cooldown_until
        .as_ref()
        .map(|retry_at| retry_at.signed_duration_since(now).num_seconds().max(0) as u64);

    (
        retry_after_seconds,
        cooldown_until.map(|retry_at| retry_at.to_rfc3339()),
    )
}

pub(crate) fn rate_limit_error_from_response(response: &reqwest::Response) -> RateLimitFetchError {
    let now = Utc::now();
    let (retry_after_seconds, cooldown_until) = cooldown_metadata(response.headers(), now);

    RateLimitFetchError {
        message: format!("Usage API returned {}", response.status()),
        retry_after_seconds,
        cooldown_until,
    }
}

pub(super) fn provider_rate_limit_error(
    provider: &str,
    error: RateLimitFetchError,
) -> ProviderRateLimits {
    ProviderRateLimits {
        provider: provider.to_string(),
        plan_tier: None,
        windows: vec![],
        extra_usage: None,
        stale: false,
        error: Some(error.message),
        retry_after_seconds: error.retry_after_seconds,
        cooldown_until: error.cooldown_until,
        fetched_at: Local::now().to_rfc3339(),
    }
}

pub(super) fn mark_rate_limits_stale(mut rate_limits: ProviderRateLimits) -> ProviderRateLimits {
    rate_limits.stale = true;
    rate_limits
}

pub(super) fn provider_cooldown_is_active(
    rate_limits: &ProviderRateLimits,
    now: DateTime<Utc>,
) -> bool {
    rate_limits
        .cooldown_until
        .as_deref()
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|dt| dt.with_timezone(&Utc) > now)
        .unwrap_or(false)
}

pub(super) fn merge_provider_rate_limits(
    fresh: Option<ProviderRateLimits>,
    cached: Option<ProviderRateLimits>,
) -> Option<ProviderRateLimits> {
    match (fresh, cached) {
        (Some(fresh), Some(cached))
            if fresh.windows.is_empty() && fresh.error.is_some() && !cached.windows.is_empty() =>
        {
            Some(ProviderRateLimits {
                stale: true,
                error: fresh.error,
                retry_after_seconds: fresh.retry_after_seconds,
                cooldown_until: fresh.cooldown_until,
                fetched_at: fresh.fetched_at,
                ..cached
            })
        }
        (Some(fresh), _) => Some(fresh),
        (None, cached) => cached,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RateLimitWindow;
    use reqwest::header::HeaderValue;

    fn provider_rate_limits(
        windows: Vec<RateLimitWindow>,
        error: Option<&str>,
        cooldown_until: Option<&str>,
    ) -> ProviderRateLimits {
        ProviderRateLimits {
            provider: "claude".to_string(),
            plan_tier: Some("Pro".to_string()),
            windows,
            extra_usage: None,
            stale: false,
            error: error.map(ToString::to_string),
            retry_after_seconds: None,
            cooldown_until: cooldown_until.map(ToString::to_string),
            fetched_at: "2026-03-17T12:00:00Z".to_string(),
        }
    }

    #[test]
    fn parses_retry_after_seconds_header() {
        let now = DateTime::parse_from_rfc3339("2026-03-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("42"));

        assert_eq!(parse_retry_after_seconds(&headers, now), Some(42));
    }

    #[test]
    fn parses_retry_after_http_date_header() {
        let now = DateTime::parse_from_rfc3339("2026-03-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut headers = HeaderMap::new();
        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Tue, 17 Mar 2026 12:00:30 GMT"),
        );

        assert_eq!(parse_retry_after_seconds(&headers, now), Some(30));
    }

    #[test]
    fn uses_the_latest_retry_boundary_when_retry_after_and_reset_headers_disagree() {
        let now = DateTime::parse_from_rfc3339("2026-03-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("5"));
        headers.insert(
            "anthropic-ratelimit-requests-reset",
            HeaderValue::from_static("2026-03-17T12:01:00Z"),
        );
        headers.insert(
            "anthropic-ratelimit-output-tokens-reset",
            HeaderValue::from_static("2026-03-17T12:02:00Z"),
        );

        let (retry_after_seconds, cooldown_until) = cooldown_metadata(&headers, now);

        assert_eq!(retry_after_seconds, Some(120));
        assert_eq!(cooldown_until.as_deref(), Some("2026-03-17T12:02:00+00:00"));
    }

    #[test]
    fn merges_cached_windows_with_fresh_error_metadata() {
        let cached = provider_rate_limits(
            vec![RateLimitWindow {
                window_id: "five_hour".to_string(),
                label: "Session (5hr)".to_string(),
                utilization: 33.0,
                resets_at: Some("2026-03-17T14:00:00Z".to_string()),
            }],
            None,
            None,
        );
        let mut fresh_error = provider_rate_limits(
            vec![],
            Some("Usage API returned 429 Too Many Requests"),
            Some("2026-03-17T12:05:00Z"),
        );
        fresh_error.fetched_at = "2026-03-17T12:04:00Z".to_string();

        let merged = merge_provider_rate_limits(Some(fresh_error), Some(cached)).unwrap();

        assert!(merged.stale);
        assert_eq!(merged.windows.len(), 1);
        assert_eq!(
            merged.error.as_deref(),
            Some("Usage API returned 429 Too Many Requests"),
        );
        assert_eq!(
            merged.cooldown_until.as_deref(),
            Some("2026-03-17T12:05:00Z"),
        );
        assert_eq!(merged.fetched_at, "2026-03-17T12:04:00Z");
    }

    #[test]
    fn detects_active_provider_cooldown() {
        let now = DateTime::parse_from_rfc3339("2026-03-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let rate_limits = provider_rate_limits(vec![], Some("429"), Some("2026-03-17T12:01:00Z"));

        assert!(provider_cooldown_is_active(&rate_limits, now));
    }
}
