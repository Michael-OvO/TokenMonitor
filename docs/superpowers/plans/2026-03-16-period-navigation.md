# Period Navigation Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add back/forward arrow navigation to view past days, weeks, months, and years of token usage data.

**Architecture:** Offset-based navigation where the frontend sends an `offset: i32` (0 = current, negative = past) and the backend returns usage data plus `period_label` (human-readable date string) and `has_earlier_data` (boolean for disabling the back arrow). A new `DateNav.svelte` component renders the `‹ label ›` row between TimeTabs and MetricsRow.

**Tech Stack:** Rust (Tauri v2 backend, chrono for dates), Svelte 5 (TypeScript frontend)

**Spec:** `docs/superpowers/specs/2026-03-16-period-navigation-design.md`

---

## Chunk 1: Backend — Models, Commands, Parser

### Task 1: Add new fields to UsagePayload (models.rs)

**Files:**
- Modify: `src-tauri/src/models.rs:6-18`

- [ ] **Step 1: Add `period_label` and `has_earlier_data` fields to UsagePayload**

In `src-tauri/src/models.rs`, add two fields to the `UsagePayload` struct after `from_cache`:

```rust
pub struct UsagePayload {
    pub total_cost: f64,
    pub total_tokens: u64,
    pub session_count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub chart_buckets: Vec<ChartBucket>,
    pub model_breakdown: Vec<ModelSummary>,
    pub active_block: Option<ActiveBlock>,
    pub five_hour_cost: f64,
    pub last_updated: String,
    pub from_cache: bool,
    pub period_label: String,
    pub has_earlier_data: bool,
}
```

- [ ] **Step 2: Fix all existing UsagePayload construction sites**

The compiler will report errors everywhere a `UsagePayload` is constructed without the new fields. Add `period_label: String::new(), has_earlier_data: false` to every construction site:

- `src-tauri/src/parser.rs` — `get_daily()`, `get_monthly()`, `get_hourly()`, `get_blocks()` (4 sites)
- `src-tauri/src/commands.rs` — `payload_with_buckets()` test helper, `merge_payloads()`, and the two inline `UsagePayload` literals in the `merge_payloads_combines_model_breakdowns_and_active_blocks` test (lines ~274 and ~292)

For `merge_payloads`, add this line before the return:

```rust
c.has_earlier_data = c.has_earlier_data && x.has_earlier_data;
```

(`c.period_label` is already set by the caller — no assignment needed.)

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check 2>&1 | head -20`
Expected: no errors (warnings OK)

- [ ] **Step 4: Run existing tests to confirm nothing broke**

Run: `cd src-tauri && cargo test 2>&1 | tail -20`
Expected: all existing tests pass

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/parser.rs src-tauri/src/commands.rs
git commit -m "feat(models): add period_label and has_earlier_data to UsagePayload"
```

---

### Task 2: Add `has_entries_before()` to parser (parser.rs)

**Files:**
- Modify: `src-tauri/src/parser.rs`

- [ ] **Step 1: Write failing tests for `has_entries_before`**

Add these tests at the bottom of the `mod tests` block in `src-tauri/src/parser.rs`:

```rust
#[test]
fn has_entries_before_claude_returns_true_when_old_entries_exist() {
    let dir = TempDir::new().unwrap();
    let content = r#"{"type":"assistant","timestamp":"2026-01-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
    write_file(&dir.path().join("session.jsonl"), content);

    let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
    assert!(parser.has_entries_before("claude", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
}

#[test]
fn has_entries_before_claude_returns_false_when_no_old_entries() {
    let dir = TempDir::new().unwrap();
    let content = r#"{"type":"assistant","timestamp":"2026-03-15T12:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
    write_file(&dir.path().join("session.jsonl"), content);

    let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
    assert!(!parser.has_entries_before("claude", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
}

#[test]
fn has_entries_before_codex_returns_true_when_old_dirs_exist() {
    let dir = TempDir::new().unwrap();
    let day_dir = dir.path().join("2026").join("01").join("15");
    fs::create_dir_all(&day_dir).unwrap();
    write_file(&day_dir.join("session.jsonl"), "{}");

    let parser = UsageParser::with_codex_dir(dir.path().to_path_buf());
    assert!(parser.has_entries_before("codex", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
}

#[test]
fn has_entries_before_codex_returns_false_when_no_old_dirs() {
    let dir = TempDir::new().unwrap();
    let day_dir = dir.path().join("2026").join("03").join("15");
    fs::create_dir_all(&day_dir).unwrap();
    write_file(&day_dir.join("session.jsonl"), "{}");

    let parser = UsageParser::with_codex_dir(dir.path().to_path_buf());
    assert!(!parser.has_entries_before("codex", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
}

#[test]
fn has_entries_before_empty_dir_returns_false() {
    let dir = TempDir::new().unwrap();
    let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
    assert!(!parser.has_entries_before("claude", NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test has_entries_before 2>&1 | tail -10`
