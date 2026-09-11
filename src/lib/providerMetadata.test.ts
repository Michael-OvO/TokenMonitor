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

import {
  createDefaultHeaderTabs,
  enabledIntegrationIds,
  RATE_LIMIT_PROVIDER_ORDER,
  rateLimitProvidersForScope,
  resolveUsageScope,
  usageScopeForAll,
  usageScopeProviders,
} from "./providerMetadata.js";
import type { HeaderTabs } from "./types/index.js";

function tabsWithDisabled(disabled: string[] = []): HeaderTabs {
  const tabs = createDefaultHeaderTabs();
  for (const id of disabled) tabs[id] = { ...tabs[id], enabled: false };
  return tabs;
}

describe("usage scope for the All tab", () => {
  it("is `all` when every integration tab is enabled", () => {
    expect(usageScopeForAll(tabsWithDisabled())).toBe("all");
  });

  it("ignores the All tab's own visibility flag", () => {
    expect(usageScopeForAll(tabsWithDisabled(["all"]))).toBe("all");
  });

  it("joins the enabled integrations in canonical order", () => {
    expect(usageScopeForAll(tabsWithDisabled(["codex"]))).toBe("claude+cursor+kimi");
    expect(enabledIntegrationIds(tabsWithDisabled(["cursor", "codex"]))).toEqual(["claude", "kimi"]);
  });

  it("collapses to the single id when only one integration is enabled", () => {
    expect(usageScopeForAll(tabsWithDisabled(["claude", "codex", "cursor"]))).toBe("kimi");
  });

  it("only rewrites the All provider", () => {
    expect(resolveUsageScope("codex", tabsWithDisabled(["codex"]))).toBe("codex");
    expect(resolveUsageScope("all", tabsWithDisabled(["codex"]))).toBe("claude+cursor+kimi");
  });

  it("expands a scope string back into its providers", () => {
    expect(usageScopeProviders("all")).toEqual(["claude", "codex", "cursor", "kimi"]);
    expect(usageScopeProviders("claude+kimi")).toEqual(["claude", "kimi"]);
    expect(usageScopeProviders("codex")).toEqual(["codex"]);
  });

  it("limits rate-limit providers to the scope", () => {
    expect(rateLimitProvidersForScope("all")).toEqual(RATE_LIMIT_PROVIDER_ORDER);
    expect(rateLimitProvidersForScope("claude+kimi")).not.toContain("codex");
    expect(rateLimitProvidersForScope("claude+kimi")).toEqual(
      RATE_LIMIT_PROVIDER_ORDER.filter((id) => id === "claude" || id === "kimi"),
    );
  });
});
