<script lang="ts">
  import { modelColor, formatCost, currencySymbol, convertCost } from "../utils/format.js";
  import { settings } from "../stores/settings.js";
  import type { ChartBucket, ChartSegment } from "../types/index.js";

  interface Props { buckets: ChartBucket[]; dataKey: string }
  let { buckets, dataKey }: Props = $props();

  let hiddenModels = $state<string[]>([]);
  $effect(() => {
    const unsub = settings.subscribe((s) => (hiddenModels = s.hiddenModels));
    return unsub;
  });

  // Filter hidden models from buckets
  let filteredBuckets = $derived(
    buckets.map((b) => {
      const segs = b.segments.filter((s) => !hiddenModels.includes(s.model_key));
      return { ...b, segments: segs, total: segs.reduce((sum, s) => sum + s.cost, 0) };
    })
  );

  const CHART_H = 72;
  const CHART_W = 280; // SVG viewbox width (y-axis labels sit outside)
  let maxCost = $derived(Math.max(...filteredBuckets.map((b) => b.total), 0.01));
  let hoveredIdx = $state(-1);
  let chartMode = $state<"bar" | "line">("bar");

  // Debounced hover for smooth detail panel transitions
  let displayedIdx = $state(-1);
  let leaveTimer: ReturnType<typeof setTimeout> | null = null;

  function onEnter(i: number) {
    if (leaveTimer) { clearTimeout(leaveTimer); leaveTimer = null; }
    hoveredIdx = i;
    displayedIdx = filteredBuckets[i]?.total > 0 ? i : -1;
  }
  function onLeave() {
    hoveredIdx = -1;
    leaveTimer = setTimeout(() => { displayedIdx = -1; }, 150);
  }

  let legendModels = $derived(() => {
    const seen = new Map<string, string>();
    for (const b of filteredBuckets) {
      for (const s of b.segments) {
        if (!seen.has(s.model_key)) seen.set(s.model_key, s.model);
      }
    }
    return Array.from(seen.entries()).map(([key, name]) => ({ key, name }));
  });

  let displayed = $derived(displayedIdx >= 0 ? filteredBuckets[displayedIdx] : null);

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

  // Bar chart geometry — fill full width, small gaps
  let barGap = $derived(Math.max(Math.min(2, CHART_W / filteredBuckets.length * 0.15), 1));
  let barWidth = $derived((CHART_W - barGap * Math.max(filteredBuckets.length - 1, 0)) / filteredBuckets.length);

  function barX(i: number): number {
    return i * (barWidth + barGap);
  }

  // Line chart: compute points per model, stacked
  let lineData = $derived(() => {
    const models = legendModels();
    const niceM = niceMax(maxCost);
    const stepX = filteredBuckets.length > 1 ? CHART_W / (filteredBuckets.length - 1) : CHART_W / 2;

    return models.map((m) => {
      const points = filteredBuckets.map((b, i) => {
        const seg = b.segments.find((s) => s.model_key === m.key);
        const cost = seg?.cost ?? 0;
        const x = filteredBuckets.length > 1 ? i * stepX : CHART_W / 2;
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
</script>

<div class="ch">
  <div class="ch-top">
    <span class="ch-t">Cost by model</span>
    <div class="ch-right">
      <div class="leg">
        {#each legendModels() as lm}
          <span class="leg-item">
            <span class="leg-dot" style="background:{modelColor(lm.key)}"></span>
            {lm.name}
          </span>
        {/each}
      </div>
      <div class="mode-toggle">
        <button class:on={chartMode === "bar"} onclick={() => (chartMode = "bar")}>
          <svg width="10" height="10" viewBox="0 0 10 10">
            <rect x="1" y="4" width="2" height="6" rx=".5" fill="currentColor"/>
            <rect x="4" y="1" width="2" height="9" rx=".5" fill="currentColor"/>
            <rect x="7" y="3" width="2" height="7" rx=".5" fill="currentColor"/>
          </svg>
        </button>
        <button class:on={chartMode === "line"} onclick={() => (chartMode = "line")}>
          <svg width="10" height="10" viewBox="0 0 10 10">
            <path d="M1,7 C3,3 5,5 9,2" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round"/>
          </svg>
        </button>
      </div>
    </div>
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
      {#key `${dataKey}-${chartMode}`}
      <div class="chart-fade">
        {#if chartMode === "bar"}
          <!-- BAR CHART -->
          <svg viewBox="0 0 {CHART_W} {CHART_H}" preserveAspectRatio="none" class="chart-svg">
            <!-- Grid lines -->
            {#each yTicks() as tick}
              <line x1="0" y1={tick.y} x2={CHART_W} y2={tick.y} stroke="rgba(255,255,255,0.04)" stroke-width="0.5"/>
            {/each}

            {#each filteredBuckets as bucket, i}
              {@const niceM = niceMax(maxCost)}
              {@const x = barX(i)}
              {@const isActive = hoveredIdx === i}
              <g
                role="img"
                aria-label="{bucket.label}: {formatCost(bucket.total)}"
                onmouseenter={() => onEnter(i)}
                onmouseleave={onLeave}
                style="cursor:pointer"
              >
                <!-- Invisible hit area -->
                <rect x={x - 1} y="0" width={barWidth + 2} height={CHART_H} fill="transparent"/>

                <!-- Stacked segments (bottom to top) -->
                {#each bucket.segments as seg, si}
                  {@const segH = (seg.cost / niceM) * CHART_H}
                  {@const prevH = bucket.segments.slice(0, si).reduce((a, s) => a + (s.cost / niceM) * CHART_H, 0)}
                  {@const segY = CHART_H - prevH - segH}
                  <rect
                    x={x}
                    y={segY}
                    width={barWidth}
                    height={Math.max(segH, 1)}
                    rx="1.5"
                    fill={modelColor(seg.model_key)}
                    opacity={isActive ? 1 : 0.7}
                    class="bar-seg"
                    style="--delay: {i * 0.03 + 0.1}s; --origin-y: {CHART_H}px; transform-origin: {x + barWidth/2}px {CHART_H}px;"
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
              <line x1="0" y1={tick.y} x2={CHART_W} y2={tick.y} stroke="rgba(255,255,255,0.04)" stroke-width="0.5"/>
            {/each}

            <defs>
              {#each lineData() as ld}
                <linearGradient id="grad-{ld.key}" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stop-color={modelColor(ld.key)} stop-opacity="0.25"/>
                  <stop offset="100%" stop-color={modelColor(ld.key)} stop-opacity="0.02"/>
                </linearGradient>
              {/each}
            </defs>

            {#each lineData() as ld}
              <!-- Area fill -->
              <path d={areaPath(ld.points)} fill="url(#grad-{ld.key})" class="area-path"/>
              <!-- Line -->
              <path d={smoothPath(ld.points)} fill="none" stroke={modelColor(ld.key)} stroke-width="1.5" stroke-linecap="round" class="line-path"/>
              <!-- Dots -->
              {#each ld.points as pt, i}
                <circle
                  cx={pt.x} cy={pt.y} r={hoveredIdx === i ? 3 : 1.5}
                  fill={modelColor(ld.key)}
                  class="dot"
                  style="transition: r .15s ease"
                />
              {/each}
            {/each}

            <!-- Hover hit areas (invisible columns) -->
            {#each filteredBuckets as bucket, i}
              {@const stepX = filteredBuckets.length > 1 ? CHART_W / (filteredBuckets.length - 1) : CHART_W}
              {@const x = filteredBuckets.length > 1 ? i * stepX : CHART_W / 2}
              <rect
                x={x - stepX / 2}
                y="0"
                width={stepX}
                height={CHART_H}
                fill="transparent"
                onmouseenter={() => onEnter(i)}
                onmouseleave={onLeave}
                style="cursor:pointer"
              />
            {/each}

            <!-- Hover vertical line -->
            {#if hoveredIdx >= 0}
              {@const stepX = filteredBuckets.length > 1 ? CHART_W / (filteredBuckets.length - 1) : CHART_W / 2}
              {@const hx = filteredBuckets.length > 1 ? hoveredIdx * stepX : CHART_W / 2}
              <line x1={hx} y1="0" x2={hx} y2={CHART_H} stroke="rgba(255,255,255,0.12)" stroke-width="0.5" stroke-dasharray="2,2"/>
            {/if}
          </svg>
        {/if}
      </div>
      {/key}
    </div>
  </div>

  {#if buckets.length > 0}
    <div class="xa">
      <span>{buckets[0]?.label ?? ""}</span>
      {#if buckets.length > 4}
        <span>{buckets[Math.floor(buckets.length / 2)]?.label ?? ""}</span>
      {/if}
      <span>{buckets[buckets.length - 1]?.label ?? ""}</span>
    </div>
  {/if}

  <!-- Detail panel -->
  <div class="detail" class:visible={displayed != null}>
    {#if displayed}
      {#key displayedIdx}
        <div class="detail-inner">
          <div class="detail-head">
            <span class="detail-label">{displayed.label}</span>
            <span class="detail-total">{formatCost(displayed.total)}</span>
          </div>
          <div class="detail-models">
            {#each displayed.segments as seg}
              <div class="detail-row">
                <span class="detail-dot" style="background:{modelColor(seg.model_key)}"></span>
                <span class="detail-name">{seg.model}</span>
                <span class="detail-cost">{formatCost(seg.cost)}</span>
              </div>
            {/each}
          </div>
        </div>
      {/key}
    {/if}
  </div>
</div>

<style>
  .ch { padding: 14px 12px; animation: fadeUp .28s ease both .09s; }

  .ch-top { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }
  .ch-t { font: 500 8px/1 'Inter', sans-serif; color: var(--t3); text-transform: uppercase; letter-spacing: .8px; }
  .ch-right { display: flex; align-items: center; gap: 8px; }
  .leg { display: flex; gap: 7px; }
  .leg-item {
    display: flex; align-items: center; gap: 3px;
    font: 400 8px/1 'Inter', sans-serif; color: var(--t2);
  }
  .leg-dot { width: 5px; height: 5px; border-radius: 1.5px; flex-shrink: 0; }

  /* Mode toggle */
  .mode-toggle {
    display: flex;
    background: rgba(255,255,255,0.04);
    border-radius: 4px;
    padding: 1.5px;
    gap: 1px;
  }
  .mode-toggle button {
    display: flex; align-items: center; justify-content: center;
    width: 20px; height: 16px;
    border: none; background: none;
    color: var(--t3); cursor: pointer;
    border-radius: 3px;
    transition: color .15s, background .15s;
  }
  .mode-toggle button.on {
    color: var(--t1);
    background: rgba(255,255,255,0.08);
  }
  .mode-toggle button:hover:not(.on) { color: var(--t2); }

  /* Chart body: y-axis + chart area side by side */
  .chart-body { display: flex; align-items: stretch; gap: 4px; }

  .y-axis {
    position: relative;
    width: 28px;
    height: 72px;
    flex-shrink: 0;
  }
  .y-label {
    position: absolute;
    right: 0;
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t2);
    font-variant-numeric: tabular-nums;
    transform: translateY(-50%);
  }

  .chart-area {
    flex: 1;
    height: 72px;
    min-height: 72px;  /* prevent collapse during content swap */
    position: relative;
  }
  .chart-fade {
    animation: chartFadeIn .2s ease both;
    min-height: 72px;  /* preserve height during {#key} destroy→create to avoid 0-height flash */
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

  /* Bar segments — grow from bottom */
  .bar-seg {
    transition: opacity .2s ease;
    animation: svgBarGrow .48s cubic-bezier(.34,1.3,.64,1) both;
    animation-delay: var(--delay, 0s);
  }
  @keyframes svgBarGrow {
    from { transform: scaleY(0); }
    to { transform: scaleY(1); }
  }

  /* Line chart */
  .line-path {
    animation: drawLine .6s ease both;
  }
  .area-path {
    animation: fadeIn .8s ease both;
  }
  .dot {
    transition: all .15s ease;
  }
  @keyframes drawLine {
    from { stroke-dashoffset: 500; stroke-dasharray: 500; }
    to { stroke-dashoffset: 0; stroke-dasharray: 500; }
  }
  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .xa { display: flex; justify-content: space-between; margin-top: 8px; padding: 0 29px 0 32px; }
  .xa span { font: 400 8px/1 'Inter', sans-serif; color: var(--t4); font-variant-numeric: tabular-nums; }

  /* Detail panel */
  .detail {
    margin-top: 10px;
    background: rgba(255,255,255,0.03);
    border-radius: 8px;
    overflow: hidden;
    max-height: 0;
    opacity: 0;
    transition: max-height .3s cubic-bezier(.25,.8,.25,1), opacity .2s ease;
  }
  .detail.visible { max-height: 120px; opacity: 1; }
  .detail-inner {
    padding: 8px 10px;
    animation: detailFade .15s ease both;
  }
  @keyframes detailFade {
    from { opacity: 0; }
    to { opacity: 1; }
  }
  .detail-head {
    display: flex; justify-content: space-between; align-items: baseline;
    margin-bottom: 5px;
  }
  .detail-label { font: 600 10px/1 'Inter', sans-serif; color: var(--t1); }
  .detail-total { font: 600 10px/1 'Inter', sans-serif; color: var(--t1); font-variant-numeric: tabular-nums; }
  .detail-models { display: flex; flex-direction: column; gap: 2px; }
  .detail-row { display: flex; align-items: center; gap: 5px; }
  .detail-dot { width: 5px; height: 5px; border-radius: 1.5px; flex-shrink: 0; }
  .detail-name { font: 400 10px/1 'Inter', sans-serif; color: var(--t2); flex: 1; }
  .detail-cost { font: 500 10px/1 'Inter', sans-serif; color: var(--t1); font-variant-numeric: tabular-nums; }
</style>
