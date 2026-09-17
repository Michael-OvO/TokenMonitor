//! One place that turns a USD amount into the string the user sees.
//!
//! Costs are USD everywhere inside the app — the archive stores no cost at all
//! and every payload is priced in USD (`usage/pricing.rs`), so conversion is
//! purely a display concern. The main webview does this in
//! `src/lib/utils/format.ts`; this module is its Rust twin, for the two
//! surfaces that TypeScript module cannot reach:
//!
//! - the tray/menu-bar title, which Rust draws itself, and
//! - the floating ball, a separate webview that never loads the settings store.
//!
//! Both files must agree on the symbol table and the rounding rules. The tests
//! below pin the shared behaviour; `format.test.ts` pins the other half.

use std::sync::{OnceLock, RwLock};

/// Mirrors `CURRENCY_SYMBOLS` in `src/lib/utils/format.ts`. Unknown codes fall
/// back to `$`, matching the frontend's `?? "$"`.
const CURRENCY_SYMBOLS: &[(&str, &str)] = &[
    ("USD", "$"),
    ("EUR", "\u{20ac}"),
    ("GBP", "\u{a3}"),
    ("JPY", "\u{a5}"),
    ("CNY", "\u{a5}"),
];

/// The currency the user picked, pushed over from the settings store by the
/// `set_currency` command. Defaults to USD so a first launch — or a frontend
/// that never got around to telling us — reads the same as it always did.
static ACTIVE_CURRENCY: OnceLock<RwLock<String>> = OnceLock::new();

fn currency_cell() -> &'static RwLock<String> {
    ACTIVE_CURRENCY.get_or_init(|| RwLock::new("USD".to_string()))
}

pub fn set_active_currency(code: &str) {
    let normalized = code.trim().to_ascii_uppercase();
    if normalized.is_empty() {
        return;
    }
    if let Ok(mut guard) = currency_cell().write() {
        *guard = normalized;
    }
}

pub fn active_currency() -> String {
    currency_cell()
        .read()
        .map(|g| g.clone())
        .unwrap_or_else(|_| "USD".to_string())
}

pub fn symbol(code: &str) -> &'static str {
    CURRENCY_SYMBOLS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, s)| *s)
        .unwrap_or("$")
}

/// USD is the base unit everywhere inside the app, so it never converts — even
/// if the rate table somehow carries a `USD` key. Mirrors `rateFor` in
/// `format.ts`, including the "no rate yet" fallback of 1: on a cold start the
/// symbol should still be right rather than silently reverting to dollars.
fn rate_for(code: &str) -> f64 {
    if code == "USD" {
        return 1.0;
    }
    crate::usage::exchange_rates::get_rate(code).unwrap_or(1.0)
}

fn convert(usd: f64) -> (f64, &'static str, bool) {
    let code = active_currency();
    let sym = symbol(&code);
    (usd * rate_for(&code), sym, code == "JPY")
}

/// Popover-style amount. `whole` comes from the tray's cost-precision setting;
/// JPY forces it, the same way `formatCost` skips the decimals for yen.
pub fn format(usd: f64, whole: bool) -> String {
    let (converted, sym, is_jpy) = convert(usd);
    if whole || is_jpy {
        format!("{sym}{}", converted.round() as i64)
    } else {
        format!("{sym}{converted:.2}")
    }
}

/// Like [`format`], but drops the decimals when the *converted* amount lands on
/// a whole unit. The decision has to happen after conversion: $70 is a round
/// number, €64.40 is not.
pub fn format_auto(usd: f64) -> String {
    let (converted, _, is_jpy) = convert(usd);
    let is_whole = (converted - converted.round()).abs() < 0.005;
    format(usd, is_whole || is_jpy)
}

/// Float-ball label: a handful of pixels wide, so precision shrinks as the
/// number grows. Thresholds apply to the converted amount — ¥149.5 belongs in
/// the "just round it" bucket even though $1.00 would not.
pub fn format_compact(usd: f64) -> String {
    let (converted, sym, _) = convert(usd);
    if converted <= 0.0 {
        return format!("{sym}0");
    }
    if converted < 1.0 {
        return format!("{sym}{converted:.2}");
    }
    if converted < 10.0 {
        return format!("{sym}{converted:.1}");
    }
    format!("{sym}{}", converted.round() as i64)
}

/// Restores USD when it goes out of scope and serialises every test that
/// touches the currency or rate globals. Both live in process statics, so
/// without this the parallel test runner interleaves them.
#[cfg(test)]
pub struct CurrencyGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl CurrencyGuard {
    pub fn new(code: &str) -> Self {
        static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_active_currency(code);
        Self { _lock: lock }
    }
}

