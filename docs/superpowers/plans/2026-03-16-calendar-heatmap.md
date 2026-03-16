# Calendar Heatmap with Plan Earnings — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a color-graded calendar heatmap panel with subscription plan "earned value" tracking.

**Architecture:** New `Calendar.svelte` panel accessed via footer icon, backed by a new `get_monthly_usage` Rust command. Plan tiers stored in settings. Heatmap uses provider accent colors with 5 intensity levels.

**Tech Stack:** Svelte 5, TypeScript, Rust/Tauri 2, Vitest (frontend tests), `cargo test` (backend tests)

**Spec:** `docs/superpowers/specs/2026-03-16-calendar-heatmap-design.md`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src-tauri/src/models.rs` | Add `CalendarDay` and `MonthlyUsagePayload` Rust structs |
| `src-tauri/src/commands.rs` | Add `get_monthly_usage` command handler |
| `src-tauri/src/lib.rs` | Register new command |
| `src/lib/types/index.ts` | Add `CalendarDay` and `MonthlyUsagePayload` TS interfaces |
| `src/lib/stores/settings.ts` | Add `claudePlan` and `codexPlan` fields |
| `src/lib/components/Settings.svelte` | Add Plan section with SegmentedControl |
| `src/lib/components/Footer.svelte` | Add calendar icon + `onCalendar` prop |
| `src/lib/calendar-utils.ts` | Pure functions: intensityLevel, computeEarned, heatmapColor (shared by component + tests) |
| `src/lib/components/Calendar.svelte` | New calendar panel (heatmap, navigation, plan selector, summary) |
| `src/App.svelte` | Wire calendar panel into view switcher |

---

## Chunk 1: Backend — Rust structs, command, and tests

### Task 1: Add Rust structs to models.rs

**Files:**
- Modify: `src-tauri/src/models.rs`

- [ ] **Step 1: Add CalendarDay and MonthlyUsagePayload structs**

Add after the `ActiveBlock` struct (after line 52):

```rust
#[derive(Debug, Serialize, Clone)]
pub struct CalendarDay {
    pub day: u32,
    pub cost: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct MonthlyUsagePayload {
    pub year: i32,
    pub month: u32,
    pub days: Vec<CalendarDay>,
    pub total_cost: f64,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/models.rs
git commit -m "feat(models): add CalendarDay and MonthlyUsagePayload structs"
```

---

### Task 2: Add get_monthly_usage command

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `commands.rs`:

```rust
#[test]
fn get_monthly_usage_returns_per_day_costs() {
    let claude_dir = TempDir::new().unwrap();
    let codex_dir = TempDir::new().unwrap();

    // Create a project dir with a JSONL file dated 2026-03-05
    let project_dir = claude_dir.path().join("test-project");
    fs::create_dir_all(&project_dir).unwrap();

    let content = r#"{"type":"assistant","timestamp":"2026-03-05T10:00:00-04:00","message":{"model":"claude-sonnet-4-6-20260301","usage":{"input_tokens":1000,"output_tokens":500},"stop_reason":"end_turn"}}"#;
    write_file(&project_dir.join("session.jsonl"), content);

    let parser = UsageParser::with_dirs(
        claude_dir.path().to_path_buf(),
        codex_dir.path().to_path_buf(),
    );
    let state = AppState {
        parser: Arc::new(parser),
        refresh_interval: Arc::new(RwLock::new(30)),
        show_tray_amount: Arc::new(RwLock::new(true)),
    };

    let payload = get_monthly_usage_sync(&state, "claude", 2026, 3);
    assert_eq!(payload.year, 2026);
    assert_eq!(payload.month, 3);
    assert!(!payload.days.is_empty(), "should have at least one day");

    let day5 = payload.days.iter().find(|d| d.day == 5);
    assert!(day5.is_some(), "should have data for day 5");
    assert!(day5.unwrap().cost > 0.0, "day 5 should have non-zero cost");
    assert!(payload.total_cost > 0.0);
}

#[test]
fn get_monthly_usage_filters_to_requested_month() {
    let claude_dir = TempDir::new().unwrap();
    let codex_dir = TempDir::new().unwrap();

    let project_dir = claude_dir.path().join("test-project");
    fs::create_dir_all(&project_dir).unwrap();

    // Write entries for Feb and Mar
    let feb_entry = r#"{"type":"assistant","timestamp":"2026-02-15T10:00:00-04:00","message":{"model":"claude-sonnet-4-6-20260301","usage":{"input_tokens":1000,"output_tokens":500},"stop_reason":"end_turn"}}"#;
    let mar_entry = r#"{"type":"assistant","timestamp":"2026-03-10T10:00:00-04:00","message":{"model":"claude-sonnet-4-6-20260301","usage":{"input_tokens":2000,"output_tokens":1000},"stop_reason":"end_turn"}}"#;
    write_file(
        &project_dir.join("session.jsonl"),
        &format!("{}\n{}", feb_entry, mar_entry),
    );

    let parser = UsageParser::with_dirs(
        claude_dir.path().to_path_buf(),
        codex_dir.path().to_path_buf(),
    );
    let state = AppState {
        parser: Arc::new(parser),
        refresh_interval: Arc::new(RwLock::new(30)),
        show_tray_amount: Arc::new(RwLock::new(true)),
    };

    // Request only February
    let payload = get_monthly_usage_sync(&state, "claude", 2026, 2);
    assert_eq!(payload.month, 2);
    // Should only contain Feb days, not March
    for day in &payload.days {
        assert!(day.day <= 28, "Feb 2026 has no day > 28");
    }
    assert!(payload.days.iter().any(|d| d.day == 15));
}

#[test]
fn get_monthly_usage_merges_providers_for_all() {
    let claude_dir = TempDir::new().unwrap();
    let codex_dir = TempDir::new().unwrap();

    // Claude entry on Mar 5
    let claude_project = claude_dir.path().join("test-project");
    fs::create_dir_all(&claude_project).unwrap();
    let claude_entry = r#"{"type":"assistant","timestamp":"2026-03-05T10:00:00-04:00","message":{"model":"claude-sonnet-4-6-20260301","usage":{"input_tokens":1000,"output_tokens":500},"stop_reason":"end_turn"}}"#;
    write_file(&claude_project.join("session.jsonl"), claude_entry);

    // Codex entry on Mar 5
    let day_dir = codex_dir.path().join("2026").join("03").join("05");
    fs::create_dir_all(&day_dir).unwrap();
    let codex_entry = r#"{"type":"event_msg","timestamp":"2026-03-05T14:00:00-04:00","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":500,"output_tokens":250,"reasoning_output_tokens":0,"cached_input_tokens":0}}}}"#;
    write_file(&day_dir.join("session.jsonl"), codex_entry);

    let parser = UsageParser::with_dirs(
        claude_dir.path().to_path_buf(),
        codex_dir.path().to_path_buf(),
    );
    let state = AppState {
        parser: Arc::new(parser),
        refresh_interval: Arc::new(RwLock::new(30)),
        show_tray_amount: Arc::new(RwLock::new(true)),
    };

    let payload = get_monthly_usage_sync(&state, "all", 2026, 3);
    let day5 = payload.days.iter().find(|d| d.day == 5);
    assert!(day5.is_some(), "should have merged day 5");
    // Both providers contribute — cost should be higher than either alone
    let claude_only = get_monthly_usage_sync(&state, "claude", 2026, 3);
    let codex_only = get_monthly_usage_sync(&state, "codex", 2026, 3);
    let claude_day5_cost = claude_only.days.iter().find(|d| d.day == 5).map(|d| d.cost).unwrap_or(0.0);
    let codex_day5_cost = codex_only.days.iter().find(|d| d.day == 5).map(|d| d.cost).unwrap_or(0.0);
    assert!(
        (day5.unwrap().cost - (claude_day5_cost + codex_day5_cost)).abs() < 0.001,
        "merged cost should equal sum of individual provider costs"
    );
}
```

- [ ] **Step 2: Add the sync helper and implement the command**

Add the synchronous helper (used by tests and the async command) and the Tauri command in `commands.rs`. Add above the `#[cfg(test)]` block:

```rust
use crate::models::*;

fn get_monthly_usage_sync(
    state: &AppState,
    provider: &str,
    year: i32,
    month: u32,
) -> MonthlyUsagePayload {
    let month_start = NaiveDate::from_ymd_opt(year, month, 1)
        .unwrap()
        .format("%Y%m%d")
        .to_string();

    let end_date = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    };

    let fetch_for_provider = |prov: &str| -> Vec<CalendarDay> {
        let usage = state.parser.get_daily(prov, &month_start);
        usage
            .chart_buckets
            .iter()
            .filter_map(|bucket| {
                // sort_key is "YYYY-MM-DD" format
                let date = NaiveDate::parse_from_str(&bucket.sort_key, "%Y-%m-%d").ok()?;
                if date >= NaiveDate::from_ymd_opt(year, month, 1).unwrap()
                    && date < end_date
                {
                    Some(CalendarDay {
                        day: date.day(),
                        cost: bucket.total,
                    })
                } else {
                    None
                }
            })
            .collect()
    };

    let days = match provider {
        "all" => {
            let claude_days = fetch_for_provider("claude");
            let codex_days = fetch_for_provider("codex");
            let mut day_map: HashMap<u32, f64> = HashMap::new();
            for d in claude_days.iter().chain(codex_days.iter()) {
                *day_map.entry(d.day).or_insert(0.0) += d.cost;
            }
            let mut merged: Vec<CalendarDay> = day_map
                .into_iter()
                .map(|(day, cost)| CalendarDay { day, cost })
                .collect();
            merged.sort_by_key(|d| d.day);
            merged
        }
        prov => fetch_for_provider(prov),
    };

    let total_cost: f64 = days.iter().map(|d| d.cost).sum();
    MonthlyUsagePayload {
        year,
        month,
        days,
        total_cost,
    }
}

#[tauri::command]
pub async fn get_monthly_usage(
    provider: String,
    year: i32,
    month: u32,
    state: State<'_, AppState>,
) -> Result<MonthlyUsagePayload, String> {
    Ok(get_monthly_usage_sync(&state, &provider, year, month))
}
```

Note: `models::*` is already imported via the existing `use crate::models::*;` at the top of `commands.rs`. The `Datelike` import is also already present. Just add the `use chrono::Datelike;` if not already there (check — it currently imports `Datelike` on line 3).

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri && cargo test -- get_monthly_usage`
Expected: all 3 new tests pass

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(commands): add get_monthly_usage command with tests"
```

---

### Task 3: Register the command in lib.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the command to the invoke_handler**

In `src-tauri/src/lib.rs`, line 88-93, add `commands::get_monthly_usage` to the handler list:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::get_usage_data,
            commands::get_monthly_usage,
            commands::set_refresh_interval,
            commands::set_show_tray_amount,
            commands::clear_cache,
        ])
