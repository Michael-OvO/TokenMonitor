import { describe, expect, it } from "vitest";
import { pieHitSectorPaths } from "./pieHitAreas.js";

const CX = 50;
const CY = 50;
const R = 46;

/** The end point of every drawing command in a path, in order. */
function points(path: string): Array<[number, number]> {
  const tokens = path.trim().split(/\s+/);
  const out: Array<[number, number]> = [];
  for (let i = 0; i < tokens.length; i++) {
    const cmd = tokens[i];
    if (cmd === "M" || cmd === "L") {
      out.push([Number(tokens[i + 1]), Number(tokens[i + 2])]);
      i += 2;
    } else if (cmd === "A") {
      out.push([Number(tokens[i + 6]), Number(tokens[i + 7])]);
      i += 7;
    } else if (cmd !== "Z") {
      throw new Error(`unexpected token ${cmd}`);
    }
  }
  return out;
}

describe("pieHitSectorPaths", () => {
  it("returns nothing without positive weights", () => {
    expect(pieHitSectorPaths([], CX, CY, R)).toEqual([]);
    expect(pieHitSectorPaths([0, 0], CX, CY, R)).toEqual([]);
  });

  it("covers the whole disc with one sector for a single slice", () => {
    const [disc, ...rest] = pieHitSectorPaths([7], CX, CY, R);
    expect(rest).toEqual([]);
    expect(disc).toMatch(/^M 4 50 A 46 46 0 1 1 96 50 A 46 46 0 1 1 4 50 Z$/);
  });

  it("makes one sector per slice, each fanning out from the centre", () => {
    const paths = pieHitSectorPaths([4, 3, 2, 1], CX, CY, R);
    expect(paths).toHaveLength(4);
    for (const p of paths) {
      expect(p.startsWith(`M ${CX} ${CY} L `)).toBe(true);
      expect(p.endsWith(" Z")).toBe(true);
      const [, edge] = points(p);
      expect(Math.hypot(edge[0] - CX, edge[1] - CY)).toBeCloseTo(R, 9);
    }
  });

  it("leaves no angular gap: each sector ends exactly where the next begins", () => {
    const paths = pieHitSectorPaths([4, 3, 2, 1], CX, CY, R);
    const rims = paths.map((p) => {
      const pts = points(p);
      return { start: pts[1], end: pts[pts.length - 1] };
    });
    for (let i = 0; i < rims.length; i++) {
      const next = rims[(i + 1) % rims.length];
      expect(rims[i].end[0]).toBeCloseTo(next.start[0], 9);
      expect(rims[i].end[1]).toBeCloseTo(next.start[1], 9);
    }
  });

  it("starts the first sector at twelve o'clock like the visible ring", () => {
    const [first] = pieHitSectorPaths([1, 1], CX, CY, R);
    const [, start] = points(first);
    expect(start[0]).toBeCloseTo(CX, 9);
    expect(start[1]).toBeCloseTo(CY - R, 9);
  });

  it("flags sectors wider than a half turn as large arcs", () => {
    const [big, small] = pieHitSectorPaths([3, 1], CX, CY, R);
    expect(big).toMatch(/ A 46 46 0 1 1 /);
    expect(small).toMatch(/ A 46 46 0 0 1 /);
  });
});
