import { describe, expect, it } from "vitest";

import { formatUsage, parseArgs } from "./cli.mjs";
import { detectHostArch, detectHostPlatformId, resolveRequestedPlatform } from "./platform.mjs";

describe("build cli", () => {
  it("parses the supported flags", () => {
    expect(
      parseArgs(["--platform", "linux", "--ci", "--clean", "--verbose"]),
    ).toEqual({
      platform: "linux",
      ci: true,
      clean: true,
      verbose: true,
      help: false,
    });
  });

  it("supports --platform=value syntax", () => {
    expect(parseArgs(["--platform=windows"])).toMatchObject({ platform: "windows" });
  });

  it("rejects missing platform values", () => {
    expect(() => parseArgs(["--platform"])).toThrow("Missing value for --platform");
  });

  it("prints a usage block", () => {
    expect(formatUsage()).toContain("--platform <current|macos|windows|linux>");
  });
});

describe("platform helpers", () => {
  it("maps supported node platforms", () => {
    expect(detectHostPlatformId("darwin")).toBe("macos");
    expect(detectHostPlatformId("win32")).toBe("windows");
    expect(detectHostPlatformId("linux")).toBe("linux");
  });

  it("normalizes the common architectures", () => {
    expect(detectHostArch("x64")).toBe("x64");
    expect(detectHostArch("arm64")).toBe("arm64");
  });

  it("fails fast on cross-platform requests", () => {
    expect(() => resolveRequestedPlatform("linux", "windows")).toThrow(
      "Cross-platform builds are not supported",
    );
  });

  it("uses the host platform for current", () => {
    expect(resolveRequestedPlatform("current", "macos")).toMatchObject({ id: "macos" });
  });
});
