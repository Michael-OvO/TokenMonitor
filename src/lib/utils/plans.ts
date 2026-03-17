import type { UsageProvider } from "../types/index.js";

/**
 * Canonical plan tier strings emitted by the Rust backend.
 *
 * Claude tiers come from `format_claude_plan_tier()` in rate_limits.rs.
 * Codex  tiers come from the `plan_type` match arm in rate_limits.rs.
 */
export type ClaudePlanTier = "Pro" | "Max 5x" | "Max 20x" | "Free";
export type CodexPlanTier  = "Plus" | "Pro" | "Free";
export type PlanTier       = ClaudePlanTier | CodexPlanTier;

/** Monthly subscription cost in USD — Claude */
const CLAUDE_PLAN_COSTS: Record<ClaudePlanTier, number> = {
  "Pro":     20,
  "Max 5x":  100,
  "Max 20x": 200,
  "Free":    0,
};

/** Monthly subscription cost in USD — Codex (OpenAI) */
const CODEX_PLAN_COSTS: Record<CodexPlanTier, number> = {
  "Plus": 20,
  "Pro":  200,
  "Free": 0,
};

/**
 * Returns the monthly subscription cost (USD) for a given plan tier and provider.
 * Returns 0 if the tier is unknown or the provider is "all".
 */
export function planTierCost(tier: string | null, provider: UsageProvider): number {
  if (!tier || provider === "all") return 0;
  if (provider === "claude") return CLAUDE_PLAN_COSTS[tier as ClaudePlanTier] ?? 0;
  if (provider === "codex")  return CODEX_PLAN_COSTS[tier as CodexPlanTier] ?? 0;
  return 0;
}