Expected: FAIL — method `has_entries_before` not found

- [ ] **Step 3: Implement `has_entries_before` on UsageParser**

Add this method to the `impl UsageParser` block in `src-tauri/src/parser.rs`:

```rust
/// Check whether any log entries exist before `before_date`.
/// Used to determine if the back-arrow should be enabled.
pub fn has_entries_before(&self, provider: &str, before_date: NaiveDate) -> bool {
    match provider {
        "claude" => self.has_claude_entries_before(before_date),
        "codex" => self.has_codex_entries_before(before_date),
        _ => {
            self.has_claude_entries_before(before_date)
                || self.has_codex_entries_before(before_date)
        }
    }
}

fn has_claude_entries_before(&self, before_date: NaiveDate) -> bool {
    let files = glob_jsonl_files(&self.claude_dir);
    for path in files {
        let file = match fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            if !line.contains("\"assistant\"") {
                continue;
            }
            let entry: ClaudeJsonlEntry = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.entry_type != "assistant" {
                continue;
            }
            // Check stop_reason to match read_claude_entries filtering
            let msg = match &entry.message {
                Some(m) if m.stop_reason.is_some() => m,
                _ => continue,
            };
            if msg.usage.is_none() {
                continue;
            }
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&entry.timestamp) {
                if dt.with_timezone(&Local).date_naive() < before_date {
                    return true;
                }
            }
        }
    }
    false
}

fn has_codex_entries_before(&self, before_date: NaiveDate) -> bool {
    // Iterate YYYY/MM/DD directories and check if any date < before_date has .jsonl files
    let years = match fs::read_dir(&self.codex_dir) {
        Ok(rd) => rd,
        Err(_) => return false,
    };
    for year_entry in years.flatten() {
        let year_name = year_entry.file_name().to_string_lossy().to_string();
        let year: i32 = match year_name.parse() {
            Ok(y) => y,
            Err(_) => continue,
        };
        let months = match fs::read_dir(year_entry.path()) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for month_entry in months.flatten() {
            let month_name = month_entry.file_name().to_string_lossy().to_string();
            let month: u32 = match month_name.parse() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let days = match fs::read_dir(month_entry.path()) {
                Ok(rd) => rd,
                Err(_) => continue,
            };
            for day_entry in days.flatten() {
                let day_name = day_entry.file_name().to_string_lossy().to_string();
                let day: u32 = match day_name.parse() {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                    if date < before_date {
                        // Check if directory contains any .jsonl files
                        if let Ok(files) = fs::read_dir(day_entry.path()) {
                            for f in files.flatten() {
                                if f.path().extension().is_some_and(|e| e == "jsonl") {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    false
}
```

Note: `ClaudeJsonlEntry` is a private struct in parser.rs — the new method is in the same file, so it has access.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test has_entries_before 2>&1 | tail -15`
Expected: all 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/parser.rs
git commit -m "feat(parser): add has_entries_before() for navigation boundary check"
```

---

### Task 3: Fix `get_hourly()` for past days (parser.rs)

**Files:**
- Modify: `src-tauri/src/parser.rs`

- [ ] **Step 1: Write failing test for past-day hourly**

Add this test to the `mod tests` block in `src-tauri/src/parser.rs`:

```rust
#[test]
fn get_hourly_past_day_returns_24_buckets() {
    let dir = TempDir::new().unwrap();
    // Entry at 9 AM on a specific past date
    let content = r#"{"type":"assistant","timestamp":"2026-01-15T09:00:00+00:00","message":{"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}"#;
    write_file(&dir.path().join("session.jsonl"), content);

    let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
    let payload = parser.get_hourly("claude", "20260115");
    assert_eq!(payload.chart_buckets.len(), 24, "past day should have 24 hourly buckets");
    // The 9 AM bucket should have data
    let nine_am = payload.chart_buckets.iter().find(|b| b.label == "9AM").unwrap();
    assert!(nine_am.total > 0.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test get_hourly_past_day 2>&1 | tail -10`
