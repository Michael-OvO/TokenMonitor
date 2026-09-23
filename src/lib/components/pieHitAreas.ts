/**
 * Invisible hit areas for the donut chart.
 *
 * The visible ring leaves a sliver between neighbouring slices and a hole in
 * the middle that belong to no category, so a pointer sliding from one slice
 * to the next passes through spots where nothing is hovered and the centre
 * label falls back to the total on the way. These sectors fan out from the
 * centre with no gaps and cover the whole disc; the ring's mouse handlers
 * live on them, so wherever the pointer is inside the disc it is over
 * exactly one slice.
 *
 * Returns one path per weight, in order (a zero weight yields a degenerate
 * sector so indices stay aligned with the visible arcs); nothing when the
 * weights add up to nothing.
 */
export function pieHitSectorPaths(weights: number[], cx: number, cy: number, radius: number): string[] {
  const total = weights.reduce((sum, w) => sum + Math.max(0, w), 0);
  if (total <= 0) return [];
  if (weights.length === 1) {
    // A single arc cannot describe a full circle, so the disc is two halves.
    return [
      `M ${cx - radius} ${cy} A ${radius} ${radius} 0 1 1 ${cx + radius} ${cy} A ${radius} ${radius} 0 1 1 ${cx - radius} ${cy} Z`,
    ];
  }
  let angle = -Math.PI / 2;
  return weights.map((w) => {
    const span = (Math.max(0, w) / total) * Math.PI * 2;
    const a0 = angle;
    const a1 = angle + span;
    angle = a1;
    const large = span > Math.PI ? 1 : 0;
    const x0 = cx + radius * Math.cos(a0);
    const y0 = cy + radius * Math.sin(a0);
    const x1 = cx + radius * Math.cos(a1);
    const y1 = cy + radius * Math.sin(a1);
    return `M ${cx} ${cy} L ${x0} ${y0} A ${radius} ${radius} 0 ${large} 1 ${x1} ${y1} Z`;
  });
}
