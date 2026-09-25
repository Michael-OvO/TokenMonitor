<script lang="ts">
  import {
    formatRateLimitUtilizationLabel,
    getRateLimitIdleSummary,
    isRateLimitProvider,
  } from "../providerMetadata.js";
  import { formatCostRange, formatCreditAmount, formatDuration, formatRetryIn } from "../utils/format.js";
  import {
    currentRateLimitWindows,
    providerHasActiveCooldown,
    providerRateLimitViewState,
    rateLimitWindowResetLabel,
  } from "../views/rateLimits.js";
  import type { PlanBudget, ProviderRateLimits, RateLimitWindow } from "../types/index.js";
  import { invoke } from "@tauri-apps/api/core";
  import { untrack } from "svelte";
  import { budgetAsks, budgetRange, windowSpan, type BudgetAsk } from "../views/planBudget.js";
  import { popoverVisible } from "../visibility.js";

  interface Props {
    providerLabel?: string;
    rateLimits: ProviderRateLimits;
  }
  let { providerLabel, rateLimits }: Props = $props();

  // Plan budget per provider and windowId; refetched when a new reading
  // arrives, not each time the page sets the same one again. Keyed by provider
  // too: providers share window ids ("five_hour"), and this instance may be
  // handed another provider's limits.
  let budgets = $state<Record<string, PlanBudget>>({});
  const budgetKey = (provider: string, windowId: string) => `${provider}:${windowId}`;
  let asks = $derived(budgetAsks(rateLimits));
  $effect(() => {
    const { provider, asks: bars } = JSON.parse(asks) as { provider: string; asks: BudgetAsk[] };
    let cancelled = false;
    for (const { windowId, since } of bars) {
      const key = budgetKey(provider, windowId);
      invoke<PlanBudget | null>("get_plan_budget", { provider, windowId, since })
        .then((b) => { if (b && !cancelled) budgets = { ...budgets, [key]: b }; })
        .catch(() => {});
    }
    return () => { cancelled = true; };
  });

  function budgetFor(w: RateLimitWindow): PlanBudget | undefined {
    return budgets[budgetKey(rateLimits.provider, w.windowId)];
  }

  function budgetLabel(w: RateLimitWindow): string {
    const span = windowSpan(rateLimits.provider, w);
    const range = span && budgetRange(rateLimits.provider, w, budgetFor(w), rateLimits.windows);
    return range ? `${formatCostRange(...range)} / ${span.unit}` : "";
  }

  // "Opus 4.1 $900 · Sonnet 4.5 $2,500" when per-model budgets are known.
  function budgetTitle(w: RateLimitWindow): string {
    if (w.budgetUsd) return "Included in the plan";
    const models = budgetFor(w)?.models ?? [];
    if (models.length === 0) return "Plan budget at API prices (rough until more usage is recorded)";
    return models.map((m) => `${m.model} ${formatCostRange(m.usd, m.usd)}`).join(" · ");
  }

  // Refresh "Resets in" + pace every 30s while shown, and at once on show.
  let refreshTick = $state(0);
  $effect(() => {
    if (!$popoverVisible) return;
    refreshTick = untrack(() => refreshTick) + 1;
    const interval = setInterval(() => { refreshTick += 1; }, 30_000);
    return () => clearInterval(interval);
  });

  let visibleWindows = $derived.by(() => {
    void refreshTick;
    return currentRateLimitWindows(rateLimits, Date.now());
  });
  let viewState = $derived.by(() => {
    void refreshTick;
    return providerRateLimitViewState(rateLimits, Date.now());
  });
  let hasActiveCooldown = $derived.by(() => {
    void refreshTick;
    return providerHasActiveCooldown(rateLimits, Date.now());
  });

  function utilizationColor(pct: number): string {
    // Use the provider accent for everything; only signal severity through
    // opacity so the bar reads as one calm color family instead of a
    // stoplight. The very-high band shifts to a desaturated alert tone.
    if (pct >= 90) return "var(--alert, #B05A52)";
    return "var(--accent)";
  }

  function utilizationFillOpacity(pct: number): number {
    if (pct >= 90) return 0.95;
    if (pct >= 50) return 0.85;
    return 0.7;
  }

  function resetsIn(isoString: string | null): string {
    void refreshTick;
    return rateLimitWindowResetLabel(rateLimits, isoString);
  }

  function paceDelta(w: RateLimitWindow, windowHours: number): number | null {
    void refreshTick;
    if (!w.resetsAt) return null;
    const resetMs = new Date(w.resetsAt).getTime();
    const now = Date.now();
    const remainingMs = resetMs - now;
    if (remainingMs <= 0) return null;
    const totalMs = windowHours * 3_600_000;
    const elapsedMs = totalMs - remainingMs;
    if (elapsedMs <= 0) return null;
    return (elapsedMs / totalMs) * 100 - w.utilization;
  }

  function paceLabel(w: RateLimitWindow, windowHours: number): string {
    const delta = paceDelta(w, windowHours);
    if (delta === null) return "";
    const abs = Math.abs(Math.round(delta));
    if (abs < 2) return "on pace";
    if (delta > 0) return `${abs}% under`;
    return `${abs}% over`;
  }

  function etaToLimit(w: RateLimitWindow, windowHours: number): string {
    void refreshTick;
    if (w.utilization <= 0 || w.utilization >= 95) return "";
    if (!w.resetsAt) return "";
    const resetMs = new Date(w.resetsAt).getTime();
    const now = Date.now();
    const remainingMs = resetMs - now;
    if (remainingMs <= 0) return "";
    const totalMs = windowHours * 3_600_000;
    const elapsedMs = totalMs - remainingMs;
    if (elapsedMs < 60_000) return ""; // need at least 1 min of history
    const etaMs = ((100 - w.utilization) * elapsedMs) / w.utilization;
    // Only warn if you'll exhaust before the window resets
    if (etaMs >= remainingMs || etaMs < 300_000) return "";
    return `limit in ~${formatDuration(etaMs)}`;
  }

  function paceColor(w: RateLimitWindow, windowHours: number): string {
    const delta = paceDelta(w, windowHours);
    if (delta === null || Math.abs(delta) < 2) return "var(--t3)";
    // Pace badges share the same calm palette as the bars themselves —
    // no traffic-light yellow.
    return "var(--t2)";
  }

  // 0 = unknown length, which turns pace and ETA off instead of guessing.
  function windowHours(w: RateLimitWindow): number {
    return (windowSpan(rateLimits.provider, w)?.minutes ?? 0) / 60;
  }

  function utilizationLabel(pct: number): string {
    if (isRateLimitProvider(rateLimits.provider)) {
      return formatRateLimitUtilizationLabel(rateLimits.provider, pct);
    }
    return `${pct}%`;
  }

  function emptySummary(): string {
    void refreshTick;
    const retryLabel = formatRetryIn(rateLimits.cooldownUntil);
    if (viewState === "error") {
      const base = rateLimits.error ?? "Unable to load rate limits right now.";
      return retryLabel ? `${base} ${retryLabel}.` : base;
    }
    if (viewState === "idle") {
      if (isRateLimitProvider(rateLimits.provider)) {
        return getRateLimitIdleSummary(rateLimits.provider);
      }
      return "Usage is being recorded, but this integration has not emitted rate-limit metadata yet.";
    }
    return "No active rate limit windows were returned for this provider.";
  }