Expected: FAIL — bucket count is not 24

- [ ] **Step 3: Modify `get_hourly` to use 0..=23 for past days**

First, add `Datelike` to the chrono import at the top of `src-tauri/src/parser.rs`:

```rust
use chrono::{Datelike, DateTime, Local, NaiveDate, Timelike};
```

Then in the `get_hourly` method, change the hour range logic. Currently:

```rust
let now = Local::now();
let current_hour = now.hour();
let min_hour = hour_map.keys().copied().min().unwrap_or(current_hour);
// ...
for h in min_hour..=current_hour {
```

Replace with:

```rust
let now = Local::now();
let today = now.date_naive();
let since_naive = parse_since_date(since);
let is_past_day = since_naive.is_some_and(|d| d < today);
let (start_hour, end_hour) = if is_past_day {
    (0u32, 23u32)
} else {
    let current_hour = now.hour();
    let min_hour = hour_map.keys().copied().min().unwrap_or(current_hour);
    (min_hour, current_hour)
};
// ...
for h in start_hour..=end_hour {
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test get_hourly_past_day 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cd src-tauri && cargo test 2>&1 | tail -10`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/parser.rs
git commit -m "fix(parser): get_hourly returns 24 buckets for past days"
```

---

### Task 4: Add offset parameter and period labels to commands (commands.rs)

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Write tests for offset date computation and period labels**

Add these tests to the `mod tests` block in `src-tauri/src/commands.rs`:

```rust
#[test]
fn period_label_day_format() {
    let date = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
    assert_eq!(format_day_label(date), "March 15, 2026");
}

#[test]
fn period_label_week_same_month() {
    let monday = NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
    let sunday = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
    assert_eq!(format_week_label(monday, sunday), "Mar 9 – 15, 2026");
}

#[test]
fn period_label_week_cross_month() {
    let monday = NaiveDate::from_ymd_opt(2026, 3, 30).unwrap();
    let sunday = NaiveDate::from_ymd_opt(2026, 4, 5).unwrap();
    assert_eq!(format_week_label(monday, sunday), "Mar 30 – Apr 5, 2026");
}

#[test]
fn period_label_week_cross_year() {
    let monday = NaiveDate::from_ymd_opt(2025, 12, 29).unwrap();
    let sunday = NaiveDate::from_ymd_opt(2026, 1, 4).unwrap();
    assert_eq!(format_week_label(monday, sunday), "Dec 29, 2025 – Jan 4, 2026");
}

#[test]
fn period_label_month_format() {
    let date = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    assert_eq!(format_month_label(date), "March 2026");
}

