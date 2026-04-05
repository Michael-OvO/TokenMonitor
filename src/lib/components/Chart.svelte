<script lang="ts">
  import { modelColor, formatCost, currencySymbol, convertCost, deviceColor } from "../utils/format.js";
  import { settings } from "../stores/settings.js";
  import { activeOffset, activePeriod, chartMode, chartSegmentMode } from "../stores/usage.js";
  import { isMacOS } from "../utils/platform.js";
  import { logger } from "../utils/logger.js";
  import type { ChartBucket } from "../types/index.js";
  import { filterVisibleChartBuckets, getXAxisLabels } from "./chartBuckets.js";

  const detailAbove = false;
  const DETAIL_CONFIG = {
    HOVER_DELAY_MS: 0,
    LEAVE_DELAY_MS: 120,
  } as const;

  interface Props {
    buckets: ChartBucket[];
    dataKey: string;
    deviceBuckets?: ChartBucket[] | null;
  }
  let { buckets, dataKey, deviceBuckets }: Props = $props();

  // Color function based on segment mode.
  let segmentColorFn = $derived(
    $chartSegmentMode === "device" && deviceBuckets
      ? deviceColor
      : modelColor
  );

  // Auto-reset to "model" when device buckets disappear.
  $effect(() => {
    if (!deviceBuckets && $chartSegmentMode === "device") {
      chartSegmentMode.set("model");
    }
  });

  let hiddenModels = $state<string[]>([]);
  $effect(() => {
    const unsub = settings.subscribe((s) => (hiddenModels = s.hiddenModels));
    return unsub;
  });

  // Select active buckets based on segment mode.
  let activeBuckets = $derived(
    $chartSegmentMode === "device" && deviceBuckets ? deviceBuckets : buckets
  );

  // Filter hidden models from buckets (only applies in model mode).
  let filteredBuckets = $derived(
    $chartSegmentMode === "device"
      ? activeBuckets
      : activeBuckets.map((b) => {
          const segs = b.segments.filter((s) => !hiddenModels.includes(s.model_key));
          return { ...b, segments: segs, total: segs.reduce((sum, s) => sum + s.cost, 0) };
        })
  );
  let visibleBuckets = $derived(filterVisibleChartBuckets(filteredBuckets, $activePeriod, $activeOffset));
  let xAxisLabels = $derived(getXAxisLabels(visibleBuckets));

  const CHART_H = 108;
  const CHART_W = 280; // SVG viewbox width (y-axis labels sit outside)
  let maxCost = $derived(Math.max(...visibleBuckets.map((b) => b.total), 0.01));
  let hoveredIdx = $state(-1);

  let displayedIdx = $state(-1);
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  let leaveTimer: ReturnType<typeof setTimeout> | null = null;
  let previousDataKey = $state("");

  let displayed = $derived(displayedIdx >= 0 ? visibleBuckets[displayedIdx] ?? null : null);

  function onEnter(i: number) {
    if (leaveTimer) { clearTimeout(leaveTimer); leaveTimer = null; }
    hoveredIdx = i;
    if (hoverTimer) { clearTimeout(hoverTimer); hoverTimer = null; }
    displayedIdx = visibleBuckets[i]?.total > 0 ? i : -1;
  }

  function onLeave() {
    hoveredIdx = -1;
    if (hoverTimer) { clearTimeout(hoverTimer); hoverTimer = null; }
    if (leaveTimer) clearTimeout(leaveTimer);
    leaveTimer = setTimeout(() => { displayedIdx = -1; }, DETAIL_CONFIG.LEAVE_DELAY_MS);
  }

  // Reset on tab / provider / offset change.
  $effect(() => {
    if (previousDataKey === "") { previousDataKey = dataKey; return; }
    if (dataKey === previousDataKey) return;
    previousDataKey = dataKey;
    if (hoverTimer) { clearTimeout(hoverTimer); hoverTimer = null; }
    hoveredIdx = -1;
    displayedIdx = -1;
  });

  $effect(() => {
    const bucketCount = visibleBuckets.length;
    if (hoveredIdx >= bucketCount) hoveredIdx = -1;
    if (displayedIdx >= bucketCount) displayedIdx = -1;
  });


  $effect(() => {
    return () => {
      if (hoverTimer) clearTimeout(hoverTimer);
      if (leaveTimer) clearTimeout(leaveTimer);
    };
  });

  let legendModels = $derived(() => {
    const seen = new Map<string, string>();
    for (const b of visibleBuckets) {
      for (const s of b.segments) {
        if (!seen.has(s.model_key)) seen.set(s.model_key, s.model);
      }
    }
    return Array.from(seen.entries()).map(([key, name]) => ({ key, name }));
  });

  // Y-axis ticks (3 ticks: 0, mid, max)
  let yTicks = $derived(() => {
    const nice = niceMax(maxCost);
    return [
      { val: nice, y: 0 },
      { val: nice / 2, y: CHART_H / 2 },
      { val: 0, y: CHART_H },
    ];
  });

  function niceMax(v: number): number {
    if (v <= 0.5) return 0.5;
    if (v <= 1) return 1;
    if (v <= 2) return 2;
    if (v <= 5) return 5;
    if (v <= 10) return 10;
    if (v <= 20) return 20;
    if (v <= 50) return 50;
    if (v <= 100) return 100;
    if (v <= 200) return 200;
    return Math.ceil(v / 100) * 100;
  }

  function yLabel(v: number): string {
    const sym = currencySymbol();
    const c = convertCost(v);
    if (c === 0) return `${sym}0`;
    if (c < 1) return `${sym}${c.toFixed(2)}`;
    return `${sym}${Math.round(c)}`;
  }

  function sortedSegments(bucket: ChartBucket | null): ChartBucket["segments"] {
    if (!bucket) return [];
    // Merge segments with the same model_key (e.g. from local + remote devices).
    const merged = new Map<string, ChartBucket["segments"][0]>();
    for (const seg of bucket.segments) {
      const existing = merged.get(seg.model_key);
      if (existing) {
        existing.cost += seg.cost;
        existing.tokens += seg.tokens;
      } else {
        merged.set(seg.model_key, { ...seg });
      }
    }
    return Array.from(merged.values()).sort((a, b) => b.cost - a.cost);
  }


  // Bar chart geometry — fill full width, small gaps
  let barGap = $derived(
    visibleBuckets.length > 1
      ? Math.max(Math.min(2, CHART_W / visibleBuckets.length * 0.15), 1)
      : 1
  );
  let barWidth = $derived(
    visibleBuckets.length > 0
      ? (CHART_W - barGap * Math.max(visibleBuckets.length - 1, 0)) / visibleBuckets.length
      : CHART_W
  );

  function barX(i: number): number {
    return i * (barWidth + barGap);
  }

  // Line chart: compute points per model, stacked
  let lineData = $derived(() => {
    const models = legendModels();
    const niceM = niceMax(maxCost);
    const stepX = visibleBuckets.length > 1 ? CHART_W / (visibleBuckets.length - 1) : CHART_W / 2;

    return models.map((m) => {
      const points = visibleBuckets.map((b, i) => {
        const seg = b.segments.find((s) => s.model_key === m.key);
        const cost = seg?.cost ?? 0;
        const x = visibleBuckets.length > 1 ? i * stepX : CHART_W / 2;
        const y = CHART_H - (cost / niceM) * CHART_H;
        return { x, y, cost };
      });
      return { key: m.key, name: m.name, points };
    });
  });

  // Smooth SVG path from points (cardinal spline approximation)
  function smoothPath(pts: Array<{x: number; y: number}>): string {
    if (pts.length === 0) return "";
    if (pts.length === 1) return `M${pts[0].x},${pts[0].y}`;
    let d = `M${pts[0].x},${pts[0].y}`;
    for (let i = 1; i < pts.length; i++) {
      const prev = pts[i - 1];
      const curr = pts[i];
      const cpx = (prev.x + curr.x) / 2;
      d += ` C${cpx},${prev.y} ${cpx},${curr.y} ${curr.x},${curr.y}`;
    }
    return d;
  }

  function areaPath(pts: Array<{x: number; y: number}>): string {
    if (pts.length === 0) return "";
    const line = smoothPath(pts);
    const last = pts[pts.length - 1];
    const first = pts[0];
    return `${line} L${last.x},${CHART_H} L${first.x},${CHART_H} Z`;
  }

  function bucketAriaLabel(bucket: ChartBucket): string {
    return `${bucket.label}: ${formatCost(bucket.total)}`;
  }
