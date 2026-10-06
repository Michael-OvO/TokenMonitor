use crate::models::RateLimitsPayload;
use chrono::{DateTime, Datelike, Duration, Local, Months, NaiveDate, TimeZone, Weekday};
use std::sync::atomic::{AtomicBool, Ordering};

/// A single mode for day/week/month/year; Usage is independent.
static ROLLING_PERIODS: AtomicBool = AtomicBool::new(false);

// Retain the old IPC's weekday argument for compatibility; weeks now start Monday.
pub(crate) fn set_period_config(_week_start: Weekday, rolling: bool) {
    ROLLING_PERIODS.store(rolling, Ordering::Relaxed);
}

pub(crate) fn period_config() -> (Weekday, bool) {
    (Weekday::Mon, ROLLING_PERIODS.load(Ordering::Relaxed))
}

#[cfg(test)]
pub(crate) fn parse_bucket_start_date(sort_key: &str) -> Result<NaiveDate, chrono::ParseError> {
    NaiveDate::parse_from_str(sort_key, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(&format!("{sort_key}-01"), "%Y-%m-%d"))
}

/// Normalize a (base_year, base_month) pair shifted by `offset` months into a valid (year, month).
pub(crate) fn resolve_month_offset(base_year: i32, base_month: u32, offset: i32) -> (i32, u32) {
    let mut y = base_year;
    let mut m = base_month as i32 + offset;
    while m <= 0 {
        y -= 1;
        m += 12;
    }
    while m > 12 {
        y += 1;
        m -= 12;
    }
    (y, m as u32)
}