#[test]
fn period_label_year_format() {
    assert_eq!(format_year_label(2026), "2026");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test period_label 2>&1 | tail -10`
Expected: FAIL — functions not found

- [ ] **Step 3: Implement label formatting functions**

Add these functions to `src-tauri/src/commands.rs` (above the `get_provider_data` function):

```rust
fn format_day_label(date: NaiveDate) -> String {
    date.format("%B %-d, %Y").to_string()
}

fn format_week_label(monday: NaiveDate, sunday: NaiveDate) -> String {
    if monday.year() != sunday.year() {
        // Cross-year: "Dec 29, 2025 – Jan 4, 2026"
        format!(
            "{} – {}",
            monday.format("%b %-d, %Y"),
            sunday.format("%b %-d, %Y")
        )
    } else if monday.month() != sunday.month() {
        // Cross-month: "Mar 30 – Apr 5, 2026"
        format!(
            "{} – {}",
            monday.format("%b %-d"),
            sunday.format("%b %-d, %Y")
        )
    } else {
        // Same month: "Mar 9 – 15, 2026"
        format!(
            "{} – {}",
            monday.format("%b %-d"),
            sunday.format("%-d, %Y")
        )
    }
}

fn format_month_label(first_of_month: NaiveDate) -> String {
    first_of_month.format("%B %Y").to_string()
}

fn format_year_label(year: i32) -> String {
    year.to_string()
}
```

- [ ] **Step 4: Run label tests**

Run: `cd src-tauri && cargo test period_label 2>&1 | tail -15`
Expected: all 6 tests pass

- [ ] **Step 5: Update `get_usage_data` to accept `offset` parameter**

In `src-tauri/src/commands.rs`, update the command signature:

```rust
#[tauri::command]
pub async fn get_usage_data(
    provider: String,
    period: String,
    offset: i32,
    state: State<'_, AppState>,
) -> Result<UsagePayload, String> {
    let parser = &state.parser;

    match provider.as_str() {
        "claude" | "codex" => Ok(get_provider_data(parser, &provider, &period, offset)?),
        "all" => {
            let claude = get_provider_data(parser, "claude", &period, offset)?;
            let codex = get_provider_data(parser, "codex", &period, offset)?;
            Ok(merge_payloads(claude, codex))
        }
        _ => Err(format!("Unknown provider: {}", provider)),
    }
}
```

- [ ] **Step 6: Rewrite `get_provider_data` with offset-based date logic**

Replace the existing `get_provider_data` function with:

```rust
fn get_provider_data(
    parser: &UsageParser,
    provider: &str,
    period: &str,
    offset: i32,
) -> Result<UsagePayload, String> {
    let now = Local::now();
    let today = now.date_naive();

    let mut payload = match period {
        "5h" => {
            let today_str = today.format("%Y%m%d").to_string();
            parser.get_blocks(provider, &today_str)
        }
        "day" => {
            let target = today + chrono::Duration::days(offset as i64);
            let since_str = target.format("%Y%m%d").to_string();
            let mut p = parser.get_hourly(provider, &since_str);
            p.period_label = format_day_label(target);
            p.has_earlier_data = parser.has_entries_before(provider, target);
            p
        }
        "week" => {
            let current_monday = today
                - chrono::Duration::days(now.weekday().num_days_from_monday() as i64);
            let target_monday = current_monday + chrono::Duration::days((offset * 7) as i64);
            let target_sunday = target_monday + chrono::Duration::days(6);
            let since_str = target_monday.format("%Y%m%d").to_string();
            let mut p = parser.get_daily(provider, &since_str);
            p.period_label = format_week_label(target_monday, target_sunday);
            p.has_earlier_data = parser.has_entries_before(provider, target_monday);
            p
        }
        "month" => {
            let mut target_year = now.year();
            let mut target_month = now.month() as i32 + offset;
            // Normalize month overflow/underflow
            while target_month <= 0 {
                target_year -= 1;
                target_month += 12;
            }
            while target_month > 12 {
                target_year += 1;
                target_month -= 12;
            }
            let first_of_month =
                NaiveDate::from_ymd_opt(target_year, target_month as u32, 1).unwrap();
            let since_str = first_of_month.format("%Y%m%d").to_string();
            let mut p = parser.get_daily(provider, &since_str);
            p.period_label = format_month_label(first_of_month);
            p.has_earlier_data = parser.has_entries_before(provider, first_of_month);
            p
        }
        "year" => {
            let target_year = now.year() + offset;
            let first_of_year = NaiveDate::from_ymd_opt(target_year, 1, 1).unwrap();
            let since_str = first_of_year.format("%Y%m%d").to_string();
            let mut p = parser.get_monthly(provider, &since_str);
            p.period_label = format_year_label(target_year);
            p.has_earlier_data = parser.has_entries_before(provider, first_of_year);
            p
        }
        _ => return Err(format!("Unknown period: {}", period)),
    };

    // 5h doesn't set these — use defaults
    if period == "5h" {
        payload.period_label = String::new();
        payload.has_earlier_data = false;
    }

    Ok(payload)
}
```

Add `use chrono::Datelike;` to the imports if not already present (it already is).

- [ ] **Step 7: Update existing tests to pass offset to `get_provider_data`**

In the `codex_5h_uses_blocks_payload_shape` test, update:

```rust
let payload = get_provider_data(&parser, "codex", "5h", 0).unwrap();
```

- [ ] **Step 8: Run all tests**

Run: `cd src-tauri && cargo test 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(commands): add offset-based navigation with period labels"
```

---

### Task 5: Register the updated command in Tauri app setup (lib.rs)

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Check if lib.rs needs updating**

The `get_usage_data` command signature changed (added `offset` param). Tauri auto-generates the IPC binding from the function signature, so the Rust side is handled. No change needed to `lib.rs` — the command is already registered by name and the new parameter will be deserialized from the frontend's invoke call automatically.

- [ ] **Step 2: Verify full compilation**

Run: `cd src-tauri && cargo check 2>&1 | head -20`
Expected: no errors

- [ ] **Step 3: Run full test suite**

Run: `cd src-tauri && cargo test 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 4: Commit (if any changes were needed)**

Skip if no changes.

---

## Chunk 2: Frontend — Types, Store, DateNav, App Wiring

### Task 6: Update TypeScript types (types/index.ts)

**Files:**
- Modify: `src/lib/types/index.ts:1-13`

- [ ] **Step 1: Add `period_label` and `has_earlier_data` to UsagePayload interface**

In `src/lib/types/index.ts`, add two fields at the end of the `UsagePayload` interface:

```typescript
export interface UsagePayload {
  total_cost: number;
  total_tokens: number;
  session_count: number;
  input_tokens: number;
  output_tokens: number;
  chart_buckets: ChartBucket[];
  model_breakdown: ModelSummary[];
  active_block: ActiveBlock | null;
  five_hour_cost: number;
  last_updated: string;
  from_cache: boolean;
  period_label: string;
  has_earlier_data: boolean;
}
```

- [ ] **Step 2: Update `emptyPayload()` in usage.ts**

In `src/lib/stores/usage.ts`, add the two new fields to the `emptyPayload()` function:

```typescript
function emptyPayload(): UsagePayload {
  return {
    total_cost: 0,
    total_tokens: 0,
    session_count: 0,
    input_tokens: 0,
    output_tokens: 0,
    chart_buckets: [],
    model_breakdown: [],
    active_block: null,
    five_hour_cost: 0,
    last_updated: new Date().toISOString(),
    from_cache: false,
    period_label: "",
    has_earlier_data: false,
  };
}
```

- [ ] **Step 3: Commit**

```bash
git add src/lib/types/index.ts src/lib/stores/usage.ts
git commit -m "feat(types): add period_label and has_earlier_data to UsagePayload"
```

---

### Task 7: Update usage store for offset support (stores/usage.ts)

**Files:**
- Modify: `src/lib/stores/usage.ts`

- [ ] **Step 1: Add `activeOffset` store and update `cacheKey`**

In `src/lib/stores/usage.ts`, add the new store after line 6:

```typescript
export const activeOffset = writable<number>(0);
```

Update the `cacheKey` function:

```typescript
function cacheKey(provider: string, period: string, offset: number = 0) {
  return `${provider}:${period}:${offset}`;
}
```

- [ ] **Step 2: Update `fetchData` to accept and pass `offset`**

Update the function signature and all `cacheKey` / `invoke` calls:

```typescript
export async function fetchData(provider: string, period: string, offset: number = 0) {
  const requestId = ++currentRequestId;
  const key = cacheKey(provider, period, offset);

  // ── Stale-while-revalidate: instant show + silent refresh ──
  const cached = payloadCache.get(key);
  if (cached && Date.now() - cached.at < CACHE_TTL) {
    usageData.set(cached.data);
    invoke<UsagePayload>("get_usage_data", { provider, period, offset })
      .then((fresh: UsagePayload) => {
        payloadCache.set(key, { data: fresh, at: Date.now() });
        if (requestId === currentRequestId) {
          usageData.set(fresh);
        }
      })
      .catch(() => {});
    return;
  }

  if (cached) {
    usageData.set(cached.data);
  } else {
    usageData.set(emptyPayload());
  }
  isLoading.set(true);
  try {
    const data = await invoke<UsagePayload>("get_usage_data", {
      provider,
      period,
      offset,
    });
    if (requestId === currentRequestId) {
      payloadCache.set(key, { data, at: Date.now() });
      usageData.set(data);
    }
  } catch (e) {
    if (requestId === currentRequestId) {
      console.error("Failed to fetch usage data:", e);
    }
  } finally {
    if (requestId === currentRequestId) {
      isLoading.set(false);
    }
  }
}
```

- [ ] **Step 3: Update `warmCache` to accept `offset`**

```typescript
export function warmCache(provider: string, period: string, offset: number = 0) {
  const key = cacheKey(provider, period, offset);
  invoke<UsagePayload>("get_usage_data", { provider, period, offset })
    .then((data: UsagePayload) => {
      payloadCache.set(key, { data, at: Date.now() });
    })
    .catch(() => {});
}
```

`warmAllPeriods` stays unchanged — it always warms at offset 0 (the default).

- [ ] **Step 4: Commit**

```bash
git add src/lib/stores/usage.ts
git commit -m "feat(store): add activeOffset store and offset support to fetchData/warmCache"
```

---

### Task 8: Create DateNav component (DateNav.svelte)

**Files:**
- Create: `src/lib/components/DateNav.svelte`

- [ ] **Step 1: Create the DateNav component**

Create `src/lib/components/DateNav.svelte`:

```svelte
<script lang="ts">
  interface Props {
    periodLabel: string;
    hasEarlierData: boolean;
    isAtPresent: boolean;
    onBack: () => void;
    onForward: () => void;
    onReset: () => void;
  }
  let { periodLabel, hasEarlierData, isAtPresent, onBack, onForward, onReset }: Props = $props();
</script>

<div class="date-nav">
  <button
    class="arrow"
    class:disabled={!hasEarlierData}
    disabled={!hasEarlierData}
    onclick={onBack}
    aria-label="Previous period"
  >‹</button>

  <button
    class="label"
    class:clickable={!isAtPresent}
    onclick={onReset}
    disabled={isAtPresent}
    aria-label="Return to current period"
  >{periodLabel}</button>

  <button
    class="arrow"
    class:disabled={isAtPresent}
    disabled={isAtPresent}
    onclick={onForward}
    aria-label="Next period"
  >›</button>
</div>

<style>
  .date-nav {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 4px 12px 0;
    animation: fadeUp .28s ease both .05s;
  }
  .arrow {
    background: none;
    border: none;
    font-size: 14px;
    color: var(--t2);
    cursor: pointer;
    padding: 2px 6px;
    border-radius: 4px;
    transition: color .2s, opacity .2s;
    line-height: 1;
  }
  .arrow:hover:not(:disabled) {
    color: var(--t1);
    background: rgba(255,255,255,0.02);
  }
  .arrow.disabled {
    color: var(--t4, var(--t3));
    cursor: default;
    opacity: 0.4;
  }
  .label {
    background: none;
    border: none;
    font: 500 10px/1 'Inter', sans-serif;
    color: var(--t1);
    letter-spacing: 0.2px;
    cursor: default;
    padding: 2px 4px;
    border-radius: 4px;
    transition: color .2s;
  }
  .label.clickable {
    cursor: pointer;
  }
  .label.clickable:hover {
    color: var(--accent);
  }
</style>
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/components/DateNav.svelte
git commit -m "feat(ui): create DateNav component for period navigation"
```

---

### Task 9: Wire everything together in App.svelte

**Files:**
- Modify: `src/App.svelte`

- [ ] **Step 1: Add import for DateNav and activeOffset**

At the top of `src/App.svelte`, add the import after the existing component imports:

```typescript
import DateNav from "./lib/components/DateNav.svelte";
```

And update the usage store import to include `activeOffset`:

```typescript
import {
  activeProvider,
  activePeriod,
  activeOffset,
  usageData,
  isLoading,
  fetchData,
  warmCache,
  warmAllPeriods,
} from "./lib/stores/usage.js";
```

- [ ] **Step 2: Add offset state variable**

After `let period = $state(...)` add:

```typescript
let offset = $state(0);
```

- [ ] **Step 3: Add offset handlers**

Add these functions after `handlePeriodChange`:

```typescript
async function handleOffsetChange(delta: number) {
  const prov = provider;
  const per = period;
  offset += delta;
  activeOffset.set(offset);
  await fetchData(prov, per, offset);
  if (period !== per || provider !== prov) return;
  dataKey = `${prov}-${per}-${offset}-${Date.now()}`;
  await tick();
  syncSizeAndVerify();
  // Warm adjacent offsets for instant navigation
  warmCache(prov, per, offset - 1);
  if (offset < 0) warmCache(prov, per, offset + 1);
}

async function handleOffsetReset() {
  if (offset === 0) return;
  const prov = provider;
  const per = period;
  offset = 0;
  activeOffset.set(0);
  await fetchData(prov, per, 0);
  if (period !== per || provider !== prov) return;
  dataKey = `${prov}-${per}-0-${Date.now()}`;
  await tick();
  syncSizeAndVerify();
}
```

- [ ] **Step 4: Update handlePeriodChange to reset offset**

In `handlePeriodChange`, add offset reset at the top of the function (after `const prov = provider;`):

```typescript
async function handlePeriodChange(p: "5h" | "day" | "week" | "month" | "year") {
  const prov = provider;
  period = p;
  activePeriod.set(p);
  offset = 0;
  activeOffset.set(0);
  await fetchData(prov, p, 0);
  // Guard: if provider or period changed while we were fetching, bail out.
  if (period !== p || provider !== prov) return;
  dataKey = `${prov}-${p}-${Date.now()}`;
  await tick();
  syncSizeAndVerify();
}
```

- [ ] **Step 5: Update handleProviderChange to preserve offset**

In `handleProviderChange`, update the `fetchData` call to pass `offset`:

```typescript
await fetchData(p, period, offset);
```

Also update the `dataKey` assignment to include offset:

```typescript
dataKey = `${p}-${period}-${offset}-${Date.now()}`;
```

No changes needed to the `warmCache` / `warmAllPeriods` calls — they already use the default offset of 0, which is correct since switching periods resets offset.

- [ ] **Step 6: Update data-updated listener to pass offset**

In the `onMount` function, update the event listener:

```typescript
unlisten = await listen("data-updated", () => {
  dataKey = `${provider}-${period}-${offset}-${Date.now()}`;
  fetchData(provider, period, offset);
});
```

- [ ] **Step 7: Update init fetchData call to pass offset**

In the `onMount` init function, update:

```typescript
await fetchData(provider, period, offset);
```

- [ ] **Step 8: Add DateNav to the template**

In the template, after `<TimeTabs>` and before `<MetricsRow>`, add:

```svelte
<TimeTabs active={period} onChange={handlePeriodChange} />
{#if period !== "5h" && data}
  <DateNav
    periodLabel={data.period_label}
    hasEarlierData={data.has_earlier_data}
    isAtPresent={offset === 0}
    onBack={() => handleOffsetChange(-1)}
    onForward={() => handleOffsetChange(1)}
    onReset={handleOffsetReset}
  />
{/if}
<MetricsRow {data} />
```

- [ ] **Step 9: Add empty-period message**

In the template, update the chart rendering block. Replace:

```svelte
{#if period === "5h"}
  <UsageBars {data} />
{:else if data.chart_buckets.length > 0}
  <Chart buckets={data.chart_buckets} {dataKey} />
{/if}
```

With:

```svelte
{#if period === "5h"}
  <UsageBars {data} />
{:else if data.total_cost === 0 && data.total_tokens === 0}
  <div class="empty-period">No usage data for this period</div>
{:else}
  <Chart buckets={data.chart_buckets} {dataKey} />
{/if}
```

- [ ] **Step 10: Add empty-period style**

In the `<style>` section of `src/App.svelte`, add:

```css
.empty-period {
  text-align: center;
  color: var(--t3);
  font: 400 10px/1 'Inter', sans-serif;
  padding: 32px 0;
}
```

- [ ] **Step 11: Verify frontend builds**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npm run build 2>&1 | tail -10`
Expected: build succeeds

- [ ] **Step 12: Commit**

```bash
git add src/App.svelte
git commit -m "feat(app): wire DateNav with offset navigation and empty-period state"
```

---

### Task 10: Manual smoke test

- [ ] **Step 1: Build and run the app**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npm run tauri dev 2>&1 &`

- [ ] **Step 2: Verify DateNav appears**

- Click the "Day" tab → should see `‹  March 16, 2026  ›` below tabs
- Click the "5H" tab → DateNav should be hidden
- Switch to "Week", "Month", "Year" → DateNav appears with appropriate labels

- [ ] **Step 3: Test navigation**

- On "Day" tab, click `‹` → label should change to "March 15, 2026", chart shows yesterday's data
- Click `‹` again → "March 14, 2026"
- Click `›` → back to "March 15, 2026"
- Click the date label → snaps back to "March 16, 2026" (today)
- `›` arrow should be disabled/dimmed at offset 0

- [ ] **Step 4: Test boundary**

- Keep clicking `‹` until you reach the earliest data → back arrow should disable
- Verify clicking the disabled back arrow does nothing

- [ ] **Step 5: Test provider switch preserves offset**

- Navigate to yesterday on "Day" tab
- Switch provider (Claude → Codex) → should still show yesterday

- [ ] **Step 6: Test period switch resets offset**

- Navigate to yesterday on "Day" tab
- Switch to "Week" tab → should show current week (offset 0)

- [ ] **Step 7: Stop dev server, commit if needed**

Kill the dev server. If any fixes were needed, commit them.
