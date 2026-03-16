# Calendar Heatmap with Plan Earnings

## Summary

Add a color-graded calendar heatmap panel to TokenMonitor that shows daily token usage intensity (GitHub contribution-graph style) and calculates how much "value" the user earned by exceeding their subscription plan cost.

## Requirements

1. **Calendar heatmap** — monthly grid of days, color intensity based on token usage activity
2. **Month navigation** — left/right arrows to browse previous months
3. **Plan selection** — user picks their subscription tier; "earned" = total spend − plan cost
4. **Minimal summary** — centered line showing total spent and earned amount
5. **Access** — calendar icon button in the footer, opens as a sliding panel (like Settings)

## Plan Tiers

Hardcoded per provider:

| Provider | Tier Name | Monthly Cost |
|----------|-----------|-------------|
| Claude | Pro | $20 |
| Claude | Max 5× | $100 |
| Claude | Max 20× | $200 |
| Codex | Plus | $20 |
| Codex | Pro | $200 |

When provider is set to "all", plans are not applicable — show total spend without the earned calculation.

## Architecture

### New Files

- `src/lib/components/Calendar.svelte` — the calendar panel component

### Modified Files

| File | Change |
|------|--------|
| `src/App.svelte` | Add `showCalendar` state, `Calendar` import, `handleCalendarOpen`/`handleCalendarClose` handlers (both call `syncSizeAndVerify()`), render `Calendar` as a new `{:else if showCalendar}` branch alongside `showSettings` (mutually exclusive — opening calendar closes settings and vice versa), pass `onCalendar={handleCalendarOpen}` to Footer |
| `src/lib/components/Footer.svelte` | Add `onCalendar: () => void` to `Props` interface (alongside existing `onSettings`), add calendar icon button next to the settings gear in the `ft2` row |
| `src/lib/stores/settings.ts` | Add `claudePlan` and `codexPlan` fields to `Settings` interface and `DEFAULTS` |
| `src/lib/components/Settings.svelte` | Add "Plan" section with per-provider tier selection using `SegmentedControl` |
| `src/lib/types/index.ts` | Add `CalendarDay` and `MonthlyUsagePayload` TypeScript interfaces |
| `src-tauri/src/models.rs` | Add `CalendarDay` and `MonthlyUsagePayload` Rust structs with `Serialize` derives |
| `src-tauri/src/commands.rs` | Add `get_monthly_usage` Tauri command that returns per-day cost data for any given month |
| `src-tauri/src/lib.rs` | Register the `get_monthly_usage` command |

### Data Flow

1. User taps calendar icon in footer → `App.svelte` sets `showCalendar = true`
2. `Calendar.svelte` mounts, reads the active provider from `activeProvider` store
3. Calls `get_monthly_usage(provider, year, month)` via Tauri IPC
4. Backend parses JSONL logs for the requested month, returns an array of `{ day: number, cost: f64 }` entries
5. Frontend computes heatmap intensity levels and earned amount
6. User can navigate months with `‹` / `›` arrows (triggers new IPC call)
7. Plan tier is read from settings store; can be quick-changed via the `SegmentedControl` in the calendar panel (which also persists the change to the store)

### New Types

```typescript
// src/lib/types/index.ts
export interface CalendarDay {
  day: number;    // 1-31
  cost: number;   // USD cost for that day
}

export interface MonthlyUsagePayload {
  year: number;
  month: number;  // 1-12
  days: CalendarDay[];
  total_cost: number;
}
```

### New Rust Structs

```rust
// src-tauri/src/models.rs
#[derive(Debug, Clone, Serialize)]
pub struct CalendarDay {
    pub day: u32,     // 1-31
    pub cost: f64,    // USD cost for that day
}

#[derive(Debug, Clone, Serialize)]
pub struct MonthlyUsagePayload {
    pub year: i32,
    pub month: u32,   // 1-12
    pub days: Vec<CalendarDay>,
    pub total_cost: f64,
}
```

### New Rust Command

```rust
// src-tauri/src/commands.rs
#[tauri::command]
pub async fn get_monthly_usage(
    provider: String,
    year: i32,
    month: u32,
    state: State<'_, AppState>,
) -> Result<MonthlyUsagePayload, String>
```

**Implementation approach**: The command handler constructs the month's start date (YYYYMMDD) and calls the existing `parser.get_daily(provider, &month_start)` which returns a `UsagePayload` with `ChartBucket` entries. The handler then maps `chart_buckets` into `CalendarDay` entries by extracting the day number from each bucket's `label` (e.g. "Mar 5" → day 5) and using the bucket's `total` as the cost. Since `get_daily()` returns data from the start date through today, the handler filters out any buckets whose date falls outside the requested month (relevant when querying past months — `get_daily` from March 1 would include data through today, so for a February query we'd call `get_daily` with Feb 1 start and filter to only Feb days). The `total_cost` field is the sum of all filtered day costs. For "all" provider, it fetches both Claude and Codex, merges per-day costs by day number, and sums totals.

### Settings Changes

```typescript
// Added to Settings interface
claudePlan: number;  // 0 (none), 20, 100, 200
codexPlan: number;   // 0 (none), 20, 200
```

Defaults: `claudePlan: 0`, `codexPlan: 0` (no plan selected = no earned calculation shown).

