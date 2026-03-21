// src-tauri/src/subagent_stats.rs

use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentScope {
    #[default]
    Main,
    Subagent,
}

// ── Public types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ScopeModelUsage {
    pub display_name: String,
    pub model_key: String,
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeUsageSummary {
    pub cost: f64,
    pub tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
    pub session_count: u32,
    pub pct_of_total_cost: f64,
    pub top_models: Vec<ScopeModelUsage>,
    pub added_lines: u64,
    pub removed_lines: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubagentStats {
    pub main: ScopeUsageSummary,
    pub subagents: ScopeUsageSummary,
}

// ── Internal builder ────────────────────────────────────────────────────────

struct ScopeSummaryBuilder {
    cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_write_tokens: u64,
    cache_read_tokens: u64,
    sessions: HashSet<String>,
    model_costs: HashMap<String, f64>,
    added_lines: u64,
    removed_lines: u64,
}

impl ScopeSummaryBuilder {
    fn new() -> Self {
        Self {
            cost: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
            sessions: HashSet::new(),
            model_costs: HashMap::new(),
            added_lines: 0,
            removed_lines: 0,
        }
    }

    fn add_entry(&mut self, entry: &crate::parser::ParsedEntry) {
        let entry_cost = crate::pricing::calculate_cost(
            &entry.model,
            entry.input_tokens,
            entry.output_tokens,
            entry.cache_creation_5m_tokens,
            entry.cache_creation_1h_tokens,
            entry.cache_read_tokens,
        );
        self.cost += entry_cost;
        self.input_tokens += entry.input_tokens;
        self.output_tokens += entry.output_tokens;
        self.cache_write_tokens += entry.cache_creation_5m_tokens + entry.cache_creation_1h_tokens;
        self.cache_read_tokens += entry.cache_read_tokens;

        if !entry.session_key.is_empty() {
            self.sessions.insert(entry.session_key.clone());
        }

        *self.model_costs.entry(entry.model.clone()).or_insert(0.0) += entry_cost;
    }

    fn add_change(&mut self, event: &crate::change_stats::ParsedChangeEvent) {
        self.added_lines += event.added_lines;
        self.removed_lines += event.removed_lines;
    }

    fn build(self, total_cost: f64) -> ScopeUsageSummary {
        let tokens = self.input_tokens
            + self.output_tokens
            + self.cache_write_tokens
            + self.cache_read_tokens;
        let pct_of_total_cost = if total_cost > 0.0 {
            self.cost / total_cost
        } else {
            0.0
        };

        // Build top models: sort by cost desc, take top 2
        let mut model_list: Vec<(String, f64)> = self.model_costs.into_iter().collect();
        model_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let top_models: Vec<ScopeModelUsage> = model_list
            .into_iter()
            .take(2)
            .map(|(raw_model, cost)| {
                let (display_name, model_key) = if crate::models::is_codex_model_name(&raw_model) {
                    crate::models::normalize_codex_model(&raw_model)
                } else {
                    let (d, k) = crate::models::normalize_claude_model(&raw_model);
                    (d.to_string(), k.to_string())
                };
                ScopeModelUsage {
                    display_name,
                    model_key,
                    cost,
                }
            })
            .collect();

        ScopeUsageSummary {
            cost: self.cost,
            tokens,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_write_tokens: self.cache_write_tokens,
            cache_read_tokens: self.cache_read_tokens,
            session_count: self.sessions.len() as u32,
            pct_of_total_cost,
            top_models,
            added_lines: self.added_lines,
            removed_lines: self.removed_lines,
        }
    }
}

// ── Public functions ────────────────────────────────────────────────────────

pub fn aggregate_subagent_stats(
    entries: &[crate::parser::ParsedEntry],
    change_events: &[crate::change_stats::ParsedChangeEvent],
    total_cost: f64,
) -> Option<SubagentStats> {
    let mut main_builder = ScopeSummaryBuilder::new();
    let mut sub_builder = ScopeSummaryBuilder::new();

    for entry in entries {
        match entry.agent_scope {
            AgentScope::Main => main_builder.add_entry(entry),
            AgentScope::Subagent => sub_builder.add_entry(entry),
        }
    }

    for event in change_events {
        match event.agent_scope {
            AgentScope::Main => main_builder.add_change(event),
            AgentScope::Subagent => sub_builder.add_change(event),
        }
    }

    // Return None if subagent scope has zero cost AND zero tokens
    let sub_tokens = sub_builder.input_tokens
        + sub_builder.output_tokens
        + sub_builder.cache_write_tokens
        + sub_builder.cache_read_tokens;
    if sub_builder.cost == 0.0 && sub_tokens == 0 {
        return None;
    }

    Some(SubagentStats {
        main: main_builder.build(total_cost),
        subagents: sub_builder.build(total_cost),
    })
}

pub fn merge_subagent_stats(
    a: Option<SubagentStats>,
    b: Option<SubagentStats>,
    merged_total_cost: f64,
) -> Option<SubagentStats> {
    match (a, b) {
        (None, None) => None,
        (Some(only), None) | (None, Some(only)) => {
            // Recompute pct_of_total_cost against the new total
            Some(SubagentStats {
                main: recompute_pct(only.main, merged_total_cost),
                subagents: recompute_pct(only.subagents, merged_total_cost),
            })
        }
        (Some(a), Some(b)) => {
            let main = merge_summaries(a.main, b.main, merged_total_cost);
            let subagents = merge_summaries(a.subagents, b.subagents, merged_total_cost);

            // Return None if merged subagent has zero cost+tokens
            if subagents.cost == 0.0 && subagents.tokens == 0 {
                return None;
            }

            Some(SubagentStats { main, subagents })
        }
    }
}

fn recompute_pct(mut summary: ScopeUsageSummary, total_cost: f64) -> ScopeUsageSummary {
    summary.pct_of_total_cost = if total_cost > 0.0 {
        summary.cost / total_cost
    } else {
        0.0
    };
    summary
}

fn merge_summaries(
    a: ScopeUsageSummary,
    b: ScopeUsageSummary,
    total_cost: f64,
) -> ScopeUsageSummary {
    let cost = a.cost + b.cost;
    let input_tokens = a.input_tokens + b.input_tokens;
    let output_tokens = a.output_tokens + b.output_tokens;
    let cache_write_tokens = a.cache_write_tokens + b.cache_write_tokens;
    let cache_read_tokens = a.cache_read_tokens + b.cache_read_tokens;
    let tokens = input_tokens + output_tokens + cache_write_tokens + cache_read_tokens;
    let session_count = a.session_count + b.session_count;
    let added_lines = a.added_lines + b.added_lines;
    let removed_lines = a.removed_lines + b.removed_lines;
    let pct_of_total_cost = if total_cost > 0.0 {
        cost / total_cost
    } else {
        0.0
    };

    // Merge top_models by model_key, re-sort, take top 2
    let mut model_map: HashMap<String, ScopeModelUsage> = HashMap::new();
    for m in a.top_models.into_iter().chain(b.top_models.into_iter()) {
        let entry = model_map
            .entry(m.model_key.clone())
            .or_insert_with(|| ScopeModelUsage {
                display_name: m.display_name.clone(),
                model_key: m.model_key.clone(),
                cost: 0.0,
            });
        entry.cost += m.cost;
    }
    let mut merged_models: Vec<ScopeModelUsage> = model_map.into_values().collect();
    merged_models.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    merged_models.truncate(2);

    ScopeUsageSummary {
        cost,
        tokens,
        input_tokens,
        output_tokens,
        cache_write_tokens,
        cache_read_tokens,
        session_count,
        pct_of_total_cost,
        top_models: merged_models,
        added_lines,
        removed_lines,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    fn make_entry(
        scope: AgentScope,
        session: &str,
        model: &str,
        input: u64,
        output: u64,
    ) -> crate::parser::ParsedEntry {
        crate::parser::ParsedEntry {
            timestamp: Local.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap(),
            model: model.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: 0,
            unique_hash: None,
            session_key: session.to_string(),
            agent_scope: scope,
        }
    }

    fn make_change(
        scope: AgentScope,
        added: u64,
        removed: u64,
    ) -> crate::change_stats::ParsedChangeEvent {
        crate::change_stats::ParsedChangeEvent {
            timestamp: Local.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap(),
            model: "opus-4-6".to_string(),
            provider: "claude".to_string(),
            path: "src/main.rs".to_string(),
            kind: crate::change_stats::ChangeEventKind::PatchEdit,
            added_lines: added,
            removed_lines: removed,
            category: crate::change_stats::FileCategory::Code,
            dedupe_key: None,
            agent_scope: scope,
        }
    }

    #[test]
    fn all_main_returns_none() {
        let entries = vec![make_entry(
            AgentScope::Main,
            "s1",
            "claude-opus-4-6",
            100,
            50,
        )];
        assert!(aggregate_subagent_stats(&entries, &[], 1.0).is_none());
    }

    #[test]
    fn subagent_with_zero_cost_and_tokens_returns_none() {
        let entries = vec![
            make_entry(AgentScope::Main, "s1", "claude-opus-4-6", 100, 50),
            make_entry(AgentScope::Subagent, "s2", "claude-haiku-4-5", 0, 0),
        ];
        assert!(aggregate_subagent_stats(&entries, &[], 1.0).is_none());
    }

    #[test]
    fn mixed_scopes_split_correctly() {
        let entries = vec![
            make_entry(AgentScope::Main, "s1", "claude-opus-4-6", 1000, 500),
            make_entry(AgentScope::Subagent, "s2", "claude-haiku-4-5", 200, 100),
            make_entry(AgentScope::Subagent, "s3", "claude-haiku-4-5", 300, 150),
        ];
        let stats = aggregate_subagent_stats(&entries, &[], 5.0).unwrap();
        assert_eq!(stats.main.input_tokens, 1000);
        assert_eq!(stats.main.output_tokens, 500);
        assert_eq!(stats.subagents.input_tokens, 500);
        assert_eq!(stats.subagents.output_tokens, 250);
        assert_eq!(
            stats.subagents.session_count, 2,
            "two distinct subagent session_keys"
        );
        assert_eq!(stats.main.session_count, 1);
    }

    #[test]
    fn top_models_capped_at_two() {
        let entries = vec![
            make_entry(AgentScope::Main, "s1", "claude-opus-4-6", 1000, 500),
            make_entry(AgentScope::Main, "s1", "claude-sonnet-4-6", 500, 200),
            make_entry(AgentScope::Main, "s1", "claude-haiku-4-5", 100, 50),
            // Need at least one subagent entry so aggregate returns Some
            make_entry(AgentScope::Subagent, "s2", "claude-haiku-4-5", 10, 5),
        ];
        let stats = aggregate_subagent_stats(&entries, &[], 5.0).unwrap();
        assert!(stats.main.top_models.len() <= 2);
        // Opus should be first (highest cost)
        assert!(stats.main.top_models[0].cost >= stats.main.top_models[1].cost);
    }

    #[test]
    fn change_events_partitioned_by_scope() {
        let entries = vec![
            make_entry(AgentScope::Main, "s1", "claude-opus-4-6", 100, 50),
            make_entry(AgentScope::Subagent, "s2", "claude-haiku-4-5", 50, 20),
        ];
        let changes = vec![
            make_change(AgentScope::Main, 100, 30),
            make_change(AgentScope::Subagent, 40, 10),
        ];
        let stats = aggregate_subagent_stats(&entries, &changes, 1.0).unwrap();
        assert_eq!(stats.main.added_lines, 100);
        assert_eq!(stats.main.removed_lines, 30);
        assert_eq!(stats.subagents.added_lines, 40);
        assert_eq!(stats.subagents.removed_lines, 10);
    }
}
