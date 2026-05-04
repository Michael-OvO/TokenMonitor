import { describe, expect, it } from "vitest";
import {
  CHANGELOG,
  CURRENT_ONBOARDING_VERSION,
  changelogSince,
  compareSemver,
} from "./changelog.js";

describe("compareSemver", () => {
  it("orders patch components numerically not lexicographically", () => {
    // The bug we're guarding against: string-compare puts "0.10" before
    // "0.9". A regression here would re-onboard users who shouldn't be.
    expect(compareSemver("0.10.0", "0.9.0")).toBeGreaterThan(0);
    expect(compareSemver("1.2.3", "1.2.10")).toBeLessThan(0);
  });

  it("treats missing patch component as zero", () => {
    expect(compareSemver("0.12", "0.12.0")).toBe(0);
    expect(compareSemver("0.12.1", "0.12")).toBeGreaterThan(0);
  });

  it("ignores pre-release suffix when comparing", () => {
    expect(compareSemver("0.12.0-rc.1", "0.12.0")).toBe(0);
  });
});

describe("changelogSince", () => {
  it("returns nothing for fresh installs", () => {
    expect(changelogSince(null)).toEqual([]);
  });

  it("returns only entries strictly newer than the given version", () => {
    const result = changelogSince("0.11.0");
    expect(result.length).toBeGreaterThan(0);
    for (const entry of result) {
      expect(compareSemver(entry.version, "0.11.0")).toBeGreaterThan(0);
    }
  });

  it("returns nothing when caller is already on the current version", () => {
    expect(changelogSince(CURRENT_ONBOARDING_VERSION)).toEqual([]);
  });
});

describe("CHANGELOG data", () => {
  it("is non-empty and has the current version as the newest entry", () => {
    expect(CHANGELOG.length).toBeGreaterThan(0);
    const newest = CHANGELOG[0];
    expect(compareSemver(newest.version, CURRENT_ONBOARDING_VERSION)).toBe(0);
  });

  it("is sorted newest-first", () => {
    for (let i = 1; i < CHANGELOG.length; i++) {
      expect(
        compareSemver(CHANGELOG[i - 1].version, CHANGELOG[i].version),
      ).toBeGreaterThan(0);
    }
  });

  it("every entry has a non-empty title and at least one highlight", () => {
    // Highlights are the entire What's-New rendering — without at
    // least one, the version block has nothing to show. Each highlight
    // must carry both a title and a body so the card never renders
    // half-empty.
    for (const entry of CHANGELOG) {
      expect(entry.title.trim().length).toBeGreaterThan(0);
      expect(entry.highlights.length).toBeGreaterThan(0);
      for (const highlight of entry.highlights) {
        expect(highlight.title.trim().length).toBeGreaterThan(0);
        expect(highlight.description.trim().length).toBeGreaterThan(0);
      }
    }
  });
});
