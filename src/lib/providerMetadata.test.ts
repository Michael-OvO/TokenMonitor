import { describe, expect, it } from "vitest";

import { isRateLimitMissingMetadataError } from "./providerMetadata.js";

describe("isRateLimitMissingMetadataError (kimi)", () => {
  it("treats an expired CLI sign-in as missing metadata, not a hard error", () => {
    expect(
      isRateLimitMissingMetadataError(
        "kimi",
        "Kimi Code CLI sign-in has expired on this machine; run `kimi` and log in again (status=400 Bad Request body={\"error\":\"invalid_grant\"})",
      ),
    ).toBe(true);
  });

  it("still treats the not-signed-in and unreadable-credentials cases as missing metadata", () => {
    expect(isRateLimitMissingMetadataError("kimi", "Kimi Code CLI is not signed in on this machine")).toBe(true);
    expect(
      isRateLimitMissingMetadataError("kimi", "Failed to read Kimi credentials at /x: No such file or directory"),
    ).toBe(true);
  });

  it("keeps genuine API failures as errors", () => {
    expect(isRateLimitMissingMetadataError("kimi", "Usage API returned 500 Internal Server Error")).toBe(false);
    expect(isRateLimitMissingMetadataError("kimi", "Kimi token refresh failed: network: timed out")).toBe(false);
  });
});
