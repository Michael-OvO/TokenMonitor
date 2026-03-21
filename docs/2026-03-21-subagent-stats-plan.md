# Subagent Stats Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Main vs Subagent usage attribution with delegation intensity, per-scope model usage, and per-scope change attribution for both Claude Code and Codex.

**Architecture:** Extend `ParsedEntry` and `ParsedChangeEvent` with `agent_scope` and `session_key` fields. Add a new `subagent_stats.rs` aggregation module. Wire into `commands.rs` payload generation. Render via a new `SubagentList.svelte` dense two-card component.

**Tech Stack:** Rust (Tauri backend), Svelte 5 (runes), TypeScript, Vitest

**Spec:** `docs/2026-03-21-subagent-stats-spec.md`

---

## File Map

### New files

| File | Responsibility |
|---|---|
| `src-tauri/src/subagent_stats.rs` | `AgentScope` enum, `SubagentStats`/`ScopeUsageSummary`/`ScopeModelUsage` structs, `aggregate_subagent_stats`, `merge_subagent_stats` |
| `src/lib/components/SubagentList.svelte` | Dense two-card UI: proportion bar, cost/tokens, top models, change lines |

### Modified files

| File | What changes |
|---|---|
| `src-tauri/src/lib.rs` | Add `mod subagent_stats;` |
| `src-tauri/src/change_stats.rs` | Add `agent_scope: AgentScope` field to `ParsedChangeEvent` |
| `src-tauri/src/parser.rs` | Add `session_key` + `agent_scope` to `ParsedEntry`; parse `isSidechain`/`sessionId`/`agentId` for Claude; parse `session_meta` for Codex; update dedupe hash |
| `src-tauri/src/models.rs` | Add `subagent_stats: Option<SubagentStats>` to `UsagePayload` |
| `src-tauri/src/commands.rs` | Wire `aggregate_subagent_stats` into `get_provider_data` and `merge_payloads` |
| `src/lib/types/index.ts` | Add `SubagentStats`, `ScopeUsageSummary`, `ScopeModelUsage`; update `UsagePayload` |
| `src/lib/stores/usage.ts` | Add `subagent_stats: null` to `emptyPayload()` |
| `src/App.svelte` | Import and render `SubagentList` |

---

### Task 1: Add `AgentScope` enum and extend `ParsedEntry`

**Files:**
- Create: `src-tauri/src/subagent_stats.rs`
- Modify: `src-tauri/src/lib.rs:1-2`
- Modify: `src-tauri/src/parser.rs:20-30` (`ParsedEntry` struct)
- Modify: `src-tauri/src/change_stats.rs:45-55` (`ParsedChangeEvent` struct)

- [ ] **Step 1: Create `subagent_stats.rs` with `AgentScope` enum**

```rust
// src-tauri/src/subagent_stats.rs

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentScope {
    #[default]
    Main,
    Subagent,
}
```

- [ ] **Step 2: Register the module in `lib.rs`**

In `src-tauri/src/lib.rs`, add after the `mod change_stats;` line:

```rust
mod subagent_stats;
```

- [ ] **Step 3: Add `session_key` and `agent_scope` to `ParsedEntry`**

In `src-tauri/src/parser.rs`, update the `ParsedEntry` struct (line 21-30) to:

```rust
#[derive(Clone)]
pub struct ParsedEntry {
    pub timestamp: DateTime<Local>,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_5m_tokens: u64,
    pub cache_creation_1h_tokens: u64,
    pub cache_read_tokens: u64,
    pub unique_hash: Option<String>,
    pub session_key: String,
    pub agent_scope: crate::subagent_stats::AgentScope,
}
```

- [ ] **Step 4: Add `agent_scope` to `ParsedChangeEvent`**

In `src-tauri/src/change_stats.rs`, update `ParsedChangeEvent` (line 46-55). Add after the `dedupe_key` field:

```rust
    pub agent_scope: crate::subagent_stats::AgentScope,
```

- [ ] **Step 5: Fix all compilation errors**

Every place that constructs a `ParsedEntry` or `ParsedChangeEvent` needs the new fields. Add defaults:
- `session_key: String::new()` (will be populated properly in later tasks)
- `agent_scope: AgentScope::Main` (default for backwards compatibility)

Search for all `ParsedEntry {` and `ParsedChangeEvent {` constructors in `parser.rs`, `commands.rs`, and test files. Add the two new fields to each.

Run: `cd src-tauri && cargo build 2>&1 | head -50`

Fix every error until `cargo build` succeeds.

- [ ] **Step 6: Run all tests to verify no regressions**

