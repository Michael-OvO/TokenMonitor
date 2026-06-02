use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::period::resolve_period_bounds;
use crate::commands::usage_query::get_usage_data_inner;
use crate::commands::AppState;
use crate::usage::integrations::all_usage_integrations;

static WARMUP_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarmupProgress {
    pub current: u32,
    pub total: u32,
    pub provider: String,
    pub period: String,
    pub offset: i32,
}

const PERIODS: &[&str] = &["5h", "day", "week", "month", "year"];

fn max_negative_offset(state: &AppState, provider: &str, period: &str) -> i32 {
    if period == "5h" {
        return 0;
    }

    let mut offset = -1i32;
    while let Ok(bounds) = resolve_period_bounds(period, offset) {
        if !state.parser.has_entries_before(provider, bounds.start) {
            break;
        }
        offset -= 1;
        if offset < -500 {
            break;
        }
    }
    offset + 1
}

fn build_warmup_keys(
    state: &AppState,
    priority_provider: &str,
    priority_period: &str,
) -> Vec<(String, String, i32)> {
    let providers: Vec<String> = {
        let mut p: Vec<String> = all_usage_integrations()
            .iter()
            .filter(|id| id.detect_roots().iter().any(|r| r.exists()))
            .map(|id| id.as_str().to_string())
            .collect();
        p.push("all".to_string());
        p
    };

    let mut keys: Vec<(String, String, i32)> = Vec::new();

    // P1: current view
    keys.push((
        priority_provider.to_string(),
        priority_period.to_string(),
        0,
    ));

    // P2: same provider, other periods, offset=0
    for period in PERIODS {
        if *period != priority_period {
            keys.push((priority_provider.to_string(), period.to_string(), 0));
        }
    }

    // P3: other providers, all periods, offset=0
    for provider in &providers {
        if provider != priority_provider {
            for period in PERIODS {
                keys.push((provider.clone(), period.to_string(), 0));
            }
        }
    }

    // P4: all historical offsets
    for provider in &providers {
        for period in PERIODS {
            let min_offset = max_negative_offset(state, provider, period);
            for offset in (min_offset..0).rev() {
                keys.push((provider.clone(), period.to_string(), offset));
            }
        }
    }

    let mut seen = HashSet::new();
    keys.retain(|k| seen.insert(k.clone()));
    keys
}

pub async fn warmup_payloads(
    app: &AppHandle,
    priority_provider: &str,
    priority_period: &str,
    emit_progress: bool,
) -> u32 {
    if WARMUP_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return 0;
    }

    let state = app.state::<AppState>();
    let keys = build_warmup_keys(&state, priority_provider, priority_period);
    let total = keys.len() as u32;
    let mut completed = 0u32;

    tracing::info!(
        total_keys = total,
        priority_provider,
        priority_period,
        "Cache warmup started"
    );

    // Phase 1: offset=0 keys — parallel across providers
    let offset_zero: Vec<_> = keys.iter().filter(|(_, _, o)| *o == 0).cloned().collect();
    let historical: Vec<_> = keys.iter().filter(|(_, _, o)| *o != 0).cloned().collect();

    // Group offset=0 by provider for parallel execution
    let mut providers_seen = HashSet::new();
    let provider_order: Vec<String> = offset_zero
        .iter()
        .filter(|(p, _, _)| providers_seen.insert(p.clone()))
        .map(|(p, _, _)| p.clone())
        .collect();

    let handles: Vec<_> = provider_order
        .iter()
        .map(|provider| {
            let app_clone = app.clone();
            let provider = provider.clone();
            let provider_keys: Vec<_> = offset_zero
                .iter()
                .filter(|(p, _, _)| *p == provider)
                .cloned()
                .collect();
            tokio::spawn(async move {
                let state = app_clone.state::<AppState>();
                let mut count = 0u32;
                for (prov, period, offset) in &provider_keys {
                    let _ =
                        get_usage_data_inner(Some(&app_clone), &state, prov, period, *offset).await;
                    state.parser.clear_entries_cache();
                    count += 1;
                }
                count
            })
        })
        .collect();

    for handle in handles {
        if let Ok(count) = handle.await {
            completed += count;
            if emit_progress {
                let _ = app.emit(
                    "cache://progress",
                    WarmupProgress {
                        current: completed,
                        total,
                        provider: String::new(),
                        period: String::new(),
                        offset: 0,
                    },
                );
            }
        }
    }

    // Phase 2: historical offsets — serial
    for (provider, period, offset) in &historical {
        if !WARMUP_RUNNING.load(Ordering::SeqCst) {
            tracing::info!("Cache warmup cancelled");
            break;
        }
        let _ = get_usage_data_inner(Some(app), &state, provider, period, *offset).await;
        state.parser.clear_entries_cache();
        completed += 1;

        if emit_progress {
            let _ = app.emit(
                "cache://progress",
                WarmupProgress {
                    current: completed,
                    total,
                    provider: provider.clone(),
                    period: period.clone(),
                    offset: *offset,
                },
            );
        }
    }

    let _ = app.emit("cache://done", completed);
    WARMUP_RUNNING.store(false, Ordering::SeqCst);
    tracing::info!(completed, total, "Cache warmup finished");
    completed
}

pub fn cancel_warmup() {
    WARMUP_RUNNING.store(false, Ordering::SeqCst);
}

pub fn is_warmup_running() -> bool {
    WARMUP_RUNNING.load(Ordering::SeqCst)
}