```

- [ ] **Step 2: Verify full build**

Run: `cd src-tauri && cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(lib): register get_monthly_usage command"
```

---

## Chunk 2: Frontend — Types, settings, and plan UI

### Task 4: Add TypeScript interfaces

**Files:**
- Modify: `src/lib/types/index.ts`

- [ ] **Step 1: Add CalendarDay and MonthlyUsagePayload interfaces**

Add at the end of `src/lib/types/index.ts`:

```typescript
export interface CalendarDay {
  day: number;
  cost: number;
}

export interface MonthlyUsagePayload {
  year: number;
  month: number;
  days: CalendarDay[];
  total_cost: number;
}
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/types/index.ts
git commit -m "feat(types): add CalendarDay and MonthlyUsagePayload interfaces"
```

---

### Task 5: Add plan settings to store

**Files:**
- Modify: `src/lib/stores/settings.ts`

- [ ] **Step 1: Add claudePlan and codexPlan to the Settings interface**

In `src/lib/stores/settings.ts`, add after `showTrayAmount: boolean;` (line 15):

```typescript
  claudePlan: number;
  codexPlan: number;
```

- [ ] **Step 2: Add defaults**

In the `DEFAULTS` object, add after `showTrayAmount: true,` (line 28):

```typescript
  claudePlan: 0,
  codexPlan: 0,