Run: `cd src-tauri && cargo test 2>&1 | tail -20`
Expected: All existing tests pass (170+). Top-line totals unchanged.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/subagent_stats.rs src-tauri/src/lib.rs src-tauri/src/parser.rs src-tauri/src/change_stats.rs src-tauri/src/commands.rs
git commit -m "feat(subagent-stats): add AgentScope enum and extend ParsedEntry/ParsedChangeEvent"
```

---

### Task 2: Parse Claude subagent metadata

**Files:**
- Modify: `src-tauri/src/parser.rs:102-118` (`ClaudeJsonlEntry` struct)
- Modify: `src-tauri/src/parser.rs:335-339` (`create_claude_unique_hash`)
- Modify: `src-tauri/src/parser.rs:610-719` (Claude parse loop)

- [ ] **Step 1: Write failing tests for Claude scope attribution**

Add to the `tests` module in `src-tauri/src/parser.rs`:

```rust
#[test]
fn claude_root_session_defaults_to_main_scope() {
    let dir = TempDir::new().unwrap();
    let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","sessionId":"sess-1","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
    write_file(&dir.path().join("session.jsonl"), content);

    let entries = read_claude_entries(dir.path(), None);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].agent_scope, crate::subagent_stats::AgentScope::Main);
    assert!(entries[0].session_key.contains("main"), "session_key should contain 'main', got: {}", entries[0].session_key);
}

#[test]
fn claude_sidechain_entry_maps_to_subagent_scope() {
    let dir = TempDir::new().unwrap();
    let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","isSidechain":true,"agentId":"a1b2c3d","sessionId":"sess-1","message":{"model":"claude-haiku-4-5","stop_reason":"end_turn","usage":{"input_tokens":50,"output_tokens":20}}}"#;
    write_file(&dir.path().join("session.jsonl"), content);

    let entries = read_claude_entries(dir.path(), None);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].agent_scope, crate::subagent_stats::AgentScope::Subagent);
    assert!(entries[0].session_key.contains("a1b2c3d"), "session_key should contain agentId, got: {}", entries[0].session_key);
}

#[test]
fn claude_dedupe_does_not_collapse_root_and_sidechain() {
    let dir = TempDir::new().unwrap();
    // Root and sidechain with same message.id and requestId
    let root = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","sessionId":"sess-1","requestId":"req-1","message":{"id":"msg-1","model":"claude-opus-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
    let sidechain = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:01+00:00","isSidechain":true,"agentId":"agt-1","sessionId":"sess-1","requestId":"req-1","message":{"id":"msg-1","model":"claude-haiku-4-5","stop_reason":"end_turn","usage":{"input_tokens":30,"output_tokens":10}}}"#;
    write_file(&dir.path().join("root.jsonl"), root);
    write_file(&dir.path().join("sidechain.jsonl"), sidechain);

    let entries = read_claude_entries(dir.path(), None);
    assert_eq!(entries.len(), 2, "root and sidechain should both survive dedupe");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test claude_root_session claude_sidechain claude_dedupe_does_not 2>&1 | tail -20`
Expected: Tests fail (session_key is empty, agent_scope is always Main, dedupe may collapse entries)

- [ ] **Step 3: Add Claude metadata fields to `ClaudeJsonlEntry`**

In `src-tauri/src/parser.rs`, update `ClaudeJsonlEntry` (line 102-118). Add these fields before the closing `}`:

```rust
    #[serde(rename = "isSidechain", default)]
    is_sidechain: Option<bool>,
    #[serde(rename = "sessionId", default)]
    session_id: Option<String>,
    #[serde(rename = "agentId", default)]
    agent_id: Option<String>,
```

- [ ] **Step 4: Update `create_claude_unique_hash` to prevent root/sidechain collision**

In `src-tauri/src/parser.rs`, update `create_claude_unique_hash` (line 335-339):

```rust
fn create_claude_unique_hash(entry: &ClaudeJsonlEntry) -> Option<String> {
    let message_id = entry.message.as_ref()?.id.as_ref()?;
    let request_id = entry.request_id.as_ref()?;
    let sidechain = if entry.is_sidechain == Some(true) { "1" } else { "0" };
    let agent = entry.agent_id.as_deref().unwrap_or("");

    Some(format!("{sidechain}:{agent}:{message_id}:{request_id}"))
}
```

- [ ] **Step 5: Compute `session_key` and `agent_scope` in the Claude parse loop**

In `src-tauri/src/parser.rs`, in the `parse_claude_session_file` function, before the `for line in reader.lines()` loop, add a helper closure or compute file-level defaults. Then in the `"assistant"` branch (around line 624-719), when constructing `ParsedEntry` (line 710-719), derive the scope:

```rust
let agent_scope = if entry.is_sidechain == Some(true) {
    crate::subagent_stats::AgentScope::Subagent
} else {
    crate::subagent_stats::AgentScope::Main
};
let session_key = match (&entry.session_id, &entry.agent_id, entry.is_sidechain) {
    (Some(sid), Some(aid), Some(true)) => format!("claude:{sid}:subagent:{aid}"),
    (Some(sid), _, _) => format!("claude:{sid}:main"),
    _ => format!("claude:file:{}", path_to_string(path)),
};
```

Add these two fields to the `ParsedEntry` constructor at line 710:

```rust
entries.push(ParsedEntry {
    // ...existing fields...
    session_key,
    agent_scope,
});
```

Also propagate `agent_scope` to any `ParsedChangeEvent` constructed from this entry's tool_use blocks. The `PendingClaudeTool` struct (line 342) needs an `agent_scope` field, and the change event emission code must pass it through.

- [ ] **Step 6: Run the tests**

Run: `cd src-tauri && cargo test claude_root_session claude_sidechain claude_dedupe_does_not 2>&1 | tail -20`
Expected: All 3 new tests pass.

- [ ] **Step 7: Run full test suite for regressions**

Run: `cd src-tauri && cargo test 2>&1 | tail -5`
Expected: All tests pass (170+).

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/parser.rs
git commit -m "feat(subagent-stats): parse Claude isSidechain/sessionId/agentId for scope attribution"
```

