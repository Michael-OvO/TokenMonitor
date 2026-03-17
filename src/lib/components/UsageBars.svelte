<script lang="ts">
  import { formatResetsIn, formatRetryIn } from "../utils/format.js";
  import {
    providerHasActiveCooldown,
    providerRateLimitViewState,
  } from "../rateLimitsView.js";
  import type { ProviderRateLimits, RateLimitWindow } from "../types/index.js";

  interface Props {
    providerLabel?: string;
    rateLimits: ProviderRateLimits;
  }
  let { providerLabel, rateLimits }: Props = $props();
  let viewState = $derived(providerRateLimitViewState(rateLimits));
  let hasActiveCooldown = $derived(providerHasActiveCooldown(rateLimits));

  // Refresh "Resets in" + pace every 30s
  let refreshTick = $state(0);
  $effect(() => {
    const interval = setInterval(() => { refreshTick += 1; }, 30_000);
    return () => clearInterval(interval);
  });

  function utilizationColor(pct: number): string {
    if (pct >= 80) return "var(--red, #ef4444)";
    if (pct >= 50) return "var(--yellow, #f59e0b)";
    return "var(--accent, #22c55e)";
  }

  function resetsIn(isoString: string | null): string {
    void refreshTick;
    return formatResetsIn(isoString);
  }

  function paceLabel(w: RateLimitWindow, windowHours: number): string {
    void refreshTick;
    if (!w.resetsAt) return "";
    const resetMs = new Date(w.resetsAt).getTime();
    const now = Date.now();
    const remainingMs = resetMs - now;
    if (remainingMs <= 0) return "";
    const totalMs = windowHours * 3_600_000;
    const elapsedMs = totalMs - remainingMs;
    if (elapsedMs <= 0) return "";
    const uniformPct = (elapsedMs / totalMs) * 100;
    const delta = uniformPct - w.utilization;
    const absDelta = Math.abs(Math.round(delta));
    if (absDelta < 2) return "on pace";
    if (delta > 0) return `${absDelta}% ahead`;
    return `${absDelta}% behind`;
  }

  function paceColor(w: RateLimitWindow, windowHours: number): string {
    void refreshTick;
    if (!w.resetsAt) return "var(--t3)";
    const resetMs = new Date(w.resetsAt).getTime();
    const now = Date.now();
    const remainingMs = resetMs - now;
    if (remainingMs <= 0) return "var(--t3)";
    const totalMs = windowHours * 3_600_000;
    const elapsedMs = totalMs - remainingMs;
    if (elapsedMs <= 0) return "var(--t3)";
    const uniformPct = (elapsedMs / totalMs) * 100;
    const delta = uniformPct - w.utilization;
    if (Math.abs(delta) < 2) return "var(--t3)";
    if (delta > 0) return "var(--green, #22c55e)";
    return "var(--red, #ef4444)";
  }

  function windowHours(windowId: string): number {
    if (windowId === "five_hour" || windowId === "primary") return 5;
    if (windowId === "secondary") return 168;
    if (windowId.startsWith("seven_day")) return 168;
    return 5;
  }

  function emptySummary(): string {
    void refreshTick;
    const retryLabel = formatRetryIn(rateLimits.cooldownUntil);
    if (viewState === "error") {
      const base = rateLimits.error ?? "Unable to load rate limits right now.";
      return retryLabel ? `${base} ${retryLabel}.` : base;
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
    {#each rateLimits.windows as w}
      {@const hours = windowHours(w.windowId)}
      {@const pace = paceLabel(w, hours)}
      <div class="ub-row">
        <div class="ub-head">
          <span class="ub-label">{w.label}</span>
          <span class="ub-val">{w.utilization}%</span>
        </div>
        <div class="ub-track">
          <div
            class="ub-fill"
            style="width: {Math.min(w.utilization, 100)}%; background: {utilizationColor(w.utilization)};"
          ></div>
        </div>
        <div class="ub-sub">
          {resetsIn(w.resetsAt)}{#if pace}
            <span class="ub-pace" style="color: {paceColor(w, hours)}"> · {pace}</span>
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
        <span class="ub-label">Extra Usage</span>
        <span class="ub-val">${rateLimits.extraUsage.usedCredits.toFixed(0)} / ${rateLimits.extraUsage.monthlyLimit.toFixed(0)}</span>
      </div>
      <div class="ub-track">
        <div
          class="ub-fill"
          style="width: {Math.min((rateLimits.extraUsage.utilization ?? 0), 100)}%; background: {utilizationColor(rateLimits.extraUsage.utilization ?? 0)};"
        ></div>
      </div>
      <div class="ub-sub">Monthly overuse budget</div>
    </div>
  {/if}
</div>

<style>
  .ub {
    padding: 10px 14px 6px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    animation: fadeUp .28s ease both .09s;
  }
  .ub-provider {
    display: flex;
    align-items: baseline;
    gap: 6px;
  }
  .ub-provider-name {
    font: 600 10px/1 'Inter', sans-serif;
    color: var(--t2);
    text-transform: uppercase;
    letter-spacing: .8px;
  }
  .ub-plan {
    font: 400 9px/1 'Inter', sans-serif;
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
  .ub-label {
    font: 500 11px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .ub-val {
    font: 500 11px/1 'Inter', sans-serif;
    color: var(--t1);
    font-variant-numeric: tabular-nums;
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
    transition: width 0.5s cubic-bezier(.25,.8,.25,1);
  }
  .ub-sub {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .ub-pace {
    font-weight: 500;
  }
  .ub-empty {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 8px 0 2px;
  }
  .ub-empty-title {
    font: 500 11px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .ub-empty-text {
    font: 400 9px/1.35 'Inter', sans-serif;
    color: var(--t3);
  }
  .ub-empty.error .ub-empty-title {
    color: var(--red, #ef4444);
  }
</style>
