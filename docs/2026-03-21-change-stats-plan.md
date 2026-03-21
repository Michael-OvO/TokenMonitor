# Change Statistics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add model-attributed change statistics (lines added/removed, file composition, efficiency metrics) to TokenMonitor, surfaced in a new 2×2 MetricsRow grid.

**Architecture:** Rust backend parses structured edit events from Claude and Codex JSONL logs during the existing single-pass file scan. Change events are aggregated alongside usage data and sent to the frontend as a `ChangeStats` object nested in `UsagePayload`. The Svelte frontend renders a 2×2 card grid replacing the current 3-card MetricsRow.

**Tech Stack:** Rust (Tauri 2, serde, chrono), Svelte 5 (runes), TypeScript, Vitest, CSS custom properties

**Spec:** `docs/2026-03-21-change-stats-ui-design.md` and `docs/design-change-stats.md`

---

## File Map

### New Files
| File | Responsibility |
|------|---------------|
| `src-tauri/src/change_stats.rs` | File classification, `ParsedChangeEvent`, `ChangeStats`/`ModelChangeSummary` structs, aggregation logic |

### Modified Files
| File | What Changes |
|------|-------------|
| `src-tauri/src/lib.rs` | Add `mod change_stats;` |
| `src-tauri/src/models.rs` | Add `change_stats` field to `UsagePayload` and `ModelSummary` |
| `src-tauri/src/parser.rs` | Emit `ParsedChangeEvent`s during Claude/Codex file parsing, extend `CachedFileEntries` |
| `src-tauri/src/commands.rs` | Aggregate change events into `ChangeStats`, attach to payload |
| `src/lib/types/index.ts` | Add `ChangeStats`, `ModelChangeSummary` interfaces, update `UsagePayload`, `ModelSummary` |
| `src/lib/components/MetricsRow.svelte` | 2×2 grid, Changes card, Composition card |
| `src/lib/components/ModelList.svelte` | Optional per-model change stats |
| `src/lib/components/Settings.svelte` | Add `showModelChangeStats` toggle |
| `src/lib/stores/settings.ts` | Add `showModelChangeStats` default |
| `src/app.css` | Composition color tokens |

---

## Task 1: File Classification Module (Rust)

**Files:**
- Create: `src-tauri/src/change_stats.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create `change_stats.rs` with `FileCategory` enum and `classify_file` function**

```rust
// src-tauri/src/change_stats.rs

use serde::Serialize;

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
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs"
        | "py" | "go" | "java" | "kt" | "scala" | "swift"
        | "c" | "cc" | "cpp" | "h" | "hpp" | "cs"
        | "rb" | "php" | "sh" | "bash" | "zsh" | "sql"
        | "html" | "css" | "scss" | "sass" | "svelte" | "vue" => FileCategory::Code,

        // Docs
        "md" | "mdx" | "txt" | "rst" | "adoc" | "asciidoc" => FileCategory::Docs,

        // Config
        "json" | "yaml" | "yml" | "toml" | "ini" | "env" | "xml" => FileCategory::Config,

        _ => FileCategory::Other,
    }
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
        assert_eq!(classify_file(".github/workflows/ci.yml"), FileCategory::Config);
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
}
```

- [ ] **Step 2: Add `mod change_stats;` to `lib.rs`**

Open `src-tauri/src/lib.rs` and add:

```rust
mod change_stats;
```

alongside the existing `mod parser;`, `mod models;`, etc.

- [ ] **Step 3: Run tests to verify**

Run: `cd src-tauri && cargo test change_stats`
Expected: All 9 classification tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/change_stats.rs src-tauri/src/lib.rs
git commit -m "feat(change-stats): add file classification module"
```

---

## Task 2: Change Event Data Structures (Rust)

**Files:**
- Modify: `src-tauri/src/change_stats.rs`
- Modify: `src-tauri/src/models.rs`

- [ ] **Step 1: Add `ParsedChangeEvent` and serializable stats structs to `change_stats.rs`**

Append to `src-tauri/src/change_stats.rs` (above the `#[cfg(test)]` block):

