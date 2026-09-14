import { describe, expect, it } from "vitest";
import { setRemoteDeviceIncludeFlag } from "./deviceStats.js";

describe("setRemoteDeviceIncludeFlag", () => {
  it("updates an existing archive-only device include flag", () => {
    const devices = [
      { alias: "peer-a", include_in_stats: true },
      { alias: "peer-b", include_in_stats: true },
    ];

    expect(setRemoteDeviceIncludeFlag(devices, "peer-a", false)).toEqual([
      { alias: "peer-a", include_in_stats: false },
      { alias: "peer-b", include_in_stats: true },
    ]);
  });

  it("adds a new archive-only device include flag", () => {
    expect(setRemoteDeviceIncludeFlag([], "peer-a", false)).toEqual([
      { alias: "peer-a", include_in_stats: false },
    ]);
  });
});