```

- [ ] **Step 3: Verify TypeScript compiles**

Run: `npx tsc --noEmit`
Expected: no errors (existing code that spreads `DEFAULTS` will pick up the new fields)

- [ ] **Step 4: Commit**

```bash
git add src/lib/stores/settings.ts
git commit -m "feat(settings): add claudePlan and codexPlan fields"
```

---

### Task 6: Add Plan section to Settings panel

**Files:**
- Modify: `src/lib/components/Settings.svelte`

- [ ] **Step 1: Update the initial `current` state**

In the `current` state initialization (line 15-26), add the new fields after `showTrayAmount: true,` (line 25):

```typescript
    claudePlan: 0,
    codexPlan: 0,
```

This must be done first because after Task 5 adds the fields to the `Settings` interface, the `$state<SettingsType>({...})` object will fail TypeScript if the fields are missing.

- [ ] **Step 2: Add plan handlers**

In the `<script>` block of `Settings.svelte`, add after the `handleBrandTheming` function (after line 61):

```typescript
  function handleClaudePlan(val: string) {
    updateSetting("claudePlan", parseInt(val));
  }

  function handleCodexPlan(val: string) {
    updateSetting("codexPlan", parseInt(val));
  }
```

- [ ] **Step 3: Add the Plan group in the template**

In the template, add the Plan group between the General `</div>` (closing the General group, after line 226) and the Monitoring `<div class="group">` (line 229):

```svelte
    <!-- Plan -->
    <div class="group">
      <div class="group-label">Plan</div>
      <div class="card">
        <div class="row border">
          <span class="label">Claude Plan</span>
          <SegmentedControl
            options={[
              { value: "0", label: "None" },
              { value: "20", label: "$20" },
              { value: "100", label: "$100" },
              { value: "200", label: "$200" },
            ]}
            value={String(current.claudePlan)}
            onChange={handleClaudePlan}
          />
        </div>
        <div class="row">
          <span class="label">Codex Plan</span>
          <SegmentedControl
            options={[
              { value: "0", label: "None" },
              { value: "20", label: "$20" },
              { value: "200", label: "$200" },
            ]}
            value={String(current.codexPlan)}
            onChange={handleCodexPlan}
          />
        </div>
      </div>
    </div>
