import { describe, expect, it } from "vitest";
import { parseCssColor, readSurfaceColor } from "./windowAppearance.js";

describe("parseCssColor", () => {
  it("parses 6-digit hex colors", () => {
    expect(parseCssColor("#141416")).toEqual({
      red: 20,
      green: 20,
      blue: 22,
      alpha: 255,
    });
  });

  it("parses 8-digit hex colors", () => {
    expect(parseCssColor("#ffffff80")).toEqual({
      red: 255,
      green: 255,
      blue: 255,
      alpha: 128,
    });
  });

  it("parses rgba() colors", () => {
    expect(parseCssColor("rgba(74, 123, 157, 0.25)")).toEqual({
      red: 74,
      green: 123,
      blue: 157,
      alpha: 64,
    });
  });

  it("returns null for unsupported colors", () => {
    expect(parseCssColor("transparent")).toBeNull();
    expect(parseCssColor("")).toBeNull();
  });
});

describe("readSurfaceColor", () => {
  it("reads the --surface CSS variable from the provided root", () => {
    const root = {} as HTMLElement;

    expect(
      readSurfaceColor(root, () => ({
        getPropertyValue: () => " #FFFFFF ",
      }) as CSSStyleDeclaration),
    ).toEqual({
      red: 255,
      green: 255,
      blue: 255,
      alpha: 255,
    });
  });
});
