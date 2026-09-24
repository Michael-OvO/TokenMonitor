<script lang="ts">
  import type { ResetTimelineLayout } from "../views/rateLimits.js";

  interface Props {
    layout: ResetTimelineLayout;
  }
  let { layout }: Props = $props();
</script>

<!-- A strip from today to the horizon with one dot per usage-limit reset at
     its expiry and week ticks for scale, then one chip per reset, in the
     same order, carrying its date and countdown. -->
<div class="rt">
  <div class="rt-track">
    {#each layout.weekTickPcts as pct}
      <span class="rt-tick" style="left: {pct}%"></span>
    {/each}
    {#each layout.markers as marker}
      <span
        class="rt-dot"
        class:urgent={marker.urgent}
        style="left: {marker.dotPct}%"
        title={marker.title}
      ></span>
    {/each}
  </div>
  <div class="rt-chips">
    {#each layout.markers as marker}
      <span class="rt-chip" class:urgent={marker.urgent} title={marker.title}>
        {marker.dateLabel}<span class="rt-chip-left">· {marker.leftLabel}</span>
      </span>
    {/each}
  </div>
</div>

<style>
  .rt {
    display: flex;
    flex-direction: column;
    gap: 6px;
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
  .rt-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .rt-chip {
    font: 500 9px/1 'Inter', sans-serif;
    font-variant-numeric: tabular-nums;
    color: var(--t2);
    background: var(--surface-2);
    border-radius: 4px;
    padding: 3px 6px;
    white-space: nowrap;
    cursor: default;
  }
  .rt-chip-left {
    margin-left: 0.35em;
    color: var(--t3);
    font-weight: 400;
  }
  .rt-chip.urgent,
  .rt-chip.urgent .rt-chip-left {
    color: var(--alert, #B05A52);
  }
</style>
