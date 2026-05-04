//! Rolling-window aggregation over JSONL parser entries.
//!
//! Replaces the `/usage_report` API call with a local computation: count
//! tokens that landed in the trailing 5 hours and 7 days, then express each
//! as a percentage of the user's plan budget. The plan budget is read from
//! settings; defaults are rough approximations of Anthropic's published
//! plans, but the user can override them in Settings → Claude plan.
//!
//! Reset-time math intentionally tracks Anthropic's behavior: a 5h window
//! starts at the *first* message in the trailing 5h band and resets exactly
//! 5h after that message. We never observe the server-side window from
//! here, so this is the closest local approximation.

use chrono::{DateTime, Duration, Utc};

use crate::usage::parser::ParsedEntry;

/// Plan-tier budget in tokens. The variants line up with the
/// `claudePlanTier` setting on the frontend; `Custom` lets the user paste
/// their own numbers when the defaults don't match their account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClaudePlanTier {
    Free,
    #[default]
    Pro,
    Max5x,
    Max20x,
    Custom {
        five_hour_tokens: u64,
        weekly_tokens: u64,
    },
}

impl ClaudePlanTier {
    /// Best-effort 5-hour budget in tokens. These constants are deliberate
    /// underestimates derived from anecdotal Anthropic plan limits — better
    /// to surface "100% used" early than to never warn the user.
    pub fn five_hour_budget_tokens(&self) -> u64 {
        match self {
            Self::Free => 50_000,
            Self::Pro => 200_000,
            Self::Max5x => 1_000_000,
            Self::Max20x => 4_000_000,
            Self::Custom {
                five_hour_tokens, ..
            } => *five_hour_tokens,
        }
    }

    /// Best-effort weekly budget in tokens (7 days).
    pub fn weekly_budget_tokens(&self) -> u64 {
        match self {
            Self::Free => 1_000_000,
            Self::Pro => 7_000_000,
            Self::Max5x => 35_000_000,
            Self::Max20x => 140_000_000,
            Self::Custom { weekly_tokens, .. } => *weekly_tokens,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Pro => "Pro",
            Self::Max5x => "Max 5x",
            Self::Max20x => "Max 20x",
            Self::Custom { .. } => "Custom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "free" | "Free" => Some(Self::Free),
            "pro" | "Pro" => Some(Self::Pro),
            "max5x" | "Max5x" | "Max 5x" => Some(Self::Max5x),
            "max20x" | "Max20x" | "Max 20x" => Some(Self::Max20x),
            _ => None,
        }
    }
}

/// One rolling window summary used by both the 5h and weekly views.
///
/// `used_tokens`, `budget_tokens`, and `anchor_at` are kept on the struct so
/// future tray/tooltip code can show the absolute numbers behind the
/// percentage; the rate-limit consumer currently only reads
/// `utilization_pct` and `resets_at`.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct WindowSummary {
    /// Total billable tokens that landed in the trailing window.
    pub used_tokens: u64,
    /// Plan budget the percentage was computed against.
    pub budget_tokens: u64,
    /// Utilization as a 0–100 percentage. Capped at a sane upper bound so
    /// runaway counts don't blow out the bar UI.
    pub utilization_pct: f64,
    /// Anchor: the first parser entry in the window. Used to compute the
    /// reset time. `None` when the window had no traffic.
    pub anchor_at: Option<DateTime<Utc>>,
    /// When the window resets — i.e. the time at which `anchor_at` falls off
    /// the trailing-window edge. `None` when there's no anchor.
    pub resets_at: Option<DateTime<Utc>>,
}

impl WindowSummary {
    fn empty(budget_tokens: u64) -> Self {
        Self {
            used_tokens: 0,
            budget_tokens,
            utilization_pct: 0.0,
            anchor_at: None,
            resets_at: None,
        }
    }
}

/// Sum the billable tokens in `entries` that fall inside the trailing
/// `window` ending at `now`. Cache-read tokens count at 0.1× and cache-write
/// at the higher tiers — we only care about user-visible budget impact, so
/// stick with the same definition the dashboard uses for "tokens spent".
fn tokens_in_window(
    entries: &[ParsedEntry],
    window: Duration,
    now: DateTime<Utc>,
) -> (u64, Option<DateTime<Utc>>) {
    let cutoff = now - window;
    let mut total: u64 = 0;
    let mut earliest: Option<DateTime<Utc>> = None;

    for entry in entries {
        let ts = entry.timestamp.with_timezone(&Utc);
        if ts < cutoff || ts > now {
            continue;
        }
        // Sum only input + output + cache-writes against the budget. Cache
        // *reads* are intentionally excluded: they're the dominant token
        // type in any CC session (a single prompt commonly reads 300k+
        // cached tokens) but they're 0.1× cost and don't count toward the
        // user-visible plan limit. Including them would inflate the 5h
        // utilization to 999% within minutes of a real session — the bug
        // that motivated this branch's overhaul.
        total = total
            .saturating_add(entry.input_tokens)
            .saturating_add(entry.output_tokens)
            .saturating_add(entry.cache_creation_5m_tokens)
            .saturating_add(entry.cache_creation_1h_tokens);
        earliest = Some(match earliest {
            Some(prev) if prev <= ts => prev,
            _ => ts,
        });
    }

    (total, earliest)
}

