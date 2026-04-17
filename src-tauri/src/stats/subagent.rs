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
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_5m_tokens: u64,
    pub cache_write_1h_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeUsageSummary {
    pub cost: f64,
    pub tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_5m_tokens: u64,
    pub cache_write_1h_tokens: u64,
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

#[derive(Default)]
struct ModelAccum {
    cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_5m_tokens: u64,
    cache_write_1h_tokens: u64,
}

// ── Internal builder ────────────────────────────────────────────────────────

struct ScopeSummaryBuilder {
    cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_write_5m_tokens: u64,
    cache_write_1h_tokens: u64,
    cache_read_tokens: u64,
    sessions: HashSet<String>,
    model_stats: HashMap<String, ModelAccum>,
    added_lines: u64,
    removed_lines: u64,
}

impl ScopeSummaryBuilder {
    fn new() -> Self {
        Self {
            cost: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            cache_read_tokens: 0,
            sessions: HashSet::new(),
            model_stats: HashMap::new(),
            added_lines: 0,
            removed_lines: 0,
        }
    }

    fn add_entry(&mut self, entry: &crate::usage::parser::ParsedEntry) {
        let model_key = crate::models::normalized_model_key(&entry.model);
        let entry_cost = crate::usage::pricing::calculate_cost_for_key(
            &model_key,
            entry.input_tokens,
            entry.output_tokens,
            entry.cache_creation_5m_tokens,
            entry.cache_creation_1h_tokens,
            entry.cache_read_tokens,
            entry.web_search_requests,
        );
        self.cost += entry_cost;
        self.input_tokens += entry.input_tokens;
        self.output_tokens += entry.output_tokens;
        self.cache_write_5m_tokens += entry.cache_creation_5m_tokens;
        self.cache_write_1h_tokens += entry.cache_creation_1h_tokens;
        self.cache_read_tokens += entry.cache_read_tokens;

        if !entry.session_key.is_empty() {
            self.sessions.insert(entry.session_key.clone());
        }

        let ma = self.model_stats.entry(entry.model.clone()).or_default();
        ma.cost += entry_cost;
        ma.input_tokens += entry.input_tokens;
        ma.output_tokens += entry.output_tokens;
        ma.cache_read_tokens += entry.cache_read_tokens;
        ma.cache_write_5m_tokens += entry.cache_creation_5m_tokens;
        ma.cache_write_1h_tokens += entry.cache_creation_1h_tokens;
    }

    fn add_change(&mut self, event: &crate::stats::change::ParsedChangeEvent) {
        self.added_lines += event.added_lines;
        self.removed_lines += event.removed_lines;
    }

    fn total_tokens(&self) -> u64 {
        self.input_tokens
            + self.output_tokens
            + self.cache_write_5m_tokens
            + self.cache_write_1h_tokens
            + self.cache_read_tokens
    }

    fn build(self, total_cost: f64) -> ScopeUsageSummary {
        let tokens = self.total_tokens();
        let pct_of_total_cost = if total_cost > 0.0 {
            (self.cost / total_cost) * 100.0
        } else {
            0.0
        };

        // Build top models: sort by cost desc, take top 2
        let mut model_list: Vec<(String, ModelAccum)> = self.model_stats.into_iter().collect();
        model_list.sort_by(|a, b| {
            b.1.cost
                .partial_cmp(&a.1.cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let top_models: Vec<ScopeModelUsage> = model_list
            .into_iter()
            .take(2)
            .map(|(raw_model, accum)| {
                let (display_name, model_key) = crate::models::normalize_model(&raw_model);
                ScopeModelUsage {
                    display_name,
                    model_key,
                    cost: accum.cost,
                    input_tokens: accum.input_tokens,
                    output_tokens: accum.output_tokens,
                    cache_read_tokens: accum.cache_read_tokens,
                    cache_write_5m_tokens: accum.cache_write_5m_tokens,
                    cache_write_1h_tokens: accum.cache_write_1h_tokens,
                }
            })
            .collect();

        ScopeUsageSummary {
            cost: self.cost,
            tokens,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_write_5m_tokens: self.cache_write_5m_tokens,
            cache_write_1h_tokens: self.cache_write_1h_tokens,
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
    entries: &[crate::usage::parser::ParsedEntry],
    change_events: &[crate::stats::change::ParsedChangeEvent],
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
    if sub_builder.cost == 0.0 && sub_builder.total_tokens() == 0 {
        return None;
    }

    Some(SubagentStats {
        main: main_builder.build(total_cost),
        subagents: sub_builder.build(total_cost),
    })
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
    ) -> crate::usage::parser::ParsedEntry {
        crate::usage::parser::ParsedEntry {
            timestamp: Local.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap(),
            model: model.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: 0,
            web_search_requests: 0,
            unique_hash: None,
            session_key: session.to_string(),
            agent_scope: scope,
        }
    }

    fn make_change(
        scope: AgentScope,
        added: u64,
        removed: u64,
    ) -> crate::stats::change::ParsedChangeEvent {
        crate::stats::change::ParsedChangeEvent {
            timestamp: Local.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap(),
            model: "opus-4-6".to_string(),
            provider: "claude".to_string(),
            path: "src/main.rs".to_string(),
            kind: crate::stats::change::ChangeEventKind::PatchEdit,
            added_lines: added,
            removed_lines: removed,
            category: crate::stats::change::FileCategory::Code,
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
