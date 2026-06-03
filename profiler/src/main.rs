//! Verify the has_entries_before fix: time it cold (after clear_cache, must
//! scan all files) and warm (cached → O(1)), against the real claude logs.

#[path = "../../src-tauri/src/models.rs"]
mod models;
#[path = "../../src-tauri/src/paths.rs"]
mod paths;
#[path = "../../src-tauri/src/stats/mod.rs"]
mod stats;
mod secrets;
#[path = "commands_mod.rs"]
mod commands;
#[path = "usage_mod.rs"]
mod usage;

use std::time::Instant;

use chrono::NaiveDate;

use crate::usage::parser::UsageParser;

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() {
    let parser = UsageParser::new();
    // "year 2026" view asks: is there any claude data before 2026-01-01?
    let before = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

    for provider in ["claude", "all"] {
        for run in 1..=3 {
            parser.clear_cache(); // cold: forces full recompute of earliest date
            let t = Instant::now();
            let cold_result = parser.has_entries_before(provider, before);
            let cold = ms(t.elapsed());

            let t = Instant::now();
            let warm_result = parser.has_entries_before(provider, before);
            let warm = ms(t.elapsed());

            eprintln!(
                "[{provider}] run {run}: cold={cold:8.1}ms (result={cold_result})  warm={warm:7.3}ms (result={warm_result})"
            );
        }
    }
}