</script>

<div class="ub">
  {#if providerLabel}
    <div class="ub-provider">
      <span class="ub-provider-name">{providerLabel}</span>
      {#if rateLimits.planTier}
        <span class="ub-plan">{rateLimits.planTier}</span>
      {/if}
    </div>
  {/if}

  {#if viewState === "ready"}
    {#each visibleWindows as w, i}
      {@const hours = windowHours(w)}
      {@const budget = budgetLabel(w)}
      {@const pace = paceLabel(w, hours)}
      {@const eta = etaToLimit(w, hours)}
      <div class="ub-row">
        <div class="ub-head">
          <span class="ub-label">{w.label}</span>
          <div class="ub-head-right">
            {#if pace}
              <span class="ub-pace-badge" style="color: {paceColor(w, hours)}">{pace}</span>
            {/if}
            <span class="ub-val" class:stale={rateLimits.stale}>{utilizationLabel(w.utilization)}</span>
          </div>
        </div>
        <div class="ub-track">
          <div
            class="ub-fill"
            style="width: {Math.min(w.utilization, 100)}%; background: {utilizationColor(w.utilization)}; opacity: {utilizationFillOpacity(w.utilization)}; --bar-delay: {i * 0.09 + 0.04}s;"
          ></div>
        </div>
        <div class="ub-sub">
          {#if eta}
            <span class="ub-eta" style="color: {utilizationColor(w.utilization)}">{eta}</span>
            <span class="ub-eta-reset"> · {resetsIn(w.resetsAt)}</span>
          {:else}
            {resetsIn(w.resetsAt)}
          {/if}
          {#if budget}
            <span title={budgetTitle(w)}>· {budget}</span>
          {/if}
        </div>
      </div>
    {/each}
  {:else}
    <div class="ub-empty" class:error={viewState === "error"}>
      <div class="ub-empty-title">
        {#if viewState === "error" && hasActiveCooldown}
          Rate-limited
        {:else if viewState === "error"}
          Rate limits unavailable
        {:else if viewState === "idle"}
          No current usage
        {:else}
          No rate limit data
        {/if}
      </div>
      <div class="ub-empty-text">{emptySummary()}</div>
    </div>
  {/if}

  {#if rateLimits.extraUsage?.isEnabled}
    <div class="ub-row">
      <div class="ub-head">
        <span class="ub-label">{rateLimits.provider === "cursor" ? "On-demand" : "Extra Usage"}</span>
        <span class="ub-val">{formatCreditAmount(rateLimits.extraUsage.usedCredits)} / {formatCreditAmount(rateLimits.extraUsage.monthlyLimit)}</span>
      </div>
      <div class="ub-track">
        <div
          class="ub-fill"
          style="width: {Math.min((rateLimits.extraUsage.utilization ?? 0), 100)}%; background: {utilizationColor(rateLimits.extraUsage.utilization ?? 0)}; opacity: {utilizationFillOpacity(rateLimits.extraUsage.utilization ?? 0)}; --bar-delay: {rateLimits.windows.length * 0.09 + 0.04}s;"
        ></div>
      </div>
      <div class="ub-sub">{rateLimits.provider === "cursor" ? "On-demand spend limit" : "Monthly overuse budget"}</div>
    </div>
  {/if}

  {#if rateLimits.credits}
    <div class="ub-row">
      <div class="ub-head">
        <span class="ub-label">Credits</span>
        <span class="ub-val">
          {#if rateLimits.credits.unlimited}
            Unlimited
          {:else if rateLimits.credits.balance != null}
            {Math.round(rateLimits.credits.balance).toLocaleString()} credits
          {:else if rateLimits.credits.hasCredits}
            Available
          {:else}
            Depleted
          {/if}
        </span>
      </div>
    </div>
  {/if}
</div>

<style>
  .ub {
    padding: 10px 14px 6px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    animation: fadeUp var(--t-slow) var(--ease-out) both .09s;
  }
  .ub-provider {
    display: flex;
    align-items: baseline;
    gap: 6px;
  }
  .ub-provider-name {
    font: 600 10px/1 system-ui, sans-serif;
    color: var(--t2);

      }
  .ub-plan {
    font: 400 9px/1 system-ui, sans-serif;
    color: var(--t3);
    background: var(--surface-2);
    padding: 2px 5px;
    border-radius: 3px;
  }
  .ub-row {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .ub-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }
  .ub-head-right {
    display: flex;
    align-items: baseline;
    gap: 5px;
  }
  .ub-pace-badge {
    font: 500 9px/1 system-ui, sans-serif;
    font-variant-numeric: tabular-nums;
  }
  .ub-label {
    font: 500 11px/1 system-ui, sans-serif;
    color: var(--t1);
  }
  .ub-val {
    font: 500 11px/1 system-ui, sans-serif;
    color: var(--t1);
    font-variant-numeric: tabular-nums;
  }
  .ub-val.stale {
    opacity: 0.55;
  }
  .ub-track {
    position: relative;
    height: 6px;
    background: var(--surface-2);
    border-radius: 3px;
    overflow: hidden;
  }
  .ub-fill {
    position: absolute;
    top: 0; left: 0; height: 100%;
    border-radius: 3px;
    overflow: hidden;
    transform-origin: left center;
    transition: width var(--t-slow) var(--ease-out), box-shadow var(--t-fast) ease;
    animation: hBarGrow var(--t-slow) var(--ease-out) both;
    animation-delay: var(--bar-delay, 0s);
  }
  @keyframes hBarGrow {
    from { transform: scaleX(0); }
    to   { transform: scaleX(1); }
  }
  /* Shimmer overlay removed — read calmer without the white sheen flash. */
  .ub-sub {
    font: 400 9px/1 system-ui, sans-serif;
    color: var(--t3);
  }
  .ub-eta {
    font-weight: 500;
  }
  .ub-eta-reset {
    opacity: 0.7;
  }
  .ub-empty {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 8px 0 2px;
  }
  .ub-empty-title {
    font: 500 11px/1 system-ui, sans-serif;
    color: var(--t1);
  }
  .ub-empty-text {
    font: 400 9px/1.35 system-ui, sans-serif;
    color: var(--t3);
  }
  .ub-empty.error .ub-empty-title {
    color: var(--red, #ef4444);
  }
</style>