```

- [ ] **Step 4: Verify it renders**

Run: `npm run dev`
Open the app → Settings → verify the Plan section appears between General and Monitoring with both SegmentedControls.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/Settings.svelte
git commit -m "feat(settings): add Plan section with Claude and Codex tier selectors"
```

---

### Task 7: Add calendar icon to Footer

**Files:**
- Modify: `src/lib/components/Footer.svelte`

- [ ] **Step 1: Add onCalendar prop**

In the `Props` interface (line 6-8), add the new callback:

```typescript
  interface Props {
    data: UsagePayload;
    onSettings: () => void;
    onCalendar: () => void;
  }
  let { data, onSettings, onCalendar }: Props = $props();
```

- [ ] **Step 2: Wrap buttons in a container and add calendar icon**

The `ft2` div uses `justify-content: space-between` with 2 children. Adding a third child would push the calendar icon to the center instead of grouping it with the gear. Wrap both buttons in a container div.

Replace the gear button in the `ft2` div (line 43-48) with:

```svelte
  <div class="ft-actions">
    <button class="gear" onclick={onCalendar} aria-label="Calendar">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <rect x="3" y="4" width="18" height="18" rx="2" ry="2"></rect>
        <line x1="16" y1="2" x2="16" y2="6"></line>
        <line x1="8" y1="2" x2="8" y2="6"></line>
        <line x1="3" y1="10" x2="21" y2="10"></line>
      </svg>
    </button>
    <button class="gear" onclick={onSettings} aria-label="Settings">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="3"></circle>
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
      </svg>
    </button>
  </div>
```

Also add this CSS rule inside the `<style>` block:

```css
  .ft-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }
```

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/Footer.svelte
git commit -m "feat(footer): add calendar icon button"
```

---

## Chunk 3: Calendar utilities, component, and App integration

### Task 8: Create calendar-utils.ts

**Files:**
- Create: `src/lib/calendar-utils.ts`

- [ ] **Step 1: Create the utility module**

Create `src/lib/calendar-utils.ts`:

```typescript
export const INTENSITY_OPACITY = [0, 0.15, 0.40, 0.65, 0.90];

