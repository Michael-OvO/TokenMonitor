import { describe, it, expect } from "vitest";
import { planTierCost } from "./plans.js";

describe("planTierCost", () => {
  it("returns the correct cost for each Claude plan tier", () => {
    expect(planTierCost("Pro", "claude")).toBe(20);
    expect(planTierCost("Max 5x", "claude")).toBe(100);
    expect(planTierCost("Max 20x", "claude")).toBe(200);
    expect(planTierCost("Free", "claude")).toBe(0);
  });

  it("returns the correct cost for each Codex plan tier", () => {
    expect(planTierCost("Plus", "codex")).toBe(20);
    expect(planTierCost("Pro", "codex")).toBe(200);
    expect(planTierCost("Free", "codex")).toBe(0);
  });

  it("returns 0 for null tier", () => {
    expect(planTierCost(null, "claude")).toBe(0);
    expect(planTierCost(null, "codex")).toBe(0);
  });

  it("returns 0 for unknown tier strings", () => {
    expect(planTierCost("Enterprise", "claude")).toBe(0);
    expect(planTierCost("Enterprise", "codex")).toBe(0);
  });

  it("returns 0 when provider is 'all'", () => {
    expect(planTierCost("Pro", "all")).toBe(0);
    expect(planTierCost("Max 5x", "all")).toBe(0);
  });
});
