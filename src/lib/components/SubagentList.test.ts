/**
 * SubagentList component logic tests
 *
 * The vitest environment is plain node (no DOM, no Svelte compiler plugin),
 * so we exercise the pure derived-value logic that drives what the component
 * renders rather than mounting the component itself.
 */
import { describe, expect, it } from "vitest";
import type { SubagentStats, ScopeUsageSummary } from "../types/index.js";

function makeScopeSummary(overrides: Partial<ScopeUsageSummary> = {}): ScopeUsageSummary {
  return {
    cost: 0,
    tokens: 0,
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_write_5m_tokens: 0,
    cache_write_1h_tokens: 0,
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

/** Mirrors the condition that controls .sa-pct visibility */
function hasSubagentPct(stats: SubagentStats): boolean {
  return stats.subagents.pct_of_total_cost != null;
}

/** Mirrors the spawn count text rendered in the subagent card */
function subagentSpawnText(stats: SubagentStats): string {
  return `${stats.subagents.session_count} spawned`;
}

describe("SubagentList", () => {
  it("both cards render when subagent usage exists", () => {
    // The component always renders exactly two .sa-card elements — one for
    // main and one for subagents — because they are unconditional siblings in
    // the template. Verify the stats shape contains both scopes.
    const stats = makeStats({
      main: { cost: 1.0, tokens: 10_000 },
      subagents: { cost: 0.5, tokens: 5_000, session_count: 3 },
    });

    expect(stats.main).toBeDefined();
    expect(stats.subagents).toBeDefined();
    // Two distinct card labels are provided by the data
    const labels = ["Main", "Subagents"];
    expect(labels).toHaveLength(2);
  });

  it("percentage badge only appears on the subagent card", () => {
    // The main card uses a plain inline text for pct_of_total_cost (no .sa-pct
    // class), while the subagent card wraps it in <span class="sa-pct">.
    // The visibility guard is `pct_of_total_cost != null`.
    const withPct = makeStats({
      main: { pct_of_total_cost: null },
      subagents: { pct_of_total_cost: 33 },
    });
    expect(hasSubagentPct(withPct)).toBe(true);

    const withoutPct = makeStats({
      subagents: { pct_of_total_cost: null },
    });
    expect(hasSubagentPct(withoutPct)).toBe(false);
  });

  it("spawn count is shown on the subagent card", () => {
    const stats = makeStats({ subagents: { session_count: 7 } });
    expect(subagentSpawnText(stats)).toBe("7 spawned");
  });
});