#[cfg(test)]
impl Drop for CurrencyGuard {
    fn drop(&mut self) {
        set_active_currency("USD");
        crate::usage::exchange_rates::set_exchange_rates(std::collections::HashMap::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Currency lives in a process-global, so currency-sensitive tests across
    /// every module must serialise on this and restore USD when they finish.
    /// `CurrencyGuard` does both.
    fn with_currency(code: &str, rates: &[(&str, f64)], body: impl FnOnce()) {
        let _guard = CurrencyGuard::new(code);
        let mut map = HashMap::new();
        for (k, v) in rates {
            map.insert((*k).to_string(), *v);
        }
        crate::usage::exchange_rates::set_exchange_rates(map);
        body();
    }

    #[test]
    fn symbols_mirror_the_frontend_table() {
        // Must match CURRENCY_SYMBOLS in src/lib/utils/format.ts.
        assert_eq!(symbol("USD"), "$");
        assert_eq!(symbol("EUR"), "\u{20ac}");
        assert_eq!(symbol("GBP"), "\u{a3}");
        assert_eq!(symbol("JPY"), "\u{a5}");
        assert_eq!(symbol("CNY"), "\u{a5}");
    }

    #[test]
    fn unknown_currency_falls_back_to_dollar_at_rate_one() {
        with_currency("XYZ", &[("EUR", 0.92)], || {
            assert_eq!(format(10.0, false), "$10.00");
        });
    }

    #[test]
    fn usd_is_never_converted_even_if_a_rate_exists_for_it() {
        with_currency("USD", &[("USD", 1234.0), ("EUR", 0.92)], || {
            assert_eq!(format(10.0, false), "$10.00");
        });
    }

    #[test]
    fn converts_and_stamps_the_selected_symbol() {
        with_currency("EUR", &[("EUR", 0.92)], || {
            assert_eq!(format(5.0, false), "\u{20ac}4.60");
        });
    }

    #[test]
    fn jpy_drops_the_decimals_like_the_frontend_does() {
        with_currency("JPY", &[("JPY", 149.5)], || {
            assert_eq!(format(1.0, false), "\u{a5}150");
        });
    }

    #[test]
    fn whole_precision_rounds_after_converting() {
        with_currency("EUR", &[("EUR", 0.92)], || {
            // 12.345 USD -> 11.3574 EUR -> "€11", not "€12" (round then convert).
            assert_eq!(format(12.345, true), "\u{20ac}11");
        });
    }

    #[test]
    fn missing_rate_for_the_selected_currency_falls_back_to_rate_one() {
        // Cold start: the rate table is empty but the user picked EUR. The
        // symbol must still be right; silently printing "$" would be worse.
        with_currency("EUR", &[], || {
            assert_eq!(format(5.0, false), "\u{20ac}5.00");
        });
    }

    #[test]
    fn format_auto_drops_decimals_only_when_the_converted_amount_is_whole() {
        with_currency("USD", &[], || {
            assert_eq!(format_auto(70.0), "$70");
            assert_eq!(format_auto(70.5), "$70.50");
        });
        // $70 is €64.40 — whole in USD, not whole in EUR, so the cents stay.
        with_currency("EUR", &[("EUR", 0.92)], || {
            assert_eq!(format_auto(70.0), "\u{20ac}64.40");
        });
    }

    #[test]
    fn compact_thresholds_match_the_float_ball_rules() {
        with_currency("USD", &[], || {
            assert_eq!(format_compact(0.0), "$0");
            assert_eq!(format_compact(-1.0), "$0");
            assert_eq!(format_compact(0.42), "$0.42");
            assert_eq!(format_compact(5.44), "$5.4");
            assert_eq!(format_compact(42.4), "$42");
        });
    }

    #[test]
    fn compact_thresholds_apply_to_the_converted_amount() {
        // $1.00 is 149.5 JPY — a "< 10, one decimal" USD amount lands in the
        // "round it" bucket once converted, which is what a JPY user expects.
        with_currency("JPY", &[("JPY", 149.5)], || {
            assert_eq!(format_compact(1.0), "\u{a5}150");
        });
    }

    #[test]
    fn active_currency_round_trips() {
        with_currency("GBP", &[("GBP", 0.79)], || {
            assert_eq!(active_currency(), "GBP");
        });
        // CurrencyGuard restored the default.
        assert_eq!(active_currency(), "USD");
    }

    #[test]
    fn blank_or_garbage_currency_codes_are_ignored() {
        let _guard = CurrencyGuard::new("EUR");
        set_active_currency("   ");
        assert_eq!(active_currency(), "EUR");
        set_active_currency("eur");
        assert_eq!(active_currency(), "EUR", "codes are upper-cased");
    }
}
