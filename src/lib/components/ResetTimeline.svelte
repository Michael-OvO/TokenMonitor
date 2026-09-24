<script lang="ts">
  import {
    assignChipSides,
    leaderTargetX,
    placeChips,
    type ChipSide,
    type ResetTimelineLayout,
  } from "../views/rateLimits.js";

  interface Props {
    layout: ResetTimelineLayout;
  }
  let { layout }: Props = $props();

  const CHIP_GAP = 4;
  const CHIP_H = 15;
  const ROW_GAP = 3;
  const LEADER_H = 8;
  const TRACK_H = 6;

  let stripWidth = $state(0);
  let chipWidths = $state<number[]>([]);

  let anchors = $derived(layout.markers.map((marker) => (marker.dotPct / 100) * stripWidth));
  let widths = $derived(layout.markers.map((_, i) => chipWidths[i] ?? 0));
  let sides = $derived(assignChipSides(anchors, widths, CHIP_GAP));

  interface Placed {
    side: ChipSide;
    leftPx: number;
    row: number;
  }
  let placed = $derived.by((): Placed[] => {
    const out: Placed[] = layout.markers.map(() => ({ side: "below", leftPx: 0, row: 0 }));
    for (const side of ["below", "above"] as const) {
      const members = sides.map((s, i) => (s === side ? i : -1)).filter((i) => i >= 0);
      const placements = placeChips(
        members.map((i) => anchors[i]),
        members.map((i) => widths[i]),
        stripWidth,
        CHIP_GAP,
      );
      members.forEach((i, k) => {
        out[i] = { side, leftPx: placements[k].leftPx, row: placements[k].row };
      });
    }
    return out;
  });

  const rowsOn = (side: ChipSide) =>
    placed.reduce((max, p) => (p.side === side ? Math.max(max, p.row + 1) : max), 0);
  let aboveRows = $derived(rowsOn("above"));
  let belowRows = $derived(rowsOn("below"));
  const rowsHeight = (rows: number) => (rows > 0 ? rows * CHIP_H + (rows - 1) * ROW_GAP : 0);
  let aboveHeight = $derived(aboveRows > 0 ? rowsHeight(aboveRows) + LEADER_H : 0);
  let trackTop = $derived(aboveHeight);
  let belowTop = $derived(trackTop + TRACK_H + LEADER_H);
  let totalHeight = $derived(belowTop + rowsHeight(belowRows));

  /** Top of a chip within the component; above-bar rows stack outward from the bar. */
  function chipTop(p: Placed): number {
    return p.side === "below"
      ? belowTop + p.row * (CHIP_H + ROW_GAP)
      : (aboveRows - 1 - p.row) * (CHIP_H + ROW_GAP);
  }
  /** Where the leader meets the chip: its top edge below the bar, its bottom edge above. */
  function chipEdgeY(p: Placed): number {
    return p.side === "below" ? chipTop(p) : chipTop(p) + CHIP_H;
  }
</script>

<!-- A strip from today to the horizon with one dot per usage-limit reset at
     its expiry and week ticks for scale. Each reset's chip (date and
     countdown) sits centred on its dot; a chip that would collide with its
     predecessor goes to the other side of the bar. Leader lines tie each
     chip to its dot. -->
<div class="rt" style="height: {totalHeight}px" bind:clientWidth={stripWidth}>
  <svg class="rt-leaders" width={stripWidth} height={totalHeight} aria-hidden="true">
    {#each layout.markers as marker, i}
      {@const p = placed[i]}
      <line
        class="rt-leader"
        class:urgent={marker.urgent}
        x1={anchors[i]}
        y1={trackTop + TRACK_H / 2}
        x2={leaderTargetX(anchors[i], p.leftPx, widths[i])}
        y2={chipEdgeY(p)}
      />
    {/each}
  </svg>
  <div class="rt-track" style="top: {trackTop}px">
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
  {#each layout.markers as marker, i}
    <span
      class="rt-chip"
      class:urgent={marker.urgent}
      style="left: {placed[i].leftPx}px; top: {chipTop(placed[i])}px"
      title={marker.title}
      bind:clientWidth={chipWidths[i]}
    >{marker.dateLabel}<span class="rt-chip-left">· {marker.leftLabel}</span></span>
  {/each}
</div>

<style>
  .rt {
    position: relative;
    margin-top: 2px;
  }
  .rt-leaders {
    position: absolute;
    inset: 0;
    display: block;
    overflow: visible;
    pointer-events: none;
  }
  .rt-leader {
    stroke: var(--t4);
    stroke-width: 1;
  }
  .rt-leader.urgent {
    stroke: var(--alert, #B05A52);
  }
  .rt-track {
    position: absolute;
    left: 0;
    right: 0;
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
  .rt-chip {
    position: absolute;
    display: inline-flex;
    align-items: center;
    height: 15px;
    box-sizing: border-box;
    padding: 0 6px;
    font: 500 9px/1 'Inter', sans-serif;
    font-variant-numeric: tabular-nums;
    color: var(--t2);
    background: var(--surface-2);
    border-radius: 4px;
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
