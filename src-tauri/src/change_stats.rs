// src-tauri/src/change_stats.rs

use chrono::{DateTime, Local};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FileCategory {
    Code,
    Docs,
    Config,
    Other,
}

pub fn classify_file(path: &str) -> FileCategory {
    let ext = match path.rsplit('.').next() {
        Some(e) => e.to_ascii_lowercase(),
        None => return FileCategory::Other,
    };

    match ext.as_str() {
        // Code
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "go" | "java" | "kt"
        | "scala" | "swift" | "c" | "cc" | "cpp" | "h" | "hpp" | "cs" | "rb" | "php" | "sh"
        | "bash" | "zsh" | "sql" | "html" | "css" | "scss" | "sass" | "svelte" | "vue" => {
            FileCategory::Code
        }

        // Docs
        "md" | "mdx" | "txt" | "rst" | "adoc" | "asciidoc" => FileCategory::Docs,

        // Config
        "json" | "yaml" | "yml" | "toml" | "ini" | "env" | "xml" => FileCategory::Config,

        _ => FileCategory::Other,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeEventKind {
    PatchEdit,
    FullWrite,
}

#[derive(Debug, Clone)]
pub struct ParsedChangeEvent {
    pub timestamp: DateTime<Local>,
    pub model: String,
    pub provider: String,
    pub path: String,
    pub kind: ChangeEventKind,
    pub added_lines: u64,
    pub removed_lines: u64,
    pub category: FileCategory,
    pub dedupe_key: Option<String>,
    pub agent_scope: crate::subagent_stats::AgentScope,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ChangeStats {
    pub added_lines: u64,
    pub removed_lines: u64,
    pub net_lines: i64,
    pub files_touched: u32,
    pub change_events: u32,
    pub write_events: u32,
    pub code_lines_changed: u64,
    pub docs_lines_changed: u64,
    pub config_lines_changed: u64,
    pub other_lines_changed: u64,
    pub avg_lines_per_event: Option<f64>,
    pub cost_per_100_net_lines: Option<f64>,
    pub tokens_per_net_line: Option<f64>,
    pub rewrite_ratio: Option<f64>,
    pub churn_ratio: Option<f64>,
    pub dominant_extension: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ModelChangeSummary {
    pub added_lines: u64,
    pub removed_lines: u64,
    pub net_lines: i64,
    pub files_touched: u32,
    pub change_events: u32,
}

pub fn aggregate_change_stats(
    events: &[ParsedChangeEvent],
    total_cost: f64,
    total_tokens: u64,
) -> Option<ChangeStats> {
    if events.is_empty() {
        return None;
    }

    let mut added: u64 = 0;
    let mut removed: u64 = 0;
    let mut code: u64 = 0;
    let mut docs: u64 = 0;
    let mut config: u64 = 0;
    let mut other: u64 = 0;
    let mut write_events: u32 = 0;
    let mut files = HashSet::new();

    for ev in events {
        added += ev.added_lines;
        removed += ev.removed_lines;
        let changed = ev.added_lines + ev.removed_lines;
        match ev.category {
            FileCategory::Code => code += changed,
            FileCategory::Docs => docs += changed,
            FileCategory::Config => config += changed,
            FileCategory::Other => other += changed,
        }
        if ev.kind == ChangeEventKind::FullWrite {
            write_events += 1;
        }
        files.insert(ev.path.clone());
    }

    let net = added as i64 - removed as i64;
    let change_events = events.len() as u32;
    let total_changed = added + removed;

    let avg_lines_per_event = if change_events > 0 {
        Some(total_changed as f64 / change_events as f64)
    } else {
        None
    };

    let cost_per_100 = if net > 0 {
        Some((total_cost / net as f64) * 100.0)
    } else {
        None
    };

    let tokens_per = if net > 0 {
        Some(total_tokens as f64 / net as f64)
    } else {
        None
    };

    let churn = if added > 0 {
        Some(removed as f64 / added as f64)
    } else {
        None
    };

    Some(ChangeStats {
        added_lines: added,
        removed_lines: removed,
        net_lines: net,
        files_touched: files.len() as u32,
        change_events,
        write_events,
        code_lines_changed: code,
        docs_lines_changed: docs,
        config_lines_changed: config,
        other_lines_changed: other,
        avg_lines_per_event,
        cost_per_100_net_lines: cost_per_100,
        tokens_per_net_line: tokens_per,
        rewrite_ratio: None,
        churn_ratio: churn,
        dominant_extension: None,
    })
}

pub fn aggregate_model_change_summary(
    events: &[ParsedChangeEvent],
    model_key: &str,
) -> Option<ModelChangeSummary> {
    let model_events: Vec<&ParsedChangeEvent> =
        events.iter().filter(|e| e.model == model_key).collect();

    if model_events.is_empty() {
        return None;
    }

    let mut added: u64 = 0;
    let mut removed: u64 = 0;
    let mut files = HashSet::new();

    for ev in &model_events {
        added += ev.added_lines;
        removed += ev.removed_lines;
        files.insert(ev.path.clone());
    }

    Some(ModelChangeSummary {
        added_lines: added,
        removed_lines: removed,
        net_lines: added as i64 - removed as i64,
        files_touched: files.len() as u32,
        change_events: model_events.len() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_rust_file() {
        assert_eq!(classify_file("src/main.rs"), FileCategory::Code);
    }

    #[test]
    fn classify_typescript_file() {
        assert_eq!(classify_file("src/lib/types/index.ts"), FileCategory::Code);
    }

    #[test]
    fn classify_svelte_file() {
        assert_eq!(classify_file("src/App.svelte"), FileCategory::Code);
    }

    #[test]
    fn classify_markdown_file() {
        assert_eq!(classify_file("docs/README.md"), FileCategory::Docs);
    }

    #[test]
    fn classify_json_file() {
        assert_eq!(classify_file("package.json"), FileCategory::Config);
    }

    #[test]
    fn classify_yaml_file() {
        assert_eq!(
            classify_file(".github/workflows/ci.yml"),
            FileCategory::Config
        );
    }

    #[test]
    fn classify_unknown_extension() {
        assert_eq!(classify_file("image.png"), FileCategory::Other);
    }

    #[test]
    fn classify_no_extension() {
        assert_eq!(classify_file("Makefile"), FileCategory::Other);
    }

    #[test]
    fn classify_case_insensitive() {
        assert_eq!(classify_file("README.MD"), FileCategory::Docs);
    }

    // ── Aggregation tests ──

    use chrono::TimeZone;

    fn make_event(path: &str, added: u64, removed: u64, model: &str) -> ParsedChangeEvent {
        ParsedChangeEvent {
            timestamp: Local.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap(),
            model: model.to_string(),
            provider: "claude".to_string(),
            path: path.to_string(),
            kind: ChangeEventKind::PatchEdit,
            added_lines: added,
            removed_lines: removed,
            category: classify_file(path),
            dedupe_key: None,
            agent_scope: crate::subagent_stats::AgentScope::Main,
        }
    }

    #[test]
    fn aggregate_empty_returns_none() {
        assert!(aggregate_change_stats(&[], 0.0, 0).is_none());
    }

    #[test]
    fn aggregate_single_event() {
        let events = vec![make_event("src/main.rs", 10, 3, "opus-4-6")];
        let stats = aggregate_change_stats(&events, 1.0, 1000).unwrap();
        assert_eq!(stats.added_lines, 10);
        assert_eq!(stats.removed_lines, 3);
        assert_eq!(stats.net_lines, 7);
        assert_eq!(stats.files_touched, 1);
        assert_eq!(stats.change_events, 1);
        assert_eq!(stats.code_lines_changed, 13);
        assert_eq!(stats.docs_lines_changed, 0);
    }

    #[test]
    fn aggregate_composition_partitions_all_lines() {
        let events = vec![
            make_event("src/main.rs", 50, 10, "opus-4-6"),
            make_event("README.md", 20, 5, "opus-4-6"),
            make_event("config.yaml", 8, 2, "opus-4-6"),
        ];
        let stats = aggregate_change_stats(&events, 5.0, 10000).unwrap();
        let total = stats.code_lines_changed
            + stats.docs_lines_changed
            + stats.config_lines_changed
            + stats.other_lines_changed;
        assert_eq!(total, stats.added_lines + stats.removed_lines);
    }

    #[test]
    fn aggregate_dedupes_files() {
        let events = vec![
            make_event("src/main.rs", 10, 0, "opus-4-6"),
            make_event("src/main.rs", 5, 2, "opus-4-6"),
        ];
        let stats = aggregate_change_stats(&events, 1.0, 1000).unwrap();
        assert_eq!(stats.files_touched, 1);
        assert_eq!(stats.change_events, 2);
    }

    #[test]
    fn aggregate_negative_net() {
        let events = vec![make_event("src/main.rs", 5, 20, "opus-4-6")];
        let stats = aggregate_change_stats(&events, 1.0, 1000).unwrap();
        assert_eq!(stats.net_lines, -15);
        assert!(stats.cost_per_100_net_lines.is_none());
        assert!(stats.tokens_per_net_line.is_none());
    }

    #[test]
    fn aggregate_efficiency_when_positive_net() {
        let events = vec![make_event("src/main.rs", 100, 0, "opus-4-6")];
        let stats = aggregate_change_stats(&events, 5.0, 50000).unwrap();
        assert!((stats.cost_per_100_net_lines.unwrap() - 5.0).abs() < 0.01);
        assert!((stats.tokens_per_net_line.unwrap() - 500.0).abs() < 0.01);
    }

    #[test]
    fn model_summary_filters_by_model() {
        let events = vec![
            make_event("src/a.rs", 30, 5, "opus-4-6"),
            make_event("src/b.rs", 10, 2, "sonnet-4-6"),
        ];
        let summary = aggregate_model_change_summary(&events, "opus-4-6").unwrap();
        assert_eq!(summary.added_lines, 30);
        assert_eq!(summary.removed_lines, 5);
        assert_eq!(summary.change_events, 1);
    }
}