</script>

<div class="ch" class:detail-above={detailAbove}>
  <div class="ch-top">
    <span class="ch-t">Cost by {$chartSegmentMode === "device" ? "device" : "model"}</span>
    <div class="ch-right">
      <div class="leg">
        {#each legendModels() as lm}
          <span class="leg-item">
            <span class="leg-dot" style="background:{segmentColorFn(lm.key)}"></span>
            {lm.name}
          </span>
        {/each}
      </div>
      {#if deviceBuckets}
        <div class="mode-toggle seg-toggle">
          <button type="button" class:on={$chartSegmentMode === "model"} title="By model" onclick={() => { logger.info("chart", "Segment: model"); chartSegmentMode.set("model"); }}>M</button>
          <button type="button" class:on={$chartSegmentMode === "device"} title="By device" onclick={() => { logger.info("chart", "Segment: device"); chartSegmentMode.set("device"); }}>D</button>
        </div>
      {/if}
      <div class="mode-toggle">
        <button
          type="button"
          class:on={$chartMode === "bar"}
          aria-label="Show bar chart"
          aria-pressed={$chartMode === "bar"}
          title="Show bar chart"
          onclick={() => { logger.info("chart", "Mode: bar"); chartMode.set("bar"); }}
        >
          <svg width="10" height="10" viewBox="0 0 10 10">
            <rect x="1" y="4" width="2" height="6" rx=".5" fill="currentColor"/>
            <rect x="4" y="1" width="2" height="9" rx=".5" fill="currentColor"/>
            <rect x="7" y="3" width="2" height="7" rx=".5" fill="currentColor"/>
          </svg>
        </button>
        <button
          type="button"
          class:on={$chartMode === "line"}
          aria-label="Show line chart"
          aria-pressed={$chartMode === "line"}
          title="Show line chart"
          onclick={() => { logger.info("chart", "Mode: line"); chartMode.set("line"); }}
        >
          <svg width="10" height="10" viewBox="0 0 10 10">
            <path d="M1,7 C3,3 5,5 9,2" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round"/>
          </svg>
        </button>
      </div>
    </div>
  </div>

  <div class="detail" class:visible={displayed != null}>
    {#if displayed}
      {@const segs = sortedSegments(displayed)}
      <div class="detail-inner">
        <div class="detail-head">
          <span class="detail-label">{displayed.label}</span>
          <span class="detail-total">{formatCost(displayed.total)}</span>
        </div>
        {#if segs.length > 0}
          <div class="detail-rows">
            {#each segs as seg}
              <div class="detail-row">
                <span class="detail-dot" style="background:{segmentColorFn(seg.model_key)}"></span>
                <span class="detail-name">{seg.model}</span>
                <span class="detail-cost">{formatCost(seg.cost)}</span>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <div class="chart-body">
    <!-- Y-axis labels -->
    <div class="y-axis">
      {#each yTicks() as tick}
        <span class="y-label" style="top: {tick.y}px">{yLabel(tick.val)}</span>
      {/each}
    </div>

    <!-- Chart area -->
    <div class="chart-area">
      {#key `${dataKey}-${$chartMode}`}
      <div class="chart-fade">
        {#if $chartMode === "bar"}
          <!-- BAR CHART -->
          <svg viewBox="0 0 {CHART_W} {CHART_H}" preserveAspectRatio="none" class="chart-svg">
            <!-- Grid lines -->
            {#each yTicks() as tick}
              <line x1="0" y1={tick.y} x2={CHART_W} y2={tick.y} style="stroke: var(--border-subtle)" stroke-width="0.5"/>
            {/each}

            {#each visibleBuckets as bucket, i}
              {@const niceM = niceMax(maxCost)}
              {@const x = barX(i)}
              {@const isActive = hoveredIdx === i}
              {@const segs = sortedSegments(bucket)}
              <g
                class="bar-group"
                role="img"
                aria-label={bucketAriaLabel(bucket)}
                onmouseenter={() => onEnter(i)}
                onmouseleave={onLeave}
                style="cursor:pointer; --delay: {(i / Math.max(visibleBuckets.length - 1, 1)) * 0.35 + 0.04}s;"
              >
                <!-- Invisible hit area -->
                <rect x={x - 1} y="0" width={barWidth + 2} height={CHART_H} fill="transparent"/>

                <!-- Stacked segments (bottom to top, sorted by cost desc) -->
                {#each segs as seg, si}
                  {@const segH = (seg.cost / niceM) * CHART_H}
                  {@const prevH = segs.slice(0, si).reduce((a, s) => a + (s.cost / niceM) * CHART_H, 0)}
                  {@const segY = CHART_H - prevH - segH}
                  <rect
                    x={x}
                    y={segY}
                    width={barWidth}
                    height={Math.max(segH, 1)}
                    rx="1.5"
                    fill={segmentColorFn(seg.model_key)}
                    opacity={isActive ? 1 : 0.7}
                    class="bar-seg"
                  />
                {/each}
              </g>
            {/each}
          </svg>

        {:else}
          <!-- LINE CHART -->
          <svg viewBox="0 0 {CHART_W} {CHART_H}" preserveAspectRatio="none" class="chart-svg line-svg">
            <!-- Grid lines -->
            {#each yTicks() as tick}
              <line x1="0" y1={tick.y} x2={CHART_W} y2={tick.y} style="stroke: var(--border-subtle)" stroke-width="0.5"/>
            {/each}

            <defs>
              {#each lineData() as ld}
                <linearGradient id="grad-{ld.key}" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stop-color={segmentColorFn(ld.key)} stop-opacity="0.25"/>
                  <stop offset="100%" stop-color={segmentColorFn(ld.key)} stop-opacity="0.02"/>
                </linearGradient>
              {/each}
            </defs>

            {#each lineData() as ld}
              <!-- Area fill -->
              <path d={areaPath(ld.points)} fill="url(#grad-{ld.key})" class="area-path"/>
              <!-- Line -->
              <path
                d={smoothPath(ld.points)}
                pathLength="1"
                fill="none"
                stroke={segmentColorFn(ld.key)}
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
                class="line-path"
              />
              <!-- Dots -->
              {#each ld.points as pt, i}
                <circle
                  cx={pt.x} cy={pt.y} r={hoveredIdx === i ? 3 : 1.5}
                  fill={segmentColorFn(ld.key)}
                  class="dot"
                  style="transition: r .15s ease"
                />
              {/each}
            {/each}

            <!-- Hover hit areas (invisible columns) -->
            {#each visibleBuckets as bucket, i}
              {@const stepX = visibleBuckets.length > 1 ? CHART_W / (visibleBuckets.length - 1) : CHART_W}
              {@const x = visibleBuckets.length > 1 ? i * stepX : CHART_W / 2}
              <g
                role="img"
                aria-label={bucketAriaLabel(bucket)}
                onmouseenter={() => onEnter(i)}
                onmouseleave={onLeave}
                style="cursor:pointer"
              >
                <rect
                  x={x - stepX / 2}
                  y="0"
                  width={stepX}
                  height={CHART_H}
                  fill="transparent"
                />
              </g>
            {/each}

            <!-- Hover vertical line -->
            {#if hoveredIdx >= 0}
              {@const stepX = visibleBuckets.length > 1 ? CHART_W / (visibleBuckets.length - 1) : CHART_W / 2}
              {@const hx = visibleBuckets.length > 1 ? hoveredIdx * stepX : CHART_W / 2}
              <line x1={hx} y1="0" x2={hx} y2={CHART_H} style="stroke: var(--border)" stroke-width="0.5" stroke-dasharray="2,2"/>
            {/if}
          </svg>
        {/if}
      </div>
      {/key}
    </div>
  </div>

  {#if xAxisLabels.length > 0}
    <div class="xa">
      {#each xAxisLabels as label}
        <span>{label}</span>
      {/each}
    </div>
  {/if}

</div>

<style>
  .ch {
    padding: 14px 12px;
    animation: fadeUp var(--t-slow) var(--ease-out) both .09s;
    display: flex;
    flex-direction: column;
    position: relative;
  }
  .ch-top { order: 1; }
  .chart-body { order: 2; }
  .xa { order: 3; }
  .detail { order: 4; }
  .ch.detail-above .detail { order: 2; }
  .ch.detail-above .chart-body { order: 3; }
  .ch.detail-above .xa { order: 4; }

  .ch-top { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }
  .ch-t { font: 500 8px/1 "Inter", sans-serif; color: var(--t3); text-transform: uppercase; letter-spacing: .8px; }
  .ch-right { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .leg { display: flex; gap: 7px; overflow: hidden; min-width: 0; }
  .leg-item {
    display: flex; align-items: center; gap: 3px;
    font: 400 8px/1.3 "Inter", sans-serif; color: var(--t2);
    white-space: nowrap; overflow: hidden; min-width: 20px;
  }
  .leg-dot { width: 5px; height: 5px; border-radius: 1.5px; flex-shrink: 0; }

  /* Mode toggle */
  .mode-toggle {
    display: flex;
    flex-shrink: 0;
    background: var(--surface-2);
    border-radius: 4px;
    padding: 1.5px;
    gap: 1px;
  }
  .mode-toggle button {
    display: flex; align-items: center; justify-content: center;
    width: 20px; height: 16px;
    border: none; background: none;
    color: var(--t3); cursor: pointer;
    border-radius: 4px;
    transition: color var(--t-fast) ease, background var(--t-fast) ease;
  }
  .mode-toggle button.on {
    color: var(--t1);
    background: var(--surface-hover);
  }
  .mode-toggle button:hover:not(.on) { color: var(--t2); }
  .seg-toggle {
    margin-right: 2px;
  }
  .seg-toggle button {
    font: 600 7px/1 'Inter', sans-serif;
    width: 16px;
  }

  .detail {
    background: var(--surface-2);
    border-radius: 8px;
    overflow: hidden;
    opacity: 0;
    max-height: 0;
    will-change: opacity;
    transition: opacity 0.12s ease;
  }
  .detail.visible {
    opacity: 1;
    max-height: 200px;
  }
  .ch.detail-above .detail { margin-bottom: 10px; }
  .ch:not(.detail-above) .detail { margin-top: 10px; }

  /* Chart body: y-axis + chart area side by side */
  .chart-body { display: flex; align-items: stretch; gap: 4px; }

  .y-axis {
    position: relative;
    width: 28px;
    height: 108px;
    flex-shrink: 0;
  }
  .y-label {
    position: absolute;
    right: 0;
    font: 500 8px/1 "Inter", sans-serif;
    color: var(--t2);
    font-variant-numeric: tabular-nums;
    transform: translateY(-50%);
  }

  .chart-area {
    flex: 1;
    height: 108px;
    min-height: 108px;
    position: relative;
  }
  .chart-fade {
    animation: chartFadeIn .2s ease both;
    min-height: 108px;
  }
  @keyframes chartFadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }
  .chart-svg {
    width: 100%;
    height: 100%;
    overflow: visible;
  }

  /* Bar groups — grow whole column from chart floor */
  .bar-group {
    transform-box: fill-box;
    transform-origin: center bottom;
    animation: svgBarGrow var(--t-slow) var(--ease-out) both;
    animation-delay: var(--delay, 0s);
  }
  @keyframes svgBarGrow {
    from { transform: scaleY(0); }
    to   { transform: scaleY(1); }
  }
  .bar-seg { transition: opacity var(--t-fast) ease; will-change: opacity; }

  /* Line chart */
  .line-path {
    stroke-dasharray: 1;
    stroke-dashoffset: 1;
    animation: drawLine .6s ease both;
  }
  .area-path {
    animation: fadeIn .8s ease both;
  }
  .dot {
    transition: all .15s ease;
  }
  @keyframes drawLine {
    to { stroke-dashoffset: 0; }
  }
  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .xa { display: flex; justify-content: space-between; margin-top: 8px; padding: 0 29px 0 32px; }
  .xa span { font: 400 8px/1 "Inter", sans-serif; color: var(--t4); font-variant-numeric: tabular-nums; }

  .detail-inner {
    padding: 8px 10px;
    min-height: 0;
    overflow: hidden;
  }
  .detail-head {
    display: flex; justify-content: space-between; align-items: baseline;
    margin-bottom: 5px;
  }
  .detail-label { font: 600 10px/1 "Inter", sans-serif; color: var(--t1); }
  .detail-total { font: 600 10px/1 "Inter", sans-serif; color: var(--t1); font-variant-numeric: tabular-nums; }
  .detail-rows {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .detail-row { display: flex; align-items: center; gap: 5px; }
  .detail-dot { width: 5px; height: 5px; border-radius: 1.5px; flex-shrink: 0; }
  .detail-name { font: 400 10px/1 "Inter", sans-serif; color: var(--t2); flex: 1; }
  .detail-cost { font: 500 10px/1 "Inter", sans-serif; color: var(--t1); font-variant-numeric: tabular-nums; }

</style>