fn summarise(
    entries: &[ParsedEntry],
    window: Duration,
    budget: u64,
    now: DateTime<Utc>,
) -> WindowSummary {
    let (used, anchor) = tokens_in_window(entries, window, now);
    if anchor.is_none() {
        return WindowSummary::empty(budget);
    }
    let pct = if budget == 0 {
        0.0
    } else {
        ((used as f64 / budget as f64) * 100.0).clamp(0.0, 999.9)
    };
    WindowSummary {
        used_tokens: used,
        budget_tokens: budget,
        utilization_pct: pct,
        anchor_at: anchor,
        resets_at: anchor.map(|a| a + window),
    }
}

/// Compute both the 5-hour and weekly window summaries against the plan
/// budgets. Always returns both summaries — empty ones when the band has no
/// traffic, so the UI has consistent shape.
pub fn compute(
    entries: &[ParsedEntry],
    plan: ClaudePlanTier,
    now: DateTime<Utc>,
) -> (WindowSummary, WindowSummary) {
    let five_hour = summarise(
        entries,
        Duration::hours(5),
        plan.five_hour_budget_tokens(),
        now,
    );
    let weekly = summarise(entries, Duration::days(7), plan.weekly_budget_tokens(), now);
    (five_hour, weekly)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::subagent::AgentScope;
    use chrono::Local;

    fn entry(when: chrono::DateTime<Local>, tokens: u64) -> ParsedEntry {
        ParsedEntry {
            timestamp: when,
            model: "claude-sonnet-4-6".into(),
            input_tokens: tokens / 2,
            output_tokens: tokens - tokens / 2,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: 0,
            web_search_requests: 0,
            unique_hash: None,
            session_key: "s1".into(),
            agent_scope: AgentScope::Main,
        }
    }

    #[test]
    fn plan_parse_round_trip() {
        assert_eq!(ClaudePlanTier::parse("Pro"), Some(ClaudePlanTier::Pro));
        assert_eq!(ClaudePlanTier::parse("Max 5x"), Some(ClaudePlanTier::Max5x));
        assert_eq!(ClaudePlanTier::parse("nonsense"), None);
    }

    #[test]
    fn empty_window_has_zero_utilization() {
        let now = Utc::now();
        let (five_hour, weekly) = compute(&[], ClaudePlanTier::Pro, now);
        assert_eq!(five_hour.utilization_pct, 0.0);
        assert_eq!(weekly.utilization_pct, 0.0);
        assert!(five_hour.anchor_at.is_none());
    }

    #[test]
    fn within_5h_counted_outside_ignored() {
        let now: DateTime<Utc> = "2026-04-29T12:00:00Z".parse().unwrap();
        let recent = (now - Duration::minutes(30)).with_timezone(&Local);
        let stale = (now - Duration::hours(10)).with_timezone(&Local);

        let entries = vec![entry(recent, 10_000), entry(stale, 100_000)];
        let (five_hour, weekly) = compute(&entries, ClaudePlanTier::Pro, now);

        assert_eq!(five_hour.used_tokens, 10_000);
        assert_eq!(weekly.used_tokens, 110_000);
    }

    #[test]
    fn resets_at_anchored_to_first_in_window() {
        let now: DateTime<Utc> = "2026-04-29T12:00:00Z".parse().unwrap();
        let early = (now - Duration::hours(4)).with_timezone(&Local);
        let later = (now - Duration::minutes(30)).with_timezone(&Local);
        let entries = vec![entry(later, 1_000), entry(early, 1_000)];
        let (five_hour, _) = compute(&entries, ClaudePlanTier::Pro, now);
        assert_eq!(five_hour.anchor_at, Some(early.with_timezone(&Utc)));
        assert_eq!(
            five_hour.resets_at,
            Some(early.with_timezone(&Utc) + Duration::hours(5))
        );
    }

    #[test]
    fn utilization_caps_at_999() {
        let now: DateTime<Utc> = "2026-04-29T12:00:00Z".parse().unwrap();
        let recent = (now - Duration::minutes(1)).with_timezone(&Local);
        // Way over budget on purpose.
        let entries = vec![entry(recent, 1_000_000_000)];
        let (five_hour, _) = compute(&entries, ClaudePlanTier::Pro, now);
        assert!(five_hour.utilization_pct <= 999.9);
    }
}