export function intensityLevel(cost: number, maxCost: number): number {
  if (maxCost === 0 || cost === 0) return 0;
  const ratio = cost / maxCost;
  if (ratio <= 0.25) return 1;
  if (ratio <= 0.50) return 2;
  if (ratio <= 0.75) return 3;
  return 4;
}

export function computeEarned(totalCost: number, planCost: number): number | null {
  if (planCost <= 0) return null;
  return totalCost - planCost;
}

export function heatmapColor(
  level: number,
  brandTheming: boolean,
  provider: "claude" | "codex" | "all" | string,
): string {
  if (level === 0) return "var(--surface-2)";
  const opacity = INTENSITY_OPACITY[level];
  if (brandTheming && provider === "claude") return `rgba(196, 112, 75, ${opacity})`;
  if (brandTheming && provider === "codex") return `rgba(74, 123, 157, ${opacity})`;
  return `rgba(77, 175, 74, ${opacity})`;
}
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/calendar-utils.ts
git commit -m "feat: add calendar heatmap utility functions"
```

---

### Task 9: Create Calendar.svelte

**Files:**
- Create: `src/lib/components/Calendar.svelte`

- [ ] **Step 1: Create the calendar component**

Create `src/lib/components/Calendar.svelte`:

```svelte
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { settings, updateSetting } from "../stores/settings.js";
  import { activeProvider } from "../stores/usage.js";
  import { formatCost } from "../utils/format.js";
  import SegmentedControl from "./SegmentedControl.svelte";
  import type { MonthlyUsagePayload } from "../types/index.js";

  interface Props {
    onBack: () => void;
  }

  let { onBack }: Props = $props();

  // Current month being viewed
  let viewYear = $state(new Date().getFullYear());
  let viewMonth = $state(new Date().getMonth() + 1); // 1-indexed

  let data = $state<MonthlyUsagePayload | null>(null);
  let loading = $state(false);
  let provider = $state<"all" | "claude" | "codex">("claude");
  let brandTheming = $state(true);
  let claudePlan = $state(0);
  let codexPlan = $state(0);

  // Subscribe to stores
  $effect(() => {
    const unsub1 = activeProvider.subscribe((p) => (provider = p));
    const unsub2 = settings.subscribe((s) => {
      brandTheming = s.brandTheming;
      claudePlan = s.claudePlan;
      codexPlan = s.codexPlan;
    });
    return () => { unsub1(); unsub2(); };
  });

  // Fetch data when month/year/provider changes
  $effect(() => {
    fetchMonth(provider, viewYear, viewMonth);
  });

  async function fetchMonth(prov: string, year: number, month: number) {
    loading = true;
    try {
      data = await invoke<MonthlyUsagePayload>("get_monthly_usage", {
        provider: prov,
        year,
        month,
      });
    } catch (e) {
      console.error("Failed to fetch monthly usage:", e);
      data = { year, month, days: [], total_cost: 0 };
    } finally {
      loading = false;
    }
  }

  function prevMonth() {
    if (viewMonth === 1) {
      viewYear -= 1;
      viewMonth = 12;
    } else {
      viewMonth -= 1;
    }
  }

  function nextMonth() {
    const now = new Date();
    if (viewYear === now.getFullYear() && viewMonth === now.getMonth() + 1) return;
    if (viewMonth === 12) {
      viewYear += 1;
      viewMonth = 1;
    } else {
      viewMonth += 1;
    }
  }

  // Plan helpers
  let activePlan = $derived(
    provider === "claude" ? claudePlan :
    provider === "codex" ? codexPlan : 0
  );

  let planOptions = $derived(
    provider === "claude"
      ? [
          { value: "0", label: "None" },
          { value: "20", label: "$20" },
          { value: "100", label: "$100" },
          { value: "200", label: "$200" },
        ]
      : provider === "codex"
      ? [
          { value: "0", label: "None" },
          { value: "20", label: "$20" },
          { value: "200", label: "$200" },
        ]
      : []
  );

  function handlePlanChange(val: string) {
    const num = parseInt(val);
    if (provider === "claude") updateSetting("claudePlan", num);
    else if (provider === "codex") updateSetting("codexPlan", num);
  }

  let earned = $derived(
    data ? computeEarned(data.total_cost, activePlan) : null
  );

  // Calendar grid helpers
  const MONTH_NAMES = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
  ];

  let monthLabel = $derived(`${MONTH_NAMES[viewMonth - 1]} ${viewYear}`);

  import {
    intensityLevel,
    computeEarned,
    heatmapColor,
    INTENSITY_OPACITY,
  } from "../calendar-utils.js";

  let isCurrentMonth = $derived.by(() => {
    const now = new Date();
    return viewYear === now.getFullYear() && viewMonth === now.getMonth() + 1;
  });

  let daysInMonth = $derived(new Date(viewYear, viewMonth, 0).getDate());

  // Monday = 0, ..., Sunday = 6
  let firstDayOffset = $derived.by(() => {
    const jsDay = new Date(viewYear, viewMonth - 1, 1).getDay(); // 0=Sun
    return jsDay === 0 ? 6 : jsDay - 1; // convert to Mon-start
  });

  // Build cost lookup from data
  let costByDay = $derived.by(() => {
    const map = new Map<number, number>();
    if (data) {
      for (const d of data.days) {
        map.set(d.day, d.cost);
      }
    }
    return map;
  });

  // Max daily spend (for intensity calculation) — only past/today days
  let maxDailyCost = $derived.by(() => {
    if (costByDay.size === 0) return 0;
    const now = new Date();
    const today = isCurrentMonth
      ? now.getDate()
      : daysInMonth;
    let max = 0;
    for (const [day, cost] of costByDay) {
      if (day <= today && cost > max) max = cost;
    }
    return max;
  });

  function isFutureDay(day: number): boolean {
    if (!isCurrentMonth) return false;
    return day > new Date().getDate();
  }
