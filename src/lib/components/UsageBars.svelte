<script lang="ts">
  import {
    formatRateLimitUtilizationLabel,
    getRateLimitIdleSummary,
    isRateLimitProvider,
  } from "../providerMetadata.js";
  import { formatRetryIn } from "../utils/format.js";
  import {
    currentRateLimitWindows,
    providerHasActiveCooldown,
    providerRateLimitViewState,
    rateLimitWindowResetLabel,
  } from "../views/rateLimits.js";
  import type { ProviderRateLimits, RateLimitWindow } from "../types/index.js";

  interface Props {
    providerLabel?: string;
    rateLimits: ProviderRateLimits;
  }
  let { providerLabel, rateLimits }: Props = $props();

  // Refresh "Resets in" + pace every 30s
  let refreshTick = $state(0);
  $effect(() => {
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
    if (pct >= 80) return "var(--red, #C44B45)";
    if (pct >= 50) return "var(--yellow, #C49A45)";
    return "var(--accent)";
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
    const hours = Math.floor(etaMs / 3_600_000);
    const mins = Math.floor((etaMs % 3_600_000) / 60_000);
    return hours > 0 ? `limit in ~${hours}h ${mins}m` : `limit in ~${mins}m`;
  }

  function paceColor(w: RateLimitWindow, windowHours: number): string {
    const delta = paceDelta(w, windowHours);
    if (delta === null || Math.abs(delta) < 2) return "var(--t3)";
    return delta > 0 ? "var(--accent)" : "var(--yellow, #C49A45)";
  }

  function windowHours(windowId: string): number {
    if (windowId === "five_hour" || windowId === "primary") return 5;
    if (windowId === "secondary") return 168;
    if (windowId.startsWith("seven_day")) return 168;
    return 5;
  }

  function formatUsdAmount(amount: number): string {
    const wholeDollars = Math.abs(amount - Math.round(amount)) < 0.005;
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: "USD",
      minimumFractionDigits: wholeDollars ? 0 : 2,
      maximumFractionDigits: 2,
    }).format(amount);
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
      {@const hours = windowHours(w.windowId)}
      {@const pace = paceLabel(w, hours)}
      {@const eta = etaToLimit(w, hours)}
      <div class="ub-row">
        <div class="ub-head">
          <span class="ub-label">{w.label}</span>
          <div class="ub-head-right">
            {#if pace}
              <span class="ub-pace-badge" style="color: {paceColor(w, hours)}">{pace}</span>
            {/if}
            <span class="ub-val">{utilizationLabel(w.utilization)}</span>
          </div>
        </div>
        <div class="ub-track">
          <div
            class="ub-fill"
            style="width: {Math.min(w.utilization, 100)}%; background: {utilizationColor(w.utilization)}; --bar-delay: {i * 0.09 + 0.04}s;{w.utilization >= 80 ? ` box-shadow: 0 0 7px 1px ${utilizationColor(w.utilization)}55;` : ''}"
          ></div>
        </div>
        <div class="ub-sub">
          {#if eta}
            <span class="ub-eta" style="color: {utilizationColor(w.utilization)}">{eta}</span>
            <span class="ub-eta-reset"> · {resetsIn(w.resetsAt)}</span>
          {:else}
            {resetsIn(w.resetsAt)}
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
        <span class="ub-label">Extra Usage</span>
        <span class="ub-val">{formatUsdAmount(rateLimits.extraUsage.usedCredits)} / {formatUsdAmount(rateLimits.extraUsage.monthlyLimit)}</span>
      </div>
      <div class="ub-track">
        <div
          class="ub-fill"
          style="width: {Math.min((rateLimits.extraUsage.utilization ?? 0), 100)}%; background: {utilizationColor(rateLimits.extraUsage.utilization ?? 0)}; --bar-delay: {rateLimits.windows.length * 0.09 + 0.04}s;"
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
    animation: fadeUp var(--t-slow) var(--ease-out) both .09s;
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
  .ub-head-right {
    display: flex;
    align-items: baseline;
    gap: 5px;
  }
  .ub-pace-badge {
    font: 500 9px/1 'Inter', sans-serif;
    font-variant-numeric: tabular-nums;
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
  .ub-fill::after {
    content: '';
    position: absolute;
    top: 0; bottom: 0;
    left: -100%; width: 100%;
    background: linear-gradient(90deg, transparent 30%, rgba(255,255,255,.22) 50%, transparent 70%);
    animation: hBarShimmer .5s ease-out both;
    animation-delay: calc(var(--bar-delay, 0s) + .52s);
    pointer-events: none;
  }
  @keyframes hBarShimmer {
    from { transform: translateX(0); }
    to   { transform: translateX(200%); opacity: 0; }
  }
  .ub-sub {
    font: 400 9px/1 'Inter', sans-serif;
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