---

### Task 3: Parse Codex subagent metadata

**Files:**
- Modify: `src-tauri/src/parser.rs:736-922` (`parse_codex_session_file`)

- [ ] **Step 1: Write failing tests for Codex scope attribution**

Add to the `tests` module in `src-tauri/src/parser.rs`:

```rust
#[test]
fn codex_no_session_meta_defaults_to_main() {
    let dir = TempDir::new().unwrap();
    let ts = Local::now().format("%Y-%m-%dT12:00:00+00:00").to_string();
    let content = format!(
        r#"{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50}}}}}}}}"#
    );
    write_file(&dir.path().join("session.jsonl"), &content);

    let (entries, _, _, _) = parse_codex_session_file(&dir.path().join("session.jsonl"));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].agent_scope, crate::subagent_stats::AgentScope::Main);
}

#[test]
fn codex_session_meta_with_subagent_other_maps_to_subagent() {
    let dir = TempDir::new().unwrap();
    let ts = Local::now().format("%Y-%m-%dT12:00:00+00:00").to_string();
    let content = format!(
        r#"{{"type":"session_meta","payload":{{"id":"sess-abc","source":{{"subagent":{{"other":"guardian"}}}}}}}}
{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50}}}}}}}}"#
    );
    write_file(&dir.path().join("session.jsonl"), &content);

    let (entries, _, _, _) = parse_codex_session_file(&dir.path().join("session.jsonl"));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].agent_scope, crate::subagent_stats::AgentScope::Subagent);
    assert_eq!(entries[0].session_key, "codex:sess-abc");
}

#[test]
fn codex_session_meta_with_thread_spawn_maps_to_subagent() {
    let dir = TempDir::new().unwrap();
    let ts = Local::now().format("%Y-%m-%dT12:00:00+00:00").to_string();
    let content = format!(
        r#"{{"type":"session_meta","payload":{{"id":"sess-xyz","source":{{"subagent":{{"thread_spawn":{{"parent_thread_id":"parent-1","depth":1}}}}}}}}}}
{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":200,"output_tokens":80}}}}}}}}"#
    );
    write_file(&dir.path().join("session.jsonl"), &content);

    let (entries, _, _, _) = parse_codex_session_file(&dir.path().join("session.jsonl"));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].agent_scope, crate::subagent_stats::AgentScope::Subagent);
    assert_eq!(entries[0].session_key, "codex:sess-xyz");
}

#[test]
fn codex_all_entries_in_file_share_same_session_key() {
    let dir = TempDir::new().unwrap();
    let ts1 = Local::now().format("%Y-%m-%dT12:00:00+00:00").to_string();
    let ts2 = Local::now().format("%Y-%m-%dT12:05:00+00:00").to_string();
    let content = format!(
        r#"{{"type":"session_meta","payload":{{"id":"sess-shared"}}}}
{{"type":"turn_context","payload":{{"cwd":"/tmp","model":"gpt-5.4"}}}}
{{"type":"event_msg","timestamp":"{ts1}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"output_tokens":50}}}}}}}}
{{"type":"event_msg","timestamp":"{ts2}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":200,"output_tokens":80}}}}}}}}"#
    );
    write_file(&dir.path().join("session.jsonl"), &content);

    let (entries, _, _, _) = parse_codex_session_file(&dir.path().join("session.jsonl"));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].session_key, entries[1].session_key);
    assert_eq!(entries[0].session_key, "codex:sess-shared");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test codex_no_session_meta codex_session_meta_with_subagent codex_session_meta_with_thread codex_all_entries_in_file 2>&1 | tail -20`
Expected: Tests fail (session_key empty, agent_scope always Main)

- [ ] **Step 3: Add `session_meta` parsing to `parse_codex_session_file`**

In `src-tauri/src/parser.rs`, in `parse_codex_session_file` (starts at line 736), after the file is opened and before the main `for line in reader.lines()` loop, add file-level state:

```rust
let mut session_key = format!("codex-file:{}", path_to_string(path));
let mut agent_scope = crate::subagent_stats::AgentScope::Main;
```

In the main loop, add a `session_meta` handler. Currently the loop checks for `turn_context` (line 760) and then `event_msg` / `response_item` (line 771). Add before the `turn_context` check:

```rust
if entry.entry_type == "session_meta" {
    if let Some(payload) = entry.payload.as_ref() {
        if let Some(id) = payload.get("id").and_then(Value::as_str) {
            session_key = format!("codex:{id}");
        }
        if payload.pointer("/source/subagent").is_some() {
            agent_scope = crate::subagent_stats::AgentScope::Subagent;
        }
    }
    continue;
}
```

Then update all `ParsedEntry` constructors in this function (around line 908-918) to include:

```rust
session_key: session_key.clone(),
agent_scope,
```

And all `ParsedChangeEvent` constructors to include:

```rust
agent_scope,
```

- [ ] **Step 4: Run the tests**

Run: `cd src-tauri && cargo test codex_no_session_meta codex_session_meta_with_subagent codex_session_meta_with_thread codex_all_entries_in_file 2>&1 | tail -20`
Expected: All 4 new tests pass.

- [ ] **Step 5: Run full test suite**

Run: `cd src-tauri && cargo test 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/parser.rs
git commit -m "feat(subagent-stats): parse Codex session_meta for subagent scope attribution"
```

---

### Task 4: Add aggregation module

**Files:**
- Modify: `src-tauri/src/subagent_stats.rs`

- [ ] **Step 1: Write failing tests for aggregation**

Add to `src-tauri/src/subagent_stats.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    fn make_entry(scope: AgentScope, session: &str, model: &str, input: u64, output: u64) -> crate::parser::ParsedEntry {
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

    fn make_change(scope: AgentScope, added: u64, removed: u64) -> crate::change_stats::ParsedChangeEvent {
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
        let entries = vec![
            make_entry(AgentScope::Main, "s1", "claude-opus-4-6", 100, 50),
        ];
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
        assert_eq!(stats.subagents.session_count, 2, "two distinct subagent session_keys");
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test subagent_stats 2>&1 | tail -20`
Expected: Fail — `aggregate_subagent_stats` not defined

- [ ] **Step 3: Implement `aggregate_subagent_stats`**

Add to `src-tauri/src/subagent_stats.rs`, above the `#[cfg(test)]` block:

```rust
use std::collections::{HashMap, HashSet};

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
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub session_count: u32,
    pub pct_of_total_cost: Option<f64>,
    pub top_models: Vec<ScopeModelUsage>,
    pub added_lines: u64,
    pub removed_lines: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubagentStats {
    pub main: ScopeUsageSummary,
    pub subagents: ScopeUsageSummary,
}

pub fn aggregate_subagent_stats(
    entries: &[crate::parser::ParsedEntry],
    change_events: &[crate::change_stats::ParsedChangeEvent],
    total_cost: f64,
) -> Option<SubagentStats> {
    let mut main_summary = ScopeSummaryBuilder::default();
    let mut sub_summary = ScopeSummaryBuilder::default();

    for entry in entries {
        let builder = match entry.agent_scope {
            AgentScope::Main => &mut main_summary,
            AgentScope::Subagent => &mut sub_summary,
        };
        let cost = crate::pricing::calculate_cost(
            &entry.model,
            entry.input_tokens,
            entry.output_tokens,
            entry.cache_creation_5m_tokens,
            entry.cache_creation_1h_tokens,
            entry.cache_read_tokens,
        );
        builder.cost += cost;
        builder.input_tokens += entry.input_tokens;
        builder.output_tokens += entry.output_tokens;
        builder.cache_read_tokens += entry.cache_read_tokens;
        builder.cache_write_tokens += entry.cache_creation_5m_tokens + entry.cache_creation_1h_tokens;
        builder.sessions.insert(entry.session_key.clone());

        let model_entry = builder.models.entry(entry.model.clone()).or_insert((String::new(), 0.0));
        if model_entry.0.is_empty() {
            let (display_name, _) = if crate::models::is_codex_model_name(&entry.model) {
                let (d, _) = crate::models::normalize_codex_model(&entry.model);
                (d, String::new())
            } else {
                let (d, k) = crate::models::normalize_claude_model(&entry.model);
                (d.to_string(), k.to_string())
            };
            model_entry.0 = display_name;
        }
        model_entry.1 += cost;
    }

    for cev in change_events {
        let builder = match cev.agent_scope {
            AgentScope::Main => &mut main_summary,
            AgentScope::Subagent => &mut sub_summary,
        };
        builder.added_lines += cev.added_lines;
        builder.removed_lines += cev.removed_lines;
    }

    // Return None if no subagent usage
    let sub_tokens = sub_summary.input_tokens + sub_summary.output_tokens
        + sub_summary.cache_read_tokens + sub_summary.cache_write_tokens;
    if sub_summary.cost == 0.0 && sub_tokens == 0 {
        return None;
    }

    Some(SubagentStats {
        main: main_summary.build(total_cost),
        subagents: sub_summary.build(total_cost),
    })
}

#[derive(Default)]
struct ScopeSummaryBuilder {
    cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    sessions: HashSet<String>,
    models: HashMap<String, (String, f64)>, // raw_model -> (display_name, cost)
    added_lines: u64,
    removed_lines: u64,
}

impl ScopeSummaryBuilder {
    fn build(self, total_cost: f64) -> ScopeUsageSummary {
        let tokens = self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens;
        let pct = if total_cost > 0.0 {
            Some(self.cost / total_cost * 100.0)
        } else {
            None
        };

        let mut model_vec: Vec<ScopeModelUsage> = self.models.into_iter().map(|(raw, (display, cost))| {
            let key = if crate::models::is_codex_model_name(&raw) {
                crate::models::normalize_codex_model(&raw).1
            } else {
                crate::models::normalize_claude_model(&raw).1.to_string()
            };
            ScopeModelUsage { display_name: display, model_key: key, cost }
        }).collect();
        model_vec.sort_by(|a, b| b.cost.partial_cmp(&a.cost).unwrap_or(std::cmp::Ordering::Equal));
        model_vec.truncate(2);

        ScopeUsageSummary {
            cost: self.cost,
            tokens,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens,
            session_count: self.sessions.len() as u32,
            pct_of_total_cost: pct,
            top_models: model_vec,
            added_lines: self.added_lines,
            removed_lines: self.removed_lines,
        }
    }
}

pub fn merge_subagent_stats(
    a: Option<SubagentStats>,
    b: Option<SubagentStats>,
    merged_total_cost: f64,
) -> Option<SubagentStats> {
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(recompute_pct(a, merged_total_cost)),
        (None, Some(b)) => Some(recompute_pct(b, merged_total_cost)),
        (Some(a), Some(b)) => {
            let main = merge_scope_summaries(a.main, b.main, merged_total_cost);
            let subagents = merge_scope_summaries(a.subagents, b.subagents, merged_total_cost);
            if subagents.cost == 0.0 && subagents.tokens == 0 {
                None
            } else {
                Some(SubagentStats { main, subagents })
            }
        }
    }
}

fn recompute_pct(mut stats: SubagentStats, total_cost: f64) -> SubagentStats {
    let pct = |cost: f64| if total_cost > 0.0 { Some(cost / total_cost * 100.0) } else { None };
    stats.main.pct_of_total_cost = pct(stats.main.cost);
    stats.subagents.pct_of_total_cost = pct(stats.subagents.cost);
    stats
}

fn merge_scope_summaries(a: ScopeUsageSummary, b: ScopeUsageSummary, total_cost: f64) -> ScopeUsageSummary {
    let cost = a.cost + b.cost;
    let pct = if total_cost > 0.0 { Some(cost / total_cost * 100.0) } else { None };

    let mut models: HashMap<String, ScopeModelUsage> = HashMap::new();
    for m in a.top_models.into_iter().chain(b.top_models) {
        let entry = models.entry(m.model_key.clone()).or_insert(ScopeModelUsage {
            display_name: m.display_name.clone(),
            model_key: m.model_key.clone(),
            cost: 0.0,
        });
        entry.cost += m.cost;
    }
    let mut model_vec: Vec<ScopeModelUsage> = models.into_values().collect();
    model_vec.sort_by(|a, b| b.cost.partial_cmp(&a.cost).unwrap_or(std::cmp::Ordering::Equal));
    model_vec.truncate(2);

    ScopeUsageSummary {
        cost,
        tokens: a.tokens + b.tokens,
        input_tokens: a.input_tokens + b.input_tokens,
        output_tokens: a.output_tokens + b.output_tokens,
        cache_read_tokens: a.cache_read_tokens + b.cache_read_tokens,
        cache_write_tokens: a.cache_write_tokens + b.cache_write_tokens,
        session_count: a.session_count + b.session_count,
        pct_of_total_cost: pct,
        top_models: model_vec,
        added_lines: a.added_lines + b.added_lines,
        removed_lines: a.removed_lines + b.removed_lines,
    }
}
```

- [ ] **Step 3b: Make `is_codex_model_name` visible to sibling modules**

In `src-tauri/src/models.rs`, change `fn is_codex_model_name` (line 158) to `pub(crate) fn is_codex_model_name`. This is needed because `subagent_stats.rs` uses it to determine display name formatting.

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test subagent_stats 2>&1 | tail -20`
Expected: All 5 aggregation tests pass.

- [ ] **Step 5: Run full test suite**

Run: `cd src-tauri && cargo test 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/subagent_stats.rs src-tauri/src/models.rs
git commit -m "feat(subagent-stats): add aggregation module with scope splitting and merge"
```

---

### Task 5: Wire aggregation into commands

**Files:**
- Modify: `src-tauri/src/models.rs:8-23` (`UsagePayload`)
- Modify: `src-tauri/src/commands.rs:867-886` (`get_provider_data`)
- Modify: `src-tauri/src/commands.rs:637-648` (merge for "all" tab)
- Modify: `src-tauri/src/commands.rs:889+` (`merge_payloads`)

- [ ] **Step 1: Add `subagent_stats` to `UsagePayload`**

In `src-tauri/src/models.rs`, add to the `UsagePayload` struct (after `change_stats`):

```rust
    pub subagent_stats: Option<crate::subagent_stats::SubagentStats>,