The `SegmentedControl` component works with string values. Plan handlers use `parseInt()` to convert:
- SegmentedControl values: `"0"`, `"20"`, `"100"`, `"200"` (strings)
- Handler: `(val: string) => updateSetting("claudePlan", parseInt(val))` (stored as number)
- This matches the existing pattern in `handleRefresh` which does `parseInt(val)` on the refresh interval SegmentedControl.

Settings panel gets a new `.group` div inside the existing `.scroll` container, positioned between the "General" and "Monitoring" groups:

```
── Plan ──────────────────────────────────
Claude Plan    [ None | $20 | $100 | $200 ]
Codex Plan     [ None | $20 | $200 ]
```

Uses the existing `SegmentedControl` component.

## Calendar Panel Design

### Layout (top to bottom)

1. **Header**: `‹  March 2026  ›` — back arrow on far left navigates to previous month, forward arrow on far right. Month/year centered. Both arrows are `var(--t3)` color, hover to `var(--t2)`.

2. **Plan selector** (only when a single provider is active + plan > 0): A `SegmentedControl` showing available tiers for the active provider. For Claude: `[ None | $20 | $100 | $200 ]`. For Codex: `[ None | $20 | $200 ]`. Changing the plan here also updates the setting in the store (same `updateSetting` call as Settings panel). Hidden when provider is "all".

3. **Day-of-week headers**: Single row `M T W T F S S`, font 9px, `var(--t4)` color.

4. **Heatmap grid**: 7-column CSS grid, `gap: 3px`. Each cell:
   - `aspect-ratio: 1` (square)
   - `border-radius: 3px`
   - Day number in center, font 9px
   - Background color = heatmap intensity (see Color Logic below)
   - Future days use `var(--surface-2)` background, `var(--t4)` text

5. **Summary line**: Centered below the grid, separated by a 1px `var(--border-subtle)` top border.
   - Small label: "MONTHLY USAGE" in uppercase 10px, `var(--t4)`, letter-spacing 0.8px
   - Below: `$347.22 · +$147.22` — total in `var(--t1)` and earned in green (`#4daf4a`), separated by a `var(--t3)` dot
   - When earned is negative (under plan): show as `−$52.78 remaining` in `var(--t3)` instead of green
   - When no plan selected or provider is "all": show only `$347.22` total, no earned figure

### Color Logic

5 intensity levels based on that month's maximum daily spend:

| Level | Condition | Opacity |
|-------|-----------|---------|
| 0 (empty) | $0 spend | `var(--surface-2)` (no color) |
| 1 | 1–25% of max | 0.15 |
| 2 | 26–50% of max | 0.40 |
| 3 | 51–75% of max | 0.65 |
| 4 | 76–100% of max | 0.90 |

**Color source** (depends on `brandTheming` setting):
- Brand theming ON + Claude active → `rgba(196, 112, 75, opacity)` (terracotta, `--opus`)
- Brand theming ON + Codex active → `rgba(74, 123, 157, opacity)` (blue, `--gpt54`)
- Brand theming OFF or provider "all" → `rgba(77, 175, 74, opacity)` (GitHub green)

### Panel Behavior

- Slides in from the right with `slideIn` animation (same as Settings: `translateX(12px) → 0`). Note: this `@keyframes slideIn` must be duplicated in Calendar's scoped `<style>` since Svelte scopes styles per component.
- Has a back button (chevron + "Calendar" text) matching Settings header pattern
- Fixed height of 460px (same as Settings) with internal scroll if needed
- Closing returns to the main dashboard view

### Month Navigation

- `‹` and `›` arrows navigate months
- Cannot navigate past the current month (right arrow disabled/hidden when viewing current month)
- Can navigate backwards freely. If a month has no data, the grid renders empty (all cells at level 0). The left arrow is always enabled — no need to detect the earliest data boundary since empty months are a clear signal to stop.
- Loading state: set grid cells to `opacity: 0.3` with a brief fade transition while fetching. No spinner — the grid structure stays visible, just dims momentarily.
- Month/year label format: "March 2026" (full month name)

## Footer Changes

The second footer row (`ft2`) currently has:
```
[timestamp]                    [gear icon]
```

After this change:
```
[timestamp]           [calendar icon] [gear icon]
```

Calendar icon: 12×12px SVG, same styling as the gear (color `var(--t4)`, hover `var(--t2)`). A simple calendar outline icon.

## Edge Cases

- **No data for a month**: Grid renders with all cells at level 0 (empty). Summary shows "$0.00".
- **Provider "all"**: Calendar works but plan pill is hidden. No earned calculation. Heatmap uses green.
- **Plan set to "None" (0)**: Calendar works, heatmap renders normally. Summary shows total only, no earned figure.
- **Partial month** (current month): Future days render with `var(--surface-2)` background and dimmed text. Only past/today days participate in max-spend calculation for intensity levels.
- **Currency conversion**: All displayed amounts (including plan costs) go through the existing `formatCost()` utility which handles the user's selected currency. The plan cost used in the earned calculation is also converted, so the math stays consistent. Plan tier labels in the SegmentedControl always show USD values since those are the canonical subscription prices.

## Testing

- Unit tests for intensity level calculation (given costs array → intensity levels)
- Unit tests for earned calculation (plan cost, total spend → earned amount)
- Rust test for `get_monthly_usage` command (parse test fixtures, verify per-day aggregation)
- Integration test: verify Settings store persists `claudePlan` / `codexPlan` values