```rust
use chrono::{DateTime, Local};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeEventKind {
    PatchEdit,
    FullWrite,
}

#[derive(Debug, Clone)]
pub struct ParsedChangeEvent {
    pub timestamp: DateTime<Local>,
    pub model: String,       // MUST be the normalized model_key (e.g. "opus-4-6"), NOT the raw model string
    pub provider: String,
    pub path: String,
    pub kind: ChangeEventKind,
    pub added_lines: u64,
    pub removed_lines: u64,
    pub category: FileCategory,
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
```

- [ ] **Step 2: Add `change_stats` field to `UsagePayload` in `models.rs`**

In `src-tauri/src/models.rs`, add `use crate::change_stats::ChangeStats;` at the top, then add the field:

```rust
pub struct UsagePayload {
    // ... existing fields ...
    pub change_stats: Option<ChangeStats>,
}
```

- [ ] **Step 3: Add `change_stats` field to `ModelSummary` in `models.rs`**

```rust
pub struct ModelSummary {
    pub display_name: String,
    pub model_key: String,
    pub cost: f64,
    pub tokens: u64,
    pub change_stats: Option<crate::change_stats::ModelChangeSummary>,
}
```

- [ ] **Step 4: Run `cargo check` to verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Compiles with warnings about unused fields (expected — we haven't wired aggregation yet). Fix any errors where `UsagePayload` is constructed without `change_stats` — add `change_stats: None` to every existing construction site.

- [ ] **Step 5: Run all existing Rust tests to verify nothing is broken**

Run: `cd src-tauri && cargo test`
Expected: All existing tests pass. Some may need `change_stats: None` added to test fixtures.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/change_stats.rs src-tauri/src/models.rs
git commit -m "feat(change-stats): add change event structs and wire into models"
```

---

## Task 3: Aggregation Logic (Rust)

**Files:**
- Modify: `src-tauri/src/change_stats.rs`

- [ ] **Step 1: Add the `aggregate_change_stats` function**

Append to `change_stats.rs` (above `#[cfg(test)]`):

```rust
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
        rewrite_ratio: None, // v1: not computed
        churn_ratio: churn,
        dominant_extension: None, // v1: deferred
    })
}

pub fn aggregate_model_change_summary(
    events: &[ParsedChangeEvent],
    model_key: &str,
) -> Option<ModelChangeSummary> {
    let model_events: Vec<&ParsedChangeEvent> = events
        .iter()
        .filter(|e| e.model == model_key)
        .collect();

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
```

- [ ] **Step 2: Add aggregation unit tests**

Add to the `#[cfg(test)]` block in `change_stats.rs`:

```rust
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
    let total = stats.code_lines_changed + stats.docs_lines_changed
        + stats.config_lines_changed + stats.other_lines_changed;
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
```

- [ ] **Step 3: Run aggregation tests**

Run: `cd src-tauri && cargo test change_stats`
Expected: All tests pass (classification + aggregation).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/change_stats.rs
git commit -m "feat(change-stats): add aggregation logic with tests"
```

---

## Task 4: Claude Edit/Write Parsing (Rust)

**Files:**
- Modify: `src-tauri/src/parser.rs`

This is the most complex backend task. The parser needs to detect Claude `Edit` tool_use events with `structuredPatch` or `oldString`/`newString`, and `Write` events. These appear as `assistant` messages with `content` arrays containing `tool_use` blocks, followed by `user` messages with `tool_result` blocks.

- [ ] **Step 1: Add `ParsedChangeEvent` import and extend `CachedFileEntries`**

At the top of `parser.rs`, add:

```rust
use crate::change_stats::{classify_file, ChangeEventKind, ParsedChangeEvent};
```

Extend `CachedFileEntries`:

```rust
#[derive(Clone)]
struct CachedFileEntries {
    stamp: FileStamp,
    entries: Vec<ParsedEntry>,
    change_events: Vec<ParsedChangeEvent>,  // NEW
    earliest_date: Option<NaiveDate>,
}
```

Update `CachedFileLoad` similarly:

```rust
struct CachedFileLoad {
    entries: Vec<ParsedEntry>,
    change_events: Vec<ParsedChangeEvent>,  // NEW
    earliest_date: Option<NaiveDate>,
    lines_read: usize,
    opened: bool,
    from_cache: bool,
}
```

**CRITICAL: Update the parse function return type aliases.** Find `ClaudeParseResult` and `CodexParseResult` type aliases (around lines 395 and 542 of `parser.rs`). Update them to include change events:

```rust
// Before: type ClaudeParseResult = (Vec<ParsedEntry>, usize, bool);
// After:
type ClaudeParseResult = (Vec<ParsedEntry>, Vec<ParsedChangeEvent>, usize, bool);

// Same for CodexParseResult
type CodexParseResult = (Vec<ParsedEntry>, Vec<ParsedChangeEvent>, usize, bool);
```

**Update `load_cached_file`** (around line 900): When it calls the parse function and constructs `CachedFileLoad`, pass the change events through:

```rust
// In load_cached_file, after calling parse_claude_session_file or parse_codex_session_file:
let (entries, change_events, lines_read, opened) = parse_result;
// ... construct CachedFileLoad with change_events field
```

**Update `load_claude_entries_with_debug` and `load_codex_entries_with_debug`**: These methods currently collect only `loaded.entries`. They must ALSO collect `loaded.change_events` into a separate vec and return them. Add a `change_events: Vec<ParsedChangeEvent>` to their return type or create a new struct wrapping entries + change_events + debug info.
```

- [ ] **Step 2: Add Claude tool_use serde types**

Add after the existing Claude JSONL serde types:

```rust
#[derive(Deserialize)]
struct ClaudeContentBlock {
    #[serde(rename = "type", default)]
    block_type: String,
    name: Option<String>,
    input: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ClaudeToolResultBlock {
    #[serde(rename = "type", default)]
    block_type: String,
    #[serde(rename = "structuredPatch")]
    structured_patch: Option<String>,
}
```

Also extend `ClaudeJsonlMessage`:

```rust
#[derive(Deserialize)]
struct ClaudeJsonlMessage {
    model: Option<String>,
    usage: Option<ClaudeJsonlUsage>,
    id: Option<String>,
    content: Option<Vec<serde_json::Value>>,  // NEW — for tool_use blocks
    role: Option<String>,                      // NEW — "assistant" or "user"
}
```

- [ ] **Step 3: Add unified diff line counting helper**

Add a helper function:

```rust
fn count_diff_lines(patch: &str) -> (u64, u64) {
    let mut added: u64 = 0;
    let mut removed: u64 = 0;
    for line in patch.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            removed += 1;
        }
    }
    (added, removed)
}

fn count_old_new_diff(old: &str, new: &str) -> (u64, u64) {
    // Conservative line-count diff: treat as full replacement of old with new.
    // A set-based approach would produce wrong results for files with repeated
    // lines (blank lines, duplicate returns, etc.). This is Tier B accuracy —
    // it over-counts when old and new share content, but never under-counts.
    let removed = old.lines().count() as u64;
    let added = new.lines().count() as u64;
    (added, removed)
}
```

- [ ] **Step 4: Integrate change event extraction into Claude file parsing**

In the function that parses Claude JSONL lines (the closure/function that processes each `ClaudeJsonlEntry`), after emitting the existing `ParsedEntry`, also scan `content` blocks for `tool_use` with `name == "Edit"` or `name == "Write"`. Extract:

- For `Edit`: get `file_path` from input, look for `structuredPatch` in the paired tool_result (or fall back to diffing `old_string`/`new_string`)
- For `Write`: increment write_events, only count lines if prior content is available in the same event

The exact integration point depends on the existing parse loop structure — the implementer should:

1. After parsing a `ClaudeJsonlEntry` as `assistant` type, scan `message.content` for `tool_use` blocks
2. For each `Edit` tool_use: extract `file_path`, `old_string`, `new_string` from `input`
3. Count diff lines using `count_old_new_diff` or `count_diff_lines` if `structuredPatch` is available
4. Create a `ParsedChangeEvent` with the **normalized model_key** (use `normalize_claude_model(raw).1` or `normalize_codex_model(raw).1`), NOT the raw model string. This is critical — `aggregate_model_change_summary` matches on `model_key`, which is the normalized form.
5. Push to the file's `change_events` vec

**Note to implementer:** This step requires careful reading of the existing parse loop in `parser.rs`. The Claude JSONL format interleaves `assistant` (with tool_use) and `user` (with tool_result) entries. For v1, parsing `old_string`/`new_string` from `Edit` input is sufficient — `structuredPatch` from tool_result is a bonus.

- [ ] **Step 5: Add unit test for Claude edit parsing**

Create a test JSONL fixture with an assistant message containing an Edit tool_use, and verify a `ParsedChangeEvent` is emitted:

```rust
#[test]
fn parse_claude_edit_emits_change_event() {
    // Write a temp JSONL file with an assistant message containing:
    // content: [{ type: "tool_use", name: "Edit", input: { file_path: "src/main.rs", old_string: "fn old()", new_string: "fn new()\nfn extra()" }}]
    // Verify: 1 change event, added_lines=1, removed_lines=0, category=Code
}
```

- [ ] **Step 6: Run tests**

Run: `cd src-tauri && cargo test`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/parser.rs
git commit -m "feat(change-stats): parse Claude Edit events for change metrics"
```

---

## Task 5: Codex apply_patch Parsing (Rust)

**Files:**
- Modify: `src-tauri/src/parser.rs`

- [ ] **Step 1: Detect Codex `apply_patch` events in the Codex parse loop**

In the Codex parsing function, after processing `turn_context` and `event_msg` types, also detect tool call events that contain `apply_patch`. The Codex JSONL contains custom tool calls with unified diffs.

Look for entries where `payload.type == "function_call"` (or similar) and `payload.name == "apply_patch"`. Extract the unified diff from the arguments, parse file paths from `---`/`+++` headers, and count `+`/`-` lines using `count_diff_lines`.

For each file in the patch:
1. Extract the file path from the diff header
2. Count added/removed lines for that file
3. Create a `ParsedChangeEvent` with `provider: "codex"` and the current model

- [ ] **Step 2: Add unit test for Codex patch parsing**

```rust
#[test]
fn parse_codex_apply_patch_emits_change_events() {
    // Write a temp JSONL file with a Codex apply_patch event containing:
    // --- a/src/main.rs
    // +++ b/src/main.rs
    // @@ -1,3 +1,4 @@
    //  existing line
    // +new line
    //  another line
    // Verify: 1 change event for src/main.rs, added_lines=1, removed_lines=0
}
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/parser.rs
git commit -m "feat(change-stats): parse Codex apply_patch events for change metrics"
```

---

## Task 6: Wire Aggregation into Commands (Rust)

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/parser.rs` (expose change events from aggregation methods)

- [ ] **Step 1: Expose change events from parser aggregation**

The existing `get_daily`, `get_hourly`, `get_monthly`, `get_blocks` methods return `UsagePayload`. They need to also collect `ParsedChangeEvent`s from the cached file data and return them alongside the payload.

**Approach: Add a separate method** `get_change_events(&self, provider: &str, since: &str) -> Vec<ParsedChangeEvent>` to `UsageParser`. This method:

1. Calls `load_claude_entries_with_debug` and/or `load_codex_entries_with_debug` (which now return change events per the Task 4 changes)
2. Filters change events by the `since` date (same filter logic as usage entries: `event.timestamp.date_naive() >= since_date`)
3. Returns the filtered `Vec<ParsedChangeEvent>`

This is called from `get_usage_data` in `commands.rs` AFTER building the `UsagePayload`, using the same `since` date string that was computed for the aggregation method. The change events use the same file cache, so there's no redundant file I/O.

- [ ] **Step 2: In `get_usage_data` command, aggregate change stats**

After building the `UsagePayload`, call `aggregate_change_stats` on the collected change events and attach the result:

```rust
payload.change_stats = aggregate_change_stats(&change_events, payload.total_cost, payload.total_tokens);
```

For model breakdowns, iterate `payload.model_breakdown` and attach per-model summaries:

```rust
for model in &mut payload.model_breakdown {
    model.change_stats = aggregate_model_change_summary(&change_events, &model.model_key);
}
```

- [ ] **Step 3: Handle `provider = "all"` merge**

When merging Claude and Codex payloads for `provider = "all"`:
- Concatenate change event lists from both providers
- Re-aggregate from the merged list (don't average ratios)
- Set `rewrite_ratio: None` in merged mode (Codex doesn't support it)

- [ ] **Step 4: Run full test suite**

Run: `cd src-tauri && cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/parser.rs src-tauri/src/commands.rs
git commit -m "feat(change-stats): wire aggregation into usage data commands"
```

---

## Task 7: TypeScript Types and Settings (Frontend)

**Files:**
- Modify: `src/lib/types/index.ts`
- Modify: `src/lib/stores/settings.ts`
- Modify: `src/lib/stores/settings.test.ts`

- [ ] **Step 1: Add `ChangeStats` and `ModelChangeSummary` interfaces**

In `src/lib/types/index.ts`, add after the `ActiveBlock` interface:

```typescript
export interface ChangeStats {
  added_lines: number;
  removed_lines: number;
  net_lines: number;  // Can be negative — only signed integer field
  files_touched: number;
  change_events: number;
  write_events: number;
  code_lines_changed: number;
  docs_lines_changed: number;
  config_lines_changed: number;
  other_lines_changed: number;
  avg_lines_per_event: number | null;
  cost_per_100_net_lines: number | null;
  tokens_per_net_line: number | null;
  rewrite_ratio: number | null;
  churn_ratio: number | null;
  dominant_extension: string | null;
}

export interface ModelChangeSummary {
  added_lines: number;
  removed_lines: number;
  net_lines: number;
  files_touched: number;
  change_events: number;
}
```

- [ ] **Step 2: Update `UsagePayload` and `ModelSummary`**

```typescript
export interface UsagePayload {
  // ... existing fields ...
  change_stats: ChangeStats | null;
}

export interface ModelSummary {
  display_name: string;
  model_key: string;
  cost: number;
  tokens: number;
  change_stats: ModelChangeSummary | null;
}
```

- [ ] **Step 3: Add `showModelChangeStats` to settings**

In `src/lib/stores/settings.ts`, find the `DEFAULTS` object and add:

```typescript
showModelChangeStats: false,
```

Also add to the `Settings` interface (or wherever the type is defined — check if it's in types/index.ts or settings.ts):

```typescript
showModelChangeStats: boolean;
```

Add normalization in `normalizeSettings()`. The function builds an explicit return object — add `showModelChangeStats` to it:

```typescript
// In the normalizeSettings return object, add:
showModelChangeStats: typeof saved?.showModelChangeStats === "boolean"
  ? saved.showModelChangeStats
  : DEFAULTS.showModelChangeStats,
```

Check the exact normalization pattern used for other boolean fields (like `brandTheming`, `glassEffect`) and follow it exactly. The function returns a fully-typed object literal — every field must be present or TypeScript will error.

- [ ] **Step 4: Add settings test**

In `src/lib/stores/settings.test.ts`, add:

```typescript
it("defaults showModelChangeStats to false", async () => {
  mockLoad.mockResolvedValueOnce(makePersistedStore({}));
  const { loadSettings, settings } = await loadSettingsModule();
  await loadSettings();
  expect(get(settings).showModelChangeStats).toBe(false);
});
```

- [ ] **Step 5: Run frontend tests**

Run: `npm test`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib/types/index.ts src/lib/stores/settings.ts src/lib/stores/settings.test.ts
git commit -m "feat(change-stats): add TypeScript types and settings default"
```

---

## Task 8: Composition Color Tokens (CSS)

**Files:**
- Modify: `src/app.css`

- [ ] **Step 1: Add composition color CSS custom properties**

In `src/app.css`, add to the dark theme (`:root, [data-theme="dark"]`) block:

```css
  --comp-code: #60a5fa;
  --comp-docs: #a78bfa;
  --comp-config: #fbbf24;
  --comp-other: rgba(255,255,255,0.15);
```

Add to the light theme (`[data-theme="light"]`) block:

```css
  --comp-code: #3b82f6;
  --comp-docs: #8b5cf6;
  --comp-config: #d97706;
  --comp-other: rgba(0,0,0,0.10);
```

Add to the system light media query (`@media (prefers-color-scheme: light)` → `:root:not([data-theme])`) block:

```css
  --comp-code: #3b82f6;
  --comp-docs: #8b5cf6;
  --comp-config: #d97706;
  --comp-other: rgba(0,0,0,0.10);
```

- [ ] **Step 2: Commit**

```bash
git add src/app.css
git commit -m "feat(change-stats): add composition color tokens for all themes"
```

---

## Task 9: MetricsRow 2×2 Grid (Frontend)

**Files:**
- Modify: `src/lib/components/MetricsRow.svelte`

This is the core UI change. Replace the 3-card horizontal row with a 2×2 grid.

- [ ] **Step 1: Update the component script**

Replace the entire `MetricsRow.svelte` with the 2×2 grid layout. The component needs:

- Import `ChangeStats` type
- Derive composition percentages from `change_stats`
- Derive efficiency metric display
- Handle all edge states (null stats, negative net, zero total)

```svelte
<script lang="ts">
  import { formatCost, formatTokens } from "../utils/format.js";
  import { settings } from "../stores/settings.js";
  import { activePeriod } from "../stores/usage.js";
  import type { UsagePayload, UsagePeriod } from "../types/index.js";

  interface Props { data: UsagePayload }
  let { data }: Props = $props();

  let threshold = $state(0);
  let period = $state<UsagePeriod>("day");
  $effect(() => {
    const unsub = settings.subscribe((s) => (threshold = s.costAlertThreshold));
    return unsub;
  });
  $effect(() => {
    const unsub = activePeriod.subscribe((value) => (period = value));
    return unsub;
  });

  let overBudget = $derived(threshold > 0 && data.total_cost >= threshold);
  let isLive = $derived(!!data.active_block?.is_active);
  let burnRate = $derived(data.active_block?.burn_rate_per_hour ?? 0);

  let inLabel = $derived(formatTokens(data.input_tokens));
  let outLabel = $derived(formatTokens(data.output_tokens));

  let cs = $derived(data.change_stats);
  let hasChanges = $derived(cs != null && (cs.added_lines > 0 || cs.removed_lines > 0));
  let netNegative = $derived(cs != null && cs.net_lines < 0);

  // Composition
  let compTotal = $derived(
    cs ? cs.code_lines_changed + cs.docs_lines_changed + cs.config_lines_changed + cs.other_lines_changed : 0
  );
  // NOTE: Use $derived(expression), not $derived(() => ...).
  // $derived(() => ...) wraps the function as the value and breaks reactivity.
  let compPcts = $derived(
    cs && compTotal > 0
      ? {
          code: (cs.code_lines_changed / compTotal) * 100,
          docs: (cs.docs_lines_changed / compTotal) * 100,
          config: (cs.config_lines_changed / compTotal) * 100,
          other: (cs.other_lines_changed / compTotal) * 100,
        }
      : { code: 0, docs: 0, config: 0, other: 0 }
  );

  // Efficiency
  let effLabel = $derived(
    cs?.cost_per_100_net_lines != null
      ? `${formatCost(cs.cost_per_100_net_lines)}/100L`
      : "—"
  );

  // Cost sublabel
  let costSub = $derived(
    isLive && burnRate > 0
      ? `${formatCost(burnRate)}/h`
      : data.session_count > 0
        ? `${data.session_count} sessions`
        : ""
  );
</script>
```

- [ ] **Step 2: Update the template**

```svelte
<div class="met"
  aria-label="Usage metrics"
>
  <!-- Cost -->
  <div class="m" class:alert={overBudget} class:live={isLive}>
    <div class="m-v">{formatCost(data.total_cost)}</div>
    <div class="m-l">
      {#if isLive}<span class="live-dot"></span>{/if}{overBudget ? "Over budget" : "Cost"}
    </div>
    {#if costSub}<div class="m-s">{costSub}</div>{/if}
  </div>

  <!-- Changes -->
  <div class="m" class:m-quiet={!hasChanges}>
    {#if hasChanges}
      <div class="m-v" aria-label="{cs.added_lines} lines added, {cs.removed_lines} lines removed, net {cs.net_lines} lines">
        <span class="ch-plus">+{cs.added_lines.toLocaleString()}</span>
        <span class="ch-slash">/</span>
        <span class="ch-minus">−{cs.removed_lines.toLocaleString()}</span>
      </div>
      <div class="m-l">Changes</div>
      <div class="m-s" class:ch-neg={netNegative}>
        net {netNegative ? "−" : "+"}{Math.abs(cs.net_lines).toLocaleString()} · {cs.files_touched} files
      </div>
    {:else}
      <div class="m-v m-v-empty">—</div>
      <div class="m-l">Changes</div>
      <div class="m-s m-empty">No structured edits detected</div>
    {/if}
  </div>

  <!-- Tokens -->
  <div class="m">
    <div class="m-v">{formatTokens(data.total_tokens)}</div>
    <div class="m-l">Tokens</div>
    {#if data.input_tokens > 0}
      <div class="m-s">{inLabel} in · {outLabel} out</div>
    {/if}
  </div>

  <!-- Composition -->
  <div class="m comp" class:m-quiet={compTotal === 0}>
    {#if compTotal > 0}
      {@const pcts = compPcts}
      <div class="comp-head">
        <span class="m-l" style="margin:0">Composition</span>
        <span class="comp-eff">{effLabel}</span>
      </div>
      <div class="comp-bar"
        role="img"
        aria-label="{Math.round(pcts.code)}% code, {Math.round(pcts.docs)}% docs, {Math.round(pcts.config)}% config, {Math.round(pcts.other)}% other"
      >
        {#if pcts.code > 0}<div class="comp-seg" style="width:{pcts.code}%; background:var(--comp-code)"></div>{/if}
        {#if pcts.docs > 0}<div class="comp-seg" style="width:{pcts.docs}%; background:var(--comp-docs)"></div>{/if}
        {#if pcts.config > 0}<div class="comp-seg" style="width:{pcts.config}%; background:var(--comp-config)"></div>{/if}
        {#if pcts.other > 0}<div class="comp-seg" style="width:{pcts.other}%; background:var(--comp-other)"></div>{/if}
      </div>
      <div class="comp-legend">
        {#if pcts.code > 0}<span class="comp-item"><span class="comp-dot" style="background:var(--comp-code)"></span>code {Math.round(pcts.code)}%</span>{/if}
        {#if pcts.docs > 0}<span class="comp-item"><span class="comp-dot" style="background:var(--comp-docs)"></span>docs {Math.round(pcts.docs)}%</span>{/if}
        {#if pcts.config > 0}<span class="comp-item"><span class="comp-dot" style="background:var(--comp-config)"></span>config {Math.round(pcts.config)}%</span>{/if}
        {#if pcts.other > 0}<span class="comp-item"><span class="comp-dot" style="background:var(--comp-other)"></span>other {Math.round(pcts.other)}%</span>{/if}
      </div>
    {:else}
      <div class="m-v m-v-empty">—</div>
      <div class="m-l">Composition</div>
      <div class="m-s m-empty">No file changes to classify</div>
    {/if}
  </div>
</div>
```

- [ ] **Step 3: Update the styles**

Replace the `<style>` block:

```css
<style>
  .met { display: flex; padding: 12px 12px 10px; gap: 4px; flex-wrap: wrap; animation: fadeUp .28s ease both .07s; }
  .m {
    flex: 1 1 calc(50% - 2px); min-width: calc(50% - 2px);
    padding: 8px 9px;
    background: var(--surface-2); border-radius: 7px;
    transition: background .18s;
  }
  .m:hover { background: var(--surface-hover); }
  .m-v {
    font: 400 13px/1 'Inter', sans-serif;
    color: var(--t1); font-variant-numeric: tabular-nums;
    letter-spacing: -.2px;
  }
  .m-l {
    display: flex; align-items: center; gap: 3px;
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3); text-transform: uppercase;
    letter-spacing: .7px; margin-top: 4px;
  }
  .m-s {
    font: 400 8px/1 'Inter', sans-serif;
    color: var(--t4); margin-top: 2px; letter-spacing: .1px;
  }
  .m.alert {
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.25);
  }
  .m.alert .m-v { color: #ef4444; }
  .m.alert .m-l { color: #f87171; }

  /* Live dot */
  .live-dot {
    width: 4px; height: 4px; border-radius: 50%;
    background: var(--accent); flex-shrink: 0;
    animation: livePulse 2s ease-in-out infinite;
  }
  @keyframes livePulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  /* Changes card */
  .ch-plus { color: #4ade80; }
  .ch-minus { color: #f87171; }
  .ch-slash { font-size: 10px; color: var(--t4); margin: 0 1px; }
  .ch-neg { color: #f87171; }

  /* Quiet state (no data) */
  .m-quiet .m-v { color: var(--t4); }
  .m-v-empty { font-size: 13px; }
  .m-empty { color: var(--t4); font-size: 8px; }

  /* Composition card */
  .comp { display: flex; flex-direction: column; gap: 5px; }
  .comp-head {
    display: flex; justify-content: space-between; align-items: baseline;
  }
  .comp-eff {
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3); font-variant-numeric: tabular-nums;
  }
  .comp-bar {
    height: 5px; border-radius: 2.5px;
    background: var(--surface-2); overflow: hidden;
    display: flex;
  }
  .comp-seg {
    height: 100%;
    animation: hBarGrow .52s cubic-bezier(.22,1,.36,1) both;
  }
  .comp-seg:first-child { border-radius: 2.5px 0 0 2.5px; }
  .comp-seg:last-child { border-radius: 0 2.5px 2.5px 0; }
  .comp-seg:only-child { border-radius: 2.5px; }
  @keyframes hBarGrow {
    from { transform: scaleX(0); }
    to { transform: scaleX(1); }
  }
  .comp-legend {
    display: flex; gap: 6px;
  }
  .comp-item {
    display: flex; align-items: center; gap: 3px;
    font: 400 7.5px/1 'Inter', sans-serif;
    color: var(--t4); white-space: nowrap;
  }
  .comp-dot {
    width: 4px; height: 4px; border-radius: 1px; flex-shrink: 0;
  }
</style>
```

- [ ] **Step 4: Verify the dev server renders correctly**

Run: `npm run tauri dev`
Expected: The MetricsRow shows a 2×2 grid. If no change data exists yet (backend not wired), the Changes and Composition cards should show the empty `—` state.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/MetricsRow.svelte
git commit -m "feat(change-stats): implement 2×2 metrics grid with composition bar"
```

---

## Task 10: ModelList Optional Change Stats (Frontend)

**Files:**
- Modify: `src/lib/components/ModelList.svelte`

- [ ] **Step 1: Add optional change stats display**

Update the component to read the `showModelChangeStats` setting and conditionally render per-model change stats:

In the `<script>`:
```typescript
let showChanges = $state(false);
$effect(() => {
  const unsub = settings.subscribe((s) => (showChanges = s.showModelChangeStats));
  return unsub;
});
```

In each model row, after the model name and before the cost:
```svelte
{#if showChanges && row.change_stats}
  <span class="mcs">
    <span class="mcs-plus">+{row.change_stats.added_lines.toLocaleString()}</span>
    <span class="mcs-slash">/</span>
    <span class="mcs-minus">−{row.change_stats.removed_lines.toLocaleString()}</span>
  </span>
{/if}
```

Add styles:
```css
.mcs {
  font: 400 9px/1 'Inter', sans-serif;
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}
.mcs-plus { color: #4ade80; }
.mcs-minus { color: #f87171; }
.mcs-slash { color: var(--t4); font-size: 8px; margin: 0 1px; }
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/components/ModelList.svelte
git commit -m "feat(change-stats): add optional per-model change stats in model list"
```

---

## Task 11: Settings Toggle (Frontend)

**Files:**
- Modify: `src/lib/components/Settings.svelte`

- [ ] **Step 1: Add the `showModelChangeStats` toggle**

Find the "Monitoring" section in Settings.svelte (where `hiddenModels` / model toggles live). Add a toggle row nearby:

```svelte
<div class="s-row">
  <div class="s-row-text">
    <div class="s-row-title">Model change stats</div>
    <div class="s-row-desc">Show lines changed per model in the breakdown list</div>
  </div>
  <ToggleSwitch
    checked={localSettings.showModelChangeStats}
    onChange={(v) => updateSetting("showModelChangeStats", v)}
  />
</div>
```

Use the same pattern as existing toggles in Settings.svelte — check which component is used (`ToggleSwitch`) and what props it takes.

- [ ] **Step 2: Commit**

```bash
git add src/lib/components/Settings.svelte
git commit -m "feat(change-stats): add showModelChangeStats toggle in settings"
```

---

## Task 12: Integration Test and Polish

**Files:**
- All modified files

- [ ] **Step 1: Run full Rust test suite**

Run: `cd src-tauri && cargo test`
Expected: All tests pass.

- [ ] **Step 2: Run full frontend test suite**

Run: `npm test`
Expected: All tests pass.

- [ ] **Step 3: Run the full app**

Run: `npm run tauri dev`
Expected:
- 2×2 grid renders correctly in dark and light themes
- Composition bar shows correct colors
- Empty states render gracefully
- Model list toggle works in settings
- No console errors

- [ ] **Step 4: Test with real Claude Code logs (if available)**

Open the app with existing Claude Code usage. Verify:
- Change stats populate from real log data
- Numbers look plausible
- Composition breakdown reflects actual file types

- [ ] **Step 5: Final commit if any polish needed**

```bash
git add -A
git commit -m "feat(change-stats): polish and integration fixes"
```