```

Fix all `UsagePayload` constructors in `commands.rs` and test helpers to include `subagent_stats: None`.

- [ ] **Step 2: Wire aggregation into `get_provider_data`**

In `src-tauri/src/commands.rs`, after the change stats wiring (around line 877), add:

```rust
    // Attach subagent stats
    let period_entries = load_entries_for_period(parser, provider, period, offset);
    payload.subagent_stats = crate::subagent_stats::aggregate_subagent_stats(
        &period_entries,
        &change_events,
        payload.total_cost,
    );
```

Note: `load_entries_for_period` already exists at line 722 and returns date-bounded entries.

- [ ] **Step 3: Wire merge into "all" provider path**

In `src-tauri/src/commands.rs`, in the "all" branch (around line 637-648), after the change stats re-aggregation, add:

```rust
            // Merge subagent stats
            merged.subagent_stats = crate::subagent_stats::merge_subagent_stats(
                claude.subagent_stats,
                codex.subagent_stats,
                merged.total_cost,
            );
```

- [ ] **Step 4: Build and test**

Run: `cd src-tauri && cargo test 2>&1 | tail -5`
Expected: All tests pass. The `compare_all_with_ccusage` regression guard still passes (totals unchanged).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/commands.rs
git commit -m "feat(subagent-stats): wire aggregation into provider data and all-tab merge"
```

---

### Task 6: Add TypeScript types and update frontend stores

**Files:**
- Modify: `src/lib/types/index.ts`
- Modify: `src/lib/stores/usage.ts`
- Modify: `src/lib/stores/usage.test.ts`

- [ ] **Step 1: Add TypeScript interfaces**

In `src/lib/types/index.ts`, add before the `CalendarDay` interface:

```typescript
export interface ScopeModelUsage {
  display_name: string;
  model_key: string;
  cost: number;
}

export interface ScopeUsageSummary {
  cost: number;
  tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  session_count: number;
  pct_of_total_cost: number | null;
  top_models: ScopeModelUsage[];
  added_lines: number;
  removed_lines: number;
}

export interface SubagentStats {
  main: ScopeUsageSummary;
  subagents: ScopeUsageSummary;
}
```

Update `UsagePayload` — add after `change_stats`:

```typescript
  subagent_stats: SubagentStats | null;
```

- [ ] **Step 2: Update `emptyPayload()` in `usage.ts`**

In `src/lib/stores/usage.ts`, in the `emptyPayload()` function, add after `change_stats: null`:

```typescript
    subagent_stats: null,
```

- [ ] **Step 3: Update test helpers**

In `src/lib/stores/usage.test.ts`, in `makePayload()`, add after `change_stats: null`:

```typescript
    subagent_stats: null,
```

- [ ] **Step 4: Run svelte-check and tests**

Run: `npx svelte-check 2>&1 | tail -5`
Expected: 0 errors

Run: `npm test 2>&1 | tail -10`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/lib/types/index.ts src/lib/stores/usage.ts src/lib/stores/usage.test.ts
git commit -m "feat(subagent-stats): add TypeScript types and update frontend stores"
```

---

### Task 7: Create `SubagentList.svelte` component

**Files:**
- Create: `src/lib/components/SubagentList.svelte`
- Modify: `src/App.svelte`
- Modify: `src/app.css` (scope color tokens)

- [ ] **Step 1: Add scope color tokens to `app.css`**

Find the existing CSS custom property definitions in `src/app.css`. Add two new tokens alongside the existing `--ch-plus`/`--ch-minus` or composition tokens:

```css
  --scope-main: #6b8afd;
  --scope-sub: #b07aff;
```

Add them in both light and dark theme sections (and glass theme if it exists).

- [ ] **Step 2: Create the `SubagentList.svelte` component**

Create `src/lib/components/SubagentList.svelte`:

```svelte
<script lang="ts">
  import { modelColor, formatCost, formatTokens } from "../utils/format.js";
  import type { SubagentStats } from "../types/index.js";

  interface Props { stats: SubagentStats }
  let { stats }: Props = $props();

  let mainPct = $derived(
    stats.main.cost + stats.subagents.cost > 0
      ? (stats.main.cost / (stats.main.cost + stats.subagents.cost)) * 100
      : 100
  );
</script>