</script>

<div class="calendar">
  <!-- Header -->
  <div class="header">
    <button class="back" onclick={onBack}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="15 18 9 12 15 6"></polyline>
      </svg>
      <span>Calendar</span>
    </button>
  </div>

  <div class="scroll">
    <!-- Month navigation -->
    <div class="month-nav">
      <button class="nav-arrow" onclick={prevMonth}>‹</button>
      <span class="month-label">{monthLabel}</span>
      <button
        class="nav-arrow"
        class:disabled={isCurrentMonth}
        onclick={nextMonth}
        disabled={isCurrentMonth}
      >›</button>
    </div>

    <!-- Plan selector -->
    {#if provider !== "all" && planOptions.length > 0}
      <div class="plan-selector">
        <SegmentedControl
          options={planOptions}
          value={String(activePlan)}
          onChange={handlePlanChange}
        />
      </div>
    {/if}

    <!-- Day-of-week headers -->
    <div class="day-headers">
      {#each ["M", "T", "W", "T", "F", "S", "S"] as day}
        <span class="day-header">{day}</span>
      {/each}
    </div>

    <!-- Heatmap grid -->
    <div class="grid" class:loading>
      <!-- Empty cells for offset -->
      {#each Array(firstDayOffset) as _}
        <div class="cell empty"></div>
      {/each}

      {#each Array(daysInMonth) as _, i}
        {@const day = i + 1}
        {@const cost = costByDay.get(day) ?? 0}
        {@const future = isFutureDay(day)}
        {@const level = future ? 0 : intensityLevel(cost, maxDailyCost)}
        <div
          class="cell"
          class:future
          style:background={heatmapColor(level, brandTheming, provider)}
        >
          <span class="day-num">{day}</span>
        </div>
      {/each}
    </div>

    <!-- Summary -->
    <div class="summary">
      <div class="summary-label">MONTHLY USAGE</div>
      <div class="summary-values">
        <span class="summary-total">{formatCost(data?.total_cost ?? 0)}</span>
        {#if earned !== null}
          <span class="summary-dot">·</span>
          {#if earned >= 0}
            <span class="summary-earned positive">+{formatCost(earned)}</span>
          {:else}
            <span class="summary-earned negative">{formatCost(Math.abs(earned))} remaining</span>
          {/if}
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .calendar {
    animation: slideIn 0.22s cubic-bezier(.25,.8,.25,1) both;
    height: 460px;
    display: flex;
    flex-direction: column;
  }

  @keyframes slideIn {
    from { opacity: 0; transform: translateX(12px); }
    to { opacity: 1; transform: translateX(0); }
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px 6px;
  }

  .back {
    display: flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: none;
    cursor: pointer;
    color: var(--t1);
    font: 600 12px/1 'Inter', sans-serif;
    padding: 0;
  }
  .back:hover { color: var(--t2); }

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 10px 10px;
    scrollbar-width: none;
  }
  .scroll::-webkit-scrollbar { display: none; }

  .month-nav {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px 4px 8px;
  }

  .nav-arrow {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--t3);
    font-size: 16px;
    padding: 2px 6px;
    transition: color 0.15s ease;
  }
  .nav-arrow:hover:not(.disabled) { color: var(--t2); }
  .nav-arrow.disabled {
    opacity: 0.2;
    cursor: default;
  }

  .month-label {
    font: 600 13px/1 'Inter', sans-serif;
    color: var(--t1);
  }

  .plan-selector {
    display: flex;
    justify-content: center;
    padding: 0 0 10px;
  }

  .day-headers {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 3px;
    text-align: center;
    padding: 0 2px 4px;
  }

  .day-header {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t4);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 3px;
    padding: 0 2px;
    transition: opacity 0.15s ease;
  }
  .grid.loading { opacity: 0.3; }

  .cell {
    aspect-ratio: 1;
    border-radius: 3px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--surface-2);
  }
  .cell.empty {
    background: transparent;
  }
  .cell.future {
    background: var(--surface-2);
  }

  .day-num {
    font: 400 9px/1 'Inter', sans-serif;
    color: rgba(255, 255, 255, 0.5);
    font-variant-numeric: tabular-nums;
  }
  .cell.future .day-num {
    color: var(--t4);
  }

  .summary {
    border-top: 1px solid var(--border-subtle);
    padding-top: 14px;
    margin-top: 14px;
    text-align: center;
  }

  .summary-label {
    font: 500 10px/1 'Inter', sans-serif;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--t4);
    margin-bottom: 6px;
  }

  .summary-values {
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 6px;
  }

  .summary-total {
    font: 600 18px/1 'Inter', sans-serif;
    color: var(--t1);
    font-variant-numeric: tabular-nums;
  }

  .summary-dot {
    font: 400 11px/1 'Inter', sans-serif;
    color: var(--t3);
  }

  .summary-earned {
    font: 600 14px/1 'Inter', sans-serif;
    font-variant-numeric: tabular-nums;
  }
  .summary-earned.positive {
    color: #4daf4a;
  }
  .summary-earned.negative {
    color: var(--t3);
  }
</style>
```

- [ ] **Step 2: Verify TypeScript compiles**

Run: `npx tsc --noEmit`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/Calendar.svelte
git commit -m "feat: add Calendar heatmap component"
```

---

### Task 10: Wire Calendar into App.svelte

**Files:**
- Modify: `src/App.svelte`

- [ ] **Step 1: Add Calendar import**

Add after the Settings import (line 39):

```typescript
  import Calendar from "./lib/components/Calendar.svelte";
```

- [ ] **Step 2: Add showCalendar state**

Add after `let showSettings = $state(false);` (line 43):

```typescript
  let showCalendar = $state(false);
```

- [ ] **Step 3: Add calendar open/close handlers**

Add after `handleSettingsClose` (after line 116):

```typescript
  async function handleCalendarOpen() {
    showCalendar = true;
    await tick();
    syncSizeAndVerify();
  }

  async function handleCalendarClose() {
    showCalendar = false;
    await tick();
    syncSizeAndVerify();
  }
```

- [ ] **Step 4: Add Calendar branch to the template**

In the template, add after the `{:else if showSettings}` / `<Settings onBack={handleSettingsClose} />` block (after the Settings branch, currently around line 293-294):

```svelte
    {:else if showCalendar}
      <Calendar onBack={handleCalendarClose} />
```

- [ ] **Step 5: Pass onCalendar to Footer**

Update the Footer component call (currently around line 312) to include the new prop:

```svelte
      <Footer {data} onSettings={handleSettingsOpen} onCalendar={handleCalendarOpen} />
```

- [ ] **Step 6: Ensure mutual exclusivity — update handleSettingsOpen**

Modify `handleSettingsOpen` (line 106-110) to also close the calendar:

```typescript
  async function handleSettingsOpen() {
    showCalendar = false;
    showSettings = true;
    await tick();
    syncSizeAndVerify();
  }
```

And modify `handleCalendarOpen` to close settings:

```typescript
  async function handleCalendarOpen() {
    showSettings = false;
    showCalendar = true;
    await tick();
    syncSizeAndVerify();
  }
```

- [ ] **Step 7: Verify full app renders**

Run: `npm run dev`
Open app → click calendar icon in footer → verify the calendar panel opens. Click back → returns to dashboard. Click settings gear → verify settings still work.

- [ ] **Step 8: Commit**

```bash
git add src/App.svelte
git commit -m "feat: wire Calendar panel into App view switcher"
```

---

## Chunk 4: Frontend tests

### Task 11: Unit tests for calendar logic

**Files:**
- Create: `src/lib/calendar.test.ts`

- [ ] **Step 1: Write tests for intensity level calculation**

Create `src/lib/calendar.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { intensityLevel, computeEarned, heatmapColor } from "./calendar-utils.js";

// ── intensityLevel ──

describe("intensityLevel", () => {
  it("returns 0 for zero cost", () => {
    expect(intensityLevel(0, 100)).toBe(0);
  });

  it("returns 0 when max is zero", () => {
    expect(intensityLevel(50, 0)).toBe(0);
  });

  it("returns 1 for 1-25% of max", () => {
    expect(intensityLevel(25, 100)).toBe(1);
    expect(intensityLevel(1, 100)).toBe(1);
  });

  it("returns 2 for 26-50% of max", () => {
    expect(intensityLevel(50, 100)).toBe(2);
    expect(intensityLevel(26, 100)).toBe(2);
  });

  it("returns 3 for 51-75% of max", () => {
    expect(intensityLevel(75, 100)).toBe(3);
    expect(intensityLevel(51, 100)).toBe(3);
  });

  it("returns 4 for 76-100% of max", () => {
    expect(intensityLevel(100, 100)).toBe(4);
    expect(intensityLevel(76, 100)).toBe(4);
  });
});

// ── computeEarned ──

describe("computeEarned", () => {
  it("returns null when plan is 0", () => {
    expect(computeEarned(347, 0)).toBeNull();
  });

  it("returns positive when spend exceeds plan", () => {
    expect(computeEarned(347, 200)).toBe(147);
  });

  it("returns negative when spend is under plan", () => {
    expect(computeEarned(15, 20)).toBe(-5);
  });

  it("returns 0 when spend equals plan", () => {
    expect(computeEarned(200, 200)).toBe(0);
  });
});

// ── heatmapColor ──

describe("heatmapColor", () => {
  it("returns surface-2 for level 0", () => {
    expect(heatmapColor(0, true, "claude")).toBe("var(--surface-2)");
  });

  it("returns terracotta for Claude with brand theming", () => {
    expect(heatmapColor(4, true, "claude")).toBe("rgba(196, 112, 75, 0.9)");
  });

  it("returns blue for Codex with brand theming", () => {
    expect(heatmapColor(2, true, "codex")).toBe("rgba(74, 123, 157, 0.4)");
  });

  it("returns green when brand theming is off", () => {
    expect(heatmapColor(3, false, "claude")).toBe("rgba(77, 175, 74, 0.65)");
  });

  it("returns green for 'all' provider regardless of brand theming", () => {
    expect(heatmapColor(1, true, "all")).toBe("rgba(77, 175, 74, 0.15)");
  });
});
```

- [ ] **Step 2: Run the tests**

Run: `npx vitest run src/lib/calendar.test.ts`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add src/lib/calendar.test.ts
git commit -m "test: add calendar heatmap intensity and earned calculation tests"
```

---

### Task 12: Run all tests to verify nothing is broken

- [ ] **Step 1: Run all frontend tests**

Run: `npx vitest run`
Expected: all tests pass (existing + new calendar tests)

- [ ] **Step 2: Run all Rust tests**

Run: `cd src-tauri && cargo test`
Expected: all tests pass (existing + new get_monthly_usage tests)

- [ ] **Step 3: Final commit if any cleanup needed**

If tests revealed any issues, fix and commit. Otherwise, no action needed.
