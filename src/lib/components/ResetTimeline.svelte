<script lang="ts">
  import type { ResetTimelineLayout } from "../views/rateLimits.js";

  interface Props {
    layout: ResetTimelineLayout;
  }
  let { layout }: Props = $props();

  let staggered = $derived(layout.markers.some((marker) => marker.labelRow === 1));
</script>

<!-- A strip from today to the horizon: one dot per usage-limit reset at its
     expiry, week ticks for scale, the date under each dot. Hovering the strip
     flips every date to its countdown, so both readings fit in one row. -->
<div class="rt" class:staggered>
  <div class="rt-track">
    {#each layout.weekTickPcts as pct}
      <span class="rt-tick" style="left: {pct}%"></span>
    {/each}
    {#each layout.markers as marker}
      <span
        class="rt-dot"
        class:urgent={marker.urgent}
        class:cluster={marker.count > 1}
        style="left: {marker.leftPct}%"
        title={marker.title}
      ></span>
    {/each}
  </div>
  <div class="rt-labels">
    <span class="rt-cap rt-cap-start">today</span>
    {#each layout.markers as marker}
      <span
        class="rt-date"
        class:urgent={marker.urgent}
        class:row1={marker.labelRow === 1}
        style="left: {marker.leftPct}%"
        title={marker.title}
      ><span class="rt-abs">{marker.dateLabel}</span><span class="rt-rel">in {marker.leftLabel}</span>{#if marker.count > 1}<span class="rt-count">×{marker.count}</span>{/if}</span>
    {/each}
    <span class="rt-cap rt-cap-end">{layout.horizonLabel}</span>
  </div>
</div>

<style>
  .rt {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding-top: 2px;
  }
  .rt-track {
    position: relative;
    height: 6px;
    background: var(--surface-2);
    border-radius: 3px;
  }
  .rt-tick {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--border);
    transform: translateX(-50%);
  }
  .rt-dot {
    position: absolute;
    top: 50%;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    /* A ring in the card colour keeps neighbouring dots apart. */
    box-shadow: 0 0 0 2px var(--surface);
    transform: translate(-50%, -50%);
    cursor: default;
  }
  .rt-dot.urgent {
    background: var(--alert, #B05A52);
  }
  .rt-dot.cluster {
    width: 8px;
    height: 8px;
  }
  .rt-count {
    margin-left: 3px;
    color: var(--t4);
  }
  .rt-labels {
    position: relative;
    height: 10px;
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
    font-variant-numeric: tabular-nums;
  }
  .rt.staggered .rt-labels {
    height: 21px;
  }
  .rt-date {
    position: absolute;
    top: 0;
    transform: translateX(-50%);
    white-space: nowrap;
    cursor: default;
  }
  .rt-date.row1 {
    top: 11px;
  }
  .rt-date.urgent {
    color: var(--alert, #B05A52);
  }
  .rt-rel {
    display: none;
  }
  .rt:hover .rt-abs {
    display: none;
  }
  .rt:hover .rt-rel {
    display: inline;
  }
  .rt-cap {
    position: absolute;
    top: 0;
    color: var(--t4);
  }
  .rt-cap-start {
    left: 0;
  }
  .rt-cap-end {
    right: 0;
  }
</style>