<div class="sa">
  <div class="sa-head">
    <span class="sa-title">Agent Breakdown</span>
  </div>

  <!-- Proportion bar -->
  <div class="sa-bar">
    <div class="sa-bar-main" style="width:{mainPct}%"></div>
    <div class="sa-bar-sub" style="width:{100 - mainPct}%"></div>
  </div>

  <!-- Two cards -->
  <div class="sa-cards">
    <!-- Main card -->
    <div class="sa-card">
      <div class="sa-card-head">
        <span class="sa-dot" style="background:var(--scope-main)"></span>
        <span class="sa-label">Main</span>
      </div>
      <div class="sa-val">{formatCost(stats.main.cost)}</div>
      <div class="sa-sub">
        {formatTokens(stats.main.tokens)} tokens
        {#if stats.main.pct_of_total_cost != null}· {stats.main.pct_of_total_cost.toFixed(0)}%{/if}
      </div>
      {#if stats.main.top_models.length > 0}
        <div class="sa-models">
          {#each stats.main.top_models as m}
            <div class="sa-model-row">
              <span class="sa-model-dot" style="background:{modelColor(m.model_key)}"></span>
              <span class="sa-model-name">{m.display_name}</span>
              <span class="sa-model-cost">{formatCost(m.cost)}</span>
            </div>
          {/each}
        </div>
      {/if}
      {#if stats.main.added_lines > 0 || stats.main.removed_lines > 0}
        <div class="sa-changes">
          <span class="ch-plus">+{stats.main.added_lines.toLocaleString()}</span>
          <span class="ch-slash"> / </span>
          <span class="ch-minus">&minus;{stats.main.removed_lines.toLocaleString()}</span>
          <span class="sa-changes-label"> lines</span>
        </div>
      {/if}
    </div>

    <!-- Subagents card -->
    <div class="sa-card">
      <div class="sa-card-head">
        <span class="sa-dot" style="background:var(--scope-sub)"></span>
        <span class="sa-label">Subagents</span>
      </div>
      <div class="sa-val">{formatCost(stats.subagents.cost)}</div>
      <div class="sa-sub">
        {formatTokens(stats.subagents.tokens)}
        {#if stats.subagents.pct_of_total_cost != null}
          · <span class="sa-pct">{stats.subagents.pct_of_total_cost.toFixed(0)}%</span>
        {/if}
        · {stats.subagents.session_count} spawned
      </div>
      {#if stats.subagents.top_models.length > 0}
        <div class="sa-models">
          {#each stats.subagents.top_models as m}
            <div class="sa-model-row">
              <span class="sa-model-dot" style="background:{modelColor(m.model_key)}"></span>
              <span class="sa-model-name">{m.display_name}</span>
              <span class="sa-model-cost">{formatCost(m.cost)}</span>
            </div>
          {/each}
        </div>
      {/if}
      {#if stats.subagents.added_lines > 0 || stats.subagents.removed_lines > 0}
        <div class="sa-changes">
          <span class="ch-plus">+{stats.subagents.added_lines.toLocaleString()}</span>
          <span class="ch-slash"> / </span>
          <span class="ch-minus">&minus;{stats.subagents.removed_lines.toLocaleString()}</span>
          <span class="sa-changes-label"> lines</span>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .sa { padding: 10px 12px; animation: fadeUp .28s ease both .07s; }
  .sa-head {
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3); text-transform: uppercase;
    letter-spacing: .7px; margin-bottom: 8px;
  }
  .sa-title { }

  .sa-bar {
    display: flex; height: 6px; border-radius: 3px;
    overflow: hidden; margin-bottom: 8px;
  }
  .sa-bar-main { background: var(--scope-main); }
  .sa-bar-sub { background: var(--scope-sub); }

  .sa-cards { display: flex; gap: 4px; }
  .sa-card {
    flex: 1; min-width: 0;
    background: var(--surface-2); border-radius: 7px;
    padding: 8px 9px; transition: background .18s;
  }
  .sa-card:hover { background: var(--surface-hover); }

  .sa-card-head {
    display: flex; align-items: center; gap: 5px; margin-bottom: 5px;
  }
  .sa-dot { width: 5px; height: 5px; border-radius: 50%; flex-shrink: 0; }
  .sa-label {
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3); text-transform: uppercase; letter-spacing: .5px;
  }
  .sa-val {
    font: 400 13px/1 'Inter', sans-serif;
    color: var(--t1); letter-spacing: -.2px;
    font-variant-numeric: tabular-nums;
  }
  .sa-sub {
    font: 400 8px/1 'Inter', sans-serif;
    color: var(--t4); margin-top: 3px;
  }
  .sa-pct { color: var(--scope-sub); }

  .sa-models {
    margin-top: 6px; padding-top: 5px;
    border-top: 1px solid var(--surface-2);
  }
  .sa-model-row {
    display: flex; align-items: center; gap: 4px; margin-bottom: 2px;
    font: 400 7.5px/1 'Inter', sans-serif;
  }
  .sa-model-dot { width: 3px; height: 3px; border-radius: 50%; flex-shrink: 0; }
  .sa-model-name { color: var(--t4); flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sa-model-cost { color: var(--t3); flex-shrink: 0; }

  .sa-changes {
    margin-top: 5px;
    font: 400 7.5px/1 'Inter', sans-serif; color: var(--t4);
  }
  .ch-plus { color: var(--ch-plus); }
  .ch-minus { color: var(--ch-minus); }
  .ch-slash { color: var(--t4); }
  .sa-changes-label { color: var(--t4); }
</style>
```

- [ ] **Step 3: Wire into `App.svelte`**

In `src/App.svelte`, add the import alongside the other component imports:

```typescript
import SubagentList from "./lib/components/SubagentList.svelte";
```

Add the rendering after the ModelList block (after line 640, before `<Footer>`):

```svelte
      {#if data.subagent_stats}
        <div class="hr"></div>
        <SubagentList stats={data.subagent_stats} />
      {/if}
```

- [ ] **Step 4: Run svelte-check**

Run: `npx svelte-check 2>&1 | tail -5`
Expected: 0 errors

- [ ] **Step 5: Run all tests**

Run: `npm test 2>&1 | tail -10`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib/components/SubagentList.svelte src/App.svelte src/app.css
git commit -m "feat(subagent-stats): add SubagentList component and wire into App"
```

---

### Task 8: Frontend component tests

**Files:**
- Create: `src/lib/components/SubagentList.test.ts`

- [ ] **Step 1: Create SubagentList tests**

Create `src/lib/components/SubagentList.test.ts`. Follow the existing test patterns in the project (check any existing `*.test.ts` files under `src/lib/` for the mounting pattern — the project uses Vitest with Svelte 5):

```typescript
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/svelte";
import SubagentList from "./SubagentList.svelte";
import type { SubagentStats, ScopeUsageSummary } from "../types/index.js";

function makeScopeSummary(overrides: Partial<ScopeUsageSummary> = {}): ScopeUsageSummary {
  return {
    cost: 0,
    tokens: 0,
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    session_count: 0,
    pct_of_total_cost: null,
    top_models: [],
    added_lines: 0,
    removed_lines: 0,
    ...overrides,
  };
}

function makeStats(overrides: { main?: Partial<ScopeUsageSummary>; subagents?: Partial<ScopeUsageSummary> } = {}): SubagentStats {
  return {
    main: makeScopeSummary(overrides.main),
    subagents: makeScopeSummary(overrides.subagents),
  };
}

describe("SubagentList", () => {
  it("renders both cards when subagent usage exists", () => {
    const stats = makeStats({
      main: { cost: 18.42, tokens: 512000, pct_of_total_cost: 79 },
      subagents: { cost: 4.91, tokens: 141000, pct_of_total_cost: 21, session_count: 8 },
    });
    const { container } = render(SubagentList, { props: { stats } });
    const cards = container.querySelectorAll(".sa-card");
    expect(cards.length).toBe(2);
    expect(container.textContent).toContain("Main");
    expect(container.textContent).toContain("Subagents");
  });

  it("shows percentage badge only on subagent card", () => {
    const stats = makeStats({
      main: { cost: 18.42, tokens: 512000, pct_of_total_cost: 79 },
      subagents: { cost: 4.91, tokens: 141000, pct_of_total_cost: 21, session_count: 3 },
    });
    const { container } = render(SubagentList, { props: { stats } });
    const pctBadges = container.querySelectorAll(".sa-pct");
    expect(pctBadges.length).toBe(1);
    expect(pctBadges[0].textContent).toContain("21");
  });

  it("shows spawn count on subagent card", () => {
    const stats = makeStats({
      main: { cost: 10, tokens: 100000 },
      subagents: { cost: 5, tokens: 50000, session_count: 12 },
    });
    const { container } = render(SubagentList, { props: { stats } });
    expect(container.textContent).toContain("12 spawned");
  });
});
```

Note: If `@testing-library/svelte` is not installed, check the project's existing test setup. The project may use a different rendering approach — adapt accordingly.

- [ ] **Step 2: Run the tests**

Run: `npm test 2>&1 | tail -15`
Expected: All tests pass including the 3 new SubagentList tests.

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/SubagentList.test.ts
git commit -m "test(subagent-stats): add SubagentList component tests"
```

---

### Task 9: Final verification and regression check

**Files:** None (verification only)

- [ ] **Step 1: Run full Rust test suite**

Run: `cd src-tauri && cargo test 2>&1 | tail -10`
Expected: All tests pass (180+). No regressions.

- [ ] **Step 2: Run clippy**

Run: `cd src-tauri && cargo clippy -- -D warnings 2>&1 | tail -5`
Expected: No warnings.

- [ ] **Step 3: Run svelte-check**

Run: `npx svelte-check 2>&1 | tail -5`
Expected: 0 errors, 0 warnings.

- [ ] **Step 4: Run frontend tests**

Run: `npm test 2>&1 | tail -10`
Expected: All tests pass.

- [ ] **Step 5: Run the pre-commit hook manually**

Run: `.git/hooks/pre-commit 2>&1 | tail -5`
Expected: "All checks passed."

- [ ] **Step 6: Verify with dev server (manual)**

Run: `npm run tauri dev`

Check:
1. Open the app → navigate to a period with subagent usage
2. The "Agent Breakdown" section should appear below Models
3. Proportion bar, two cards with cost/tokens/models/changes
4. Switch to "All" tab → merged stats
5. Navigate to 5h view → section visible independently
6. Navigate to a period with no subagent data → section hidden