/// Return the first day of the month following (year, month).
/// Returns None if the resulting date is out of range.
pub(crate) fn first_of_next_month(year: i32, month: u32) -> Option<NaiveDate> {
    if month == 12 {
        NaiveDate::from_ymd_opt(year.checked_add(1)?, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
}

fn local_midnight(date: NaiveDate) -> DateTime<Local> {
    let naive = date.and_hms_opt(0, 0, 0).unwrap_or_default();
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => dt,
        // ponytail: DST spring-forward can skip midnight; one hour later still
        // lands on the intended calendar day for date-period bounds.
        chrono::LocalResult::None => Local
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest()
            .unwrap_or_else(Local::now),
    }
}

/// Resolved date range and label for a (period, offset) pair.
/// Single source of truth — computed once and threaded through the pipeline.
pub(crate) struct PeriodBounds {
    pub start: NaiveDate,
    pub period_label: String,
    /// Inclusive start instant. Calendar periods: local midnight of `start`.
    pub range_start: DateTime<Local>,
    /// Exclusive end instant; current to-date and rolling windows end now.
    pub range_end: DateTime<Local>,
}

impl PeriodBounds {
    fn from_dates(start: NaiveDate, end: NaiveDate, period_label: String) -> Self {
        Self {
            range_start: local_midnight(start),
            range_end: local_midnight(end),
            start,
            period_label,
        }
    }

    pub(crate) fn from_range(
        range_start: DateTime<Local>,
        range_end: DateTime<Local>,
        period_label: String,
    ) -> Self {
        let start = range_start.date_naive();
        Self {
            start,
            period_label,
            range_start,
            range_end,
        }
    }

    pub fn contains_timestamp(&self, ts: DateTime<Local>) -> bool {
        ts >= self.range_start && ts < self.range_end
    }
}

/// Claude/Codex official 5h window end (`resets_at`). Cursor has no 5h meter
/// (billing cycle), so All/Cursor reuse Claude's window. Missing → `None`
/// and callers fall back to rolling `now-5h..now`.
pub(crate) fn official_five_hour_reset(
    provider: &str,
    payload: Option<&RateLimitsPayload>,
) -> Option<DateTime<Local>> {
    let payload = payload?;
    let (limits, window_id) = match provider {
        "codex" => (payload.codex.as_ref()?, "primary"),
        _ => (payload.claude.as_ref()?, "five_hour"),
    };
    let resets = limits
        .windows
        .iter()
        .find(|w| w.window_id == window_id)?
        .resets_at
        .as_deref()?;
    DateTime::parse_from_rfc3339(resets)
        .ok()
        .map(|dt| dt.with_timezone(&Local))
}

pub(crate) fn resolve_period_bounds_for_provider(
    period: &str,
    offset: i32,
    provider: &str,
    rate_limits: Option<&RateLimitsPayload>,
) -> Result<PeriodBounds, String> {
    let reset = if period == "5h" {
        official_five_hour_reset(provider, rate_limits)
    } else {
        None
    };
    resolve_period_bounds_with_reset(period, offset, reset)
}

pub(crate) fn resolve_period_bounds(period: &str, offset: i32) -> Result<PeriodBounds, String> {
    resolve_period_bounds_with_reset(period, offset, None)
}

pub(crate) fn resolve_period_bounds_with_reset(
    period: &str,
    offset: i32,
    five_hour_reset: Option<DateTime<Local>>,
) -> Result<PeriodBounds, String> {
    let (week_start, rolling) = period_config();
    resolve_period_bounds_configured(
        period,
        offset,
        Local::now(),
        five_hour_reset,
        week_start,
        rolling,
    )
}

fn five_hour_range(
    offset: i32,
    now: DateTime<Local>,
    reset: Option<DateTime<Local>>,
) -> (DateTime<Local>, DateTime<Local>) {
    let window = Duration::hours(5);
    // Official window is [resets_at - 5h, resets_at). Offset shifts by whole
    // windows. Expired/missing resets_at: rolling now-5h..now — not calendar
    // days, not JSONL session gaps.
    let end = match reset.filter(|r| *r > now) {
        Some(r) => r + window * offset,
        None => now + window * offset,
    };
    (end - window, end)
}

/// Test-only: calendar-aligned, Monday-start bounds at a fixed `now`.
#[cfg(test)]
pub(crate) fn resolve_period_bounds_at(
    period: &str,
    offset: i32,
    now: DateTime<Local>,
    five_hour_reset: Option<DateTime<Local>>,
) -> Result<PeriodBounds, String> {
    resolve_period_bounds_configured(period, offset, now, five_hour_reset, Weekday::Mon, false)
}

/// Both boundaries are shifted from the same anchor, so month-end clamping
/// never creates gaps or overlaps when navigating history.
fn rolling_bounds(period: &str, offset: i32, now: DateTime<Local>) -> Option<PeriodBounds> {
    let shift = |units: i32| -> Option<DateTime<Local>> {
        if units == 0 {
            return Some(now);
        }
        match period {
            "day" => now.checked_add_signed(Duration::hours(24 * i64::from(units))),
            "week" => now.checked_add_signed(Duration::hours(168 * i64::from(units))),
            "month" | "year" => {
                let months = units.checked_mul(if period == "year" { 12 } else { 1 })?;
                let date = shift_months(now.date_naive(), months)?;
                let naive = date.and_time(now.time());
                // Choose the first occurrence at fall-back and move through a
                // skipped spring-forward hour, preserving a usable boundary.
                Local.from_local_datetime(&naive).earliest().or_else(|| {
                    Local
                        .from_local_datetime(&(naive + Duration::hours(1)))
                        .earliest()
                })
            }
            _ => None,
        }
    };
    let end = shift(offset)?;
    let start = shift(offset.checked_sub(1)?)?;
    let label = format!(
        "{} \u{2013} {}",
        start.format("%b %-d, %Y"),
        end.format("%b %-d, %Y")
    );
    Some(PeriodBounds::from_range(start, end, label))
}

fn shift_months(d: NaiveDate, months: i32) -> Option<NaiveDate> {
    if months >= 0 {
        d.checked_add_months(Months::new(months as u32))
    } else {
        d.checked_sub_months(Months::new(months.unsigned_abs()))
    }
}

fn resolve_period_bounds_configured(
    period: &str,
    offset: i32,
    now: DateTime<Local>,
    five_hour_reset: Option<DateTime<Local>>,
    _week_start: Weekday,
    rolling: bool,
) -> Result<PeriodBounds, String> {
    let today = now.date_naive();
    if rolling && matches!(period, "day" | "week" | "month" | "year") {
        return rolling_bounds(period, offset, now)
            .ok_or_else(|| format!("Rolling {period} offset out of range: {offset}"));
    }
    let mut bounds = match period {
        "5h" => {
            let (range_start, range_end) = five_hour_range(offset, now, five_hour_reset);
            Ok(PeriodBounds::from_range(
                range_start,
                range_end,
                String::new(),
            ))
        }
        "day" => {
            let target = today + Duration::days(offset as i64);
            Ok(PeriodBounds::from_dates(
                target,
                target + Duration::days(1),
                format_day_label(target),
            ))
        }
        "week" => {
            let days_since_start = now.weekday().num_days_from_monday();
            let current_monday = today - Duration::days(days_since_start as i64);
            let target_monday = current_monday + Duration::days((offset * 7) as i64);
            let end = target_monday + Duration::days(7);
            let target_sunday = end - Duration::days(1);
            Ok(PeriodBounds::from_dates(
                target_monday,
                end,
                format_week_label(target_monday, target_sunday),
            ))
        }
        "month" => {
            let (year, month) = resolve_month_offset(now.year(), now.month(), offset);
            let start = NaiveDate::from_ymd_opt(year, month, 1)
                .ok_or_else(|| format!("Invalid month: year={year}, month={month}"))?;
            let end = first_of_next_month(year, month)
                .ok_or_else(|| format!("Invalid next month: year={year}, month={month}"))?;
            Ok(PeriodBounds::from_dates(
                start,
                end,
                format_month_label(start),
            ))
        }
        "year" => {
            let target_year = now
                .year()
                .checked_add(offset)
                .ok_or_else(|| format!("Year offset overflow: {offset}"))?;
            let start = NaiveDate::from_ymd_opt(target_year, 1, 1)
                .ok_or_else(|| format!("Invalid year: {target_year}"))?;
            let next_year = target_year
                .checked_add(1)
                .ok_or_else(|| format!("Year+1 overflow: {target_year}"))?;
            let end = NaiveDate::from_ymd_opt(next_year, 1, 1)
                .ok_or_else(|| format!("Invalid next year: {next_year}"))?;
            Ok(PeriodBounds::from_dates(
                start,
                end,
                format_year_label(target_year),
            ))
        }
        _ => Err(format!("Unknown period: {period}")),
    }?;
    if period != "5h" && offset == 0 {
        bounds = PeriodBounds::from_range(
            bounds.range_start,
            now,
            if period == "day" {
                format_day_label(today)
            } else {
                format_week_label(bounds.start, today)
            },
        );
    }
    Ok(bounds)
}

/// Convenience wrapper for callers that only need (start, end) dates.
#[cfg(test)]
pub(crate) fn compute_date_bounds(period: &str, offset: i32) -> Option<(NaiveDate, NaiveDate)> {
    resolve_period_bounds(period, offset).ok().map(|b| {
        let end = b.range_end.date_naive()
            + if b.range_end.time() == chrono::NaiveTime::MIN {
                Duration::zero()
            } else {
                Duration::days(1)
            };
        (b.start, end)
    })
}

pub(crate) fn format_day_label(date: NaiveDate) -> String {
    date.format("%B %-d, %Y").to_string()
}

pub(crate) fn format_week_label(monday: NaiveDate, sunday: NaiveDate) -> String {
    if monday.year() != sunday.year() {
        format!(
            "{} \u{2013} {}",
            monday.format("%b %-d, %Y"),
            sunday.format("%b %-d, %Y")
        )
    } else if monday.month() != sunday.month() {
        format!(
            "{} \u{2013} {}",
            monday.format("%b %-d"),
            sunday.format("%b %-d, %Y")
        )
    } else {
        format!(
            "{} \u{2013} {}",
            monday.format("%b %-d"),
            sunday.format("%-d, %Y")
        )
    }
}

pub(crate) fn format_month_label(first_of_month: NaiveDate) -> String {
    first_of_month.format("%B %Y").to_string()
}

pub(crate) fn format_year_label(year: i32) -> String {
    year.to_string()
}

pub(crate) fn month_offset_from_now(year: i32, month: u32) -> i32 {
    let now = Local::now();
    (year - now.year()) * 12 + month as i32 - now.month() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderRateLimits, RateLimitWindow, RateLimitsPayload};

    fn local(year: i32, month: u32, day: u32, hour: u32, min: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, min, 0)
            .single()
            .unwrap()
    }

    fn claude_payload(resets_at: &str) -> RateLimitsPayload {
        RateLimitsPayload {
            claude: Some(ProviderRateLimits {
                provider: "claude".into(),
                plan_tier: None,
                windows: vec![RateLimitWindow::new(
                    "five_hour".into(),
                    "Session (5hr)".into(),
                    10.0,
                    Some(resets_at.to_string()),
                )],
                extra_usage: None,
                credits: None,
                stale: false,
                error: None,
                retry_after_seconds: None,
                cooldown_until: None,
                fetched_at: resets_at.to_string(),
            }),
            codex: None,
            cursor: None,
            kimi: None,
        }
    }

    #[test]
    fn to_date_starts_at_calendar_boundary_and_ends_now() {
        let now = local(2026, 9, 16, 12, 30);
        for (period, start) in [
            ("day", local(2026, 9, 16, 0, 0)),
            ("week", local(2026, 9, 14, 0, 0)),
            ("month", local(2026, 9, 1, 0, 0)),
            ("year", local(2026, 1, 1, 0, 0)),
        ] {
            let b = resolve_period_bounds_configured(period, 0, now, None, Weekday::Sun, false)
                .unwrap();
            assert_eq!(b.range_start, start, "{period}");
            assert_eq!(b.range_end, now, "{period}");
            assert!(!b.contains_timestamp(now));
        }
        let prev = resolve_period_bounds_at("week", -1, now, None).unwrap();
        assert_eq!(prev.range_start, local(2026, 9, 7, 0, 0));
        assert_eq!(prev.range_end, local(2026, 9, 14, 0, 0));
    }

    #[test]
    fn rolling_windows_end_now_and_history_is_adjacent() {
        let now = local(2026, 3, 31, 12, 30);
        for (period, start) in [
            ("day", now - Duration::hours(24)),
            ("week", now - Duration::hours(168)),
            ("month", local(2026, 2, 28, 12, 30)),
            ("year", local(2025, 3, 31, 12, 30)),
        ] {
            let current =
                resolve_period_bounds_configured(period, 0, now, None, Weekday::Mon, true).unwrap();
            assert_eq!(current.range_start, start, "{period}");
            assert_eq!(current.range_end, now, "{period}");
            let prev = resolve_period_bounds_configured(period, -1, now, None, Weekday::Mon, true)
                .unwrap();
            assert_eq!(prev.range_end, start, "{period}");
            let older = resolve_period_bounds_configured(period, -2, now, None, Weekday::Mon, true)
                .unwrap();
            assert_eq!(older.range_end, prev.range_start, "{period}");
        }
        let leap = local(2024, 2, 29, 12, 30);
        let year =
            resolve_period_bounds_configured("year", 0, leap, None, Weekday::Mon, true).unwrap();
        assert_eq!(year.range_start, local(2023, 2, 28, 12, 30));
        let reset = now + Duration::hours(2);
        let usage = resolve_period_bounds_configured("5h", 0, now, Some(reset), Weekday::Mon, true)
            .unwrap();
        assert_eq!(usage.range_end, reset);
    }

    #[test]
    fn rolling_label_stops_on_the_date() {
        let now = local(2025, 5, 25, 15, 42);
        let bounds =
            resolve_period_bounds_configured("year", 0, now, None, Weekday::Mon, true).unwrap();
        assert_eq!(bounds.period_label, "May 25, 2024 \u{2013} May 25, 2025");
        assert!(!bounds.period_label.contains(':'));
        assert_eq!(bounds.range_end, now);
        assert!(!bounds.contains_timestamp(local(2025, 5, 26, 0, 0)));
    }

    #[test]
    fn five_h_uses_official_reset_window() {
        let now = local(2026, 8, 14, 16, 0);
        let reset = local(2026, 8, 14, 18, 0);
        let bounds = resolve_period_bounds_at("5h", 0, now, Some(reset)).unwrap();
        assert_eq!(bounds.range_start, reset - Duration::hours(5));
        assert_eq!(bounds.range_end, reset);
        assert!(bounds.contains_timestamp(local(2026, 8, 14, 13, 30)));
        assert!(!bounds.contains_timestamp(local(2026, 8, 14, 12, 59)));
    }

    #[test]
    fn five_h_cross_midnight_keeps_previous_evening() {
        let now = local(2026, 8, 15, 1, 0);
        let reset = local(2026, 8, 15, 3, 0);
        let bounds = resolve_period_bounds_at("5h", 0, now, Some(reset)).unwrap();
        assert_eq!(bounds.start, NaiveDate::from_ymd_opt(2026, 8, 14).unwrap());
        assert!(bounds.contains_timestamp(local(2026, 8, 14, 23, 0)));
        assert!(!bounds.contains_timestamp(local(2026, 8, 14, 21, 59)));
    }

    #[test]
    fn five_h_without_reset_is_rolling_now_minus_five_hours() {
        let now = local(2026, 8, 14, 16, 0);
        let bounds = resolve_period_bounds_at("5h", 0, now, None).unwrap();
        assert_eq!(bounds.range_end, now);
        assert_eq!(bounds.range_start, now - Duration::hours(5));
    }

    #[test]
    fn five_h_expired_reset_falls_back_to_rolling() {
        let now = local(2026, 8, 14, 16, 0);
        let reset = local(2026, 8, 14, 15, 0);
        let bounds = resolve_period_bounds_at("5h", 0, now, Some(reset)).unwrap();
        assert_eq!(bounds.range_end, now);
        assert_eq!(bounds.range_start, now - Duration::hours(5));
    }

    #[test]
    fn five_h_offset_is_previous_window() {
        let now = local(2026, 8, 14, 16, 0);
        let reset = local(2026, 8, 14, 18, 0);
        let bounds = resolve_period_bounds_at("5h", -1, now, Some(reset)).unwrap();
        assert_eq!(bounds.range_end, reset - Duration::hours(5));
        assert_eq!(bounds.range_start, reset - Duration::hours(10));
    }

    #[test]
    fn official_reset_reads_claude_five_hour_and_codex_primary() {
        let reset = local(2026, 8, 14, 18, 0);
        let payload = claude_payload(&reset.to_rfc3339());
        assert_eq!(
            official_five_hour_reset("claude", Some(&payload)),
            Some(reset)
        );
        assert_eq!(official_five_hour_reset("all", Some(&payload)), Some(reset));
        assert_eq!(
            official_five_hour_reset("cursor", Some(&payload)),
            Some(reset)
        );
        assert!(official_five_hour_reset("codex", Some(&payload)).is_none());
    }
}
