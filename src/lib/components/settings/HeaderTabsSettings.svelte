<script lang="ts">
  import SettingsDisclosure from "./SettingsDisclosure.svelte";
  import {
    DEFAULT_HEADER_TABS,
    settings,
    updateSetting,
    type Settings as SettingsType,
  } from "../../stores/settings.js";
  import {
    getUsageProviderLogoKind,
    getUsageProviderTitle,
    USAGE_PROVIDER_ORDER,
    type UsageProviderLogoKind,
  } from "../../providerMetadata.js";
  import type { HeaderTabConfig, UsageProvider } from "../../types/index.js";
  import Toggle from "../Toggle.svelte";
  import ProviderLogo from "../ProviderLogo.svelte";

  let current = $derived($settings as SettingsType);

  const HEADER_TAB_FIELDS: Array<{
    provider: UsageProvider;
    title: string;
    logoKind: UsageProviderLogoKind;
  }> = USAGE_PROVIDER_ORDER.map((provider) => ({
    provider,
    title: getUsageProviderTitle(provider),
    logoKind: getUsageProviderLogoKind(provider),
  }));

  /** Live-updated tab list for the preview Toggle. Labels always come
   * from `DEFAULT_HEADER_TABS` now that user-customizable text has been
   * removed; we still read the per-provider `enabled` flag so toggling
   * a chip below removes the segment from the preview immediately. */
  let previewOptions = $derived.by(() =>
    HEADER_TAB_FIELDS.filter(({ provider }) => current.headerTabs[provider].enabled).map(
      ({ provider }) => ({
        value: provider,
        label: DEFAULT_HEADER_TABS[provider]?.label ?? provider,
      }),
    ),
  );

  /** Local active-provider state for the preview. Independent of the
   * popover's real `activeProvider` so clicking a preview tab doesn't
   * navigate the user away. Repinned to the first visible tab whenever
   * the visible set changes. */
  let previewActive = $state<UsageProvider>(USAGE_PROVIDER_ORDER[0]);
  let headerExpanded = $state(false);
  $effect(() => {
    const visible = previewOptions;
    if (visible.length === 0) return;
    if (!visible.some((opt) => opt.value === previewActive)) {
      previewActive = visible[0].value;
    }
  });

  function updateHeaderTab(provider: UsageProvider, patch: Partial<HeaderTabConfig>) {
    updateSetting("headerTabs", {
      ...current.headerTabs,
      [provider]: {
        ...current.headerTabs[provider],
        ...patch,
      },
    });
  }

  /** Toggle the visibility of a tab. The popover guarantees at least
   * one tab is visible at any time (`getVisibleHeaderProviders` falls
   * back to the first provider when none are enabled), but we still
   * disable the click on the last-enabled chip so the user gets visible
   * feedback that the action is blocked rather than silently ignored.
   *
   * UX nicety: enabling a chip auto-pins the preview to that provider
   * so the user sees the result of their action immediately.
   * Disabling the currently-previewed chip falls through to the
   * `$effect` that repoints `previewActive` to the next visible tab,
   * so the preview never goes blank mid-toggle. */
  function handleToggleVisibility(provider: UsageProvider) {
    const isOn = current.headerTabs[provider].enabled;
    if (isOn && countEnabled() <= 1) return;
    const next = !isOn;
    updateHeaderTab(provider, { enabled: next });
    if (next) previewActive = provider;
  }

  function countEnabled(): number {
    return HEADER_TAB_FIELDS.reduce(
      (acc, { provider }) => acc + (current.headerTabs[provider].enabled ? 1 : 0),
      0,
    );
  }
</script>

<div class="block">
  <button class="row collapsible-toggle" type="button" aria-expanded={headerExpanded} onclick={() => (headerExpanded = !headerExpanded)}>
    <span class="label">Header Tabs</span>
    <div class="collapsible-right">
      <span class="count">{countEnabled()} of {HEADER_TAB_FIELDS.length}</span>
      <svg class="collapsible-chevron" class:open={headerExpanded} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="6 9 12 15 18 9"></polyline>
      </svg>
    </div>
  </button>
  <SettingsDisclosure open={headerExpanded}>
    <div class="header-content">
      <div class="header-preview" data-provider={previewActive}>
        {#if previewOptions.length > 0}
          <Toggle
            active={previewActive}
            options={previewOptions}
            brandTheming={current.brandTheming}
            onChange={(p) => (previewActive = p)}
          />
        {:else}
          <div class="header-preview-empty">Enable at least one tab to see the preview.</div>
        {/if}
      </div>
      <div class="chip-row" role="group" aria-label="Header tabs visibility">
        {#each HEADER_TAB_FIELDS as tab}
          {@const enabled = current.headerTabs[tab.provider].enabled}
          {@const lastEnabled = enabled && countEnabled() <= 1}
          <button
            type="button"
            class="chip"
            class:on={enabled}
            data-provider={tab.provider}
            disabled={lastEnabled}
            aria-pressed={enabled}
            aria-label={`${enabled ? "Hide" : "Show"} ${tab.title} tab`}
            title={lastEnabled ? "At least one tab must stay visible." : tab.title}
            onclick={() => handleToggleVisibility(tab.provider)}
          >
            <span class={`chip-logo ${tab.logoKind}`} aria-hidden="true">
              <ProviderLogo kind={tab.logoKind} size={13} />
            </span>
            <span class="chip-label">{tab.title}</span>
            <svg class="chip-check" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <polyline points="5 12 10 17 19 7"></polyline>
            </svg>
          </button>
        {/each}
      </div>
    </div>
  </SettingsDisclosure>
</div>

<style>
  .block {
    border-top: 1px solid var(--border-subtle);
  }
  .row {
    padding: 7px 10px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .collapsible-toggle {
    width: 100%;
    background: none;
    border: none;
    cursor: pointer;
    user-select: none;
  }
  .collapsible-toggle:hover {
    background: var(--surface-hover);
  }
  .collapsible-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .collapsible-chevron {
    color: var(--t3);
    transform: rotate(-90deg);
  }
  .collapsible-chevron.open {
    transform: rotate(0deg);
  }
  .label {
    font: 400 10px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .count {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
  }

  .header-content {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 4px 12px 16px;
  }

  /* Keep the live preview at its natural size so its visual and layout
     bounds agree. Only the visibility controls below may wrap. */
  .header-preview {
    background: var(--surface-2);
    border-radius: 6px;
    padding: 12px;
  }
  .header-preview :global(.tog-wrap) {
    padding: 0;
    animation: none;
  }
  .header-preview :global(.provider-logo) {
    padding: 0 2px 8px;
    animation: none;
  }
  .header-preview :global(.provider-logo span) {
    font-size: 10.5px;
  }
  .header-preview :global(.tog button) {
    padding: 6px 3px;
    font-size: 9px;
    color: var(--t2);
  }
  .header-preview :global(.tog button.on) {
    color: var(--t1);
  }
  .header-preview-empty {
    padding: 12px 12px;
    text-align: center;
    font: 400 9.5px/1.4 'Inter', sans-serif;
    color: var(--t4);
  }

  /* Full provider names and a checkmark stay readable in the narrow
     popover. Wider containers can accommodate more columns. */
  .chip-row {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 120px), 1fr));
    gap: 8px;
  }
  .chip {
    min-width: 0;
    min-height: 32px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
    border-radius: 6px;
    border: 1px solid var(--border-subtle);
    background: transparent;
    color: var(--t2);
    font: 500 10px/1.3 'Inter', sans-serif;
    text-align: left;
    letter-spacing: -0.02px;
    cursor: pointer;
    transition:
      background var(--t-fast, 120ms) ease,
      border-color var(--t-fast, 120ms) ease,
      color var(--t-fast, 120ms) ease,
      transform var(--t-fast, 120ms) ease;
  }
  .chip:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--t1);
  }
  .chip:active:not(:disabled) {
    transform: translateY(0.5px);
  }
  /* Filled = visible. Brand-tinted background and accent-colored icon
     so a glance reads "Claude is on / Codex is off" without parsing
     text. The accent comes from the chip's own `data-provider` scope
     below, so each enabled chip carries its OWN brand color, not the
     popover's. */
  .chip.on {
    background: var(--accent-soft, rgba(255, 255, 255, 0.06));
    border-color: transparent;
    color: var(--t1);
  }
  .chip.on .chip-logo {
    color: var(--accent, var(--t1));
  }
  .chip:disabled {
    cursor: not-allowed;
    opacity: 0.7;
  }
  .chip-logo {
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--t3);
    transition: color var(--t-fast, 120ms) ease;
  }
  .chip-label {
    flex: 1;
    min-width: 0;
  }
  .chip-check {
    flex-shrink: 0;
    opacity: 0;
  }
  .chip.on .chip-check {
    opacity: 1;
  }
  .chip:focus-visible,
  .collapsible-toggle:focus-visible {
    outline: 2px solid var(--t2);
    outline-offset: -2px;
  }
  @media (prefers-reduced-motion: reduce) {
    .chip,
    .chip-logo {
      transition: none;
    }
  }

  /* ── Brand-color scoping ─────────────────────────────────────────
     Both the preview wrapper and each individual chip carry their own
     `data-provider` attribute, so `--accent` and `--accent-soft` resolve
     to the correct brand colors without inheriting the popover's
     active provider. Light-theme variants are duplicated locally
     because the global `[data-theme="light"][data-provider="…"]`
     selectors require both attributes on the same element (data-theme
     lives on <html>) and so can't reach this scope. If the brand
     colors in `app.css` change, update both locations.

     The "all" scope is critical: the popover normally *removes*
     data-provider when it's on "All", and the cascade falls back to
     the html-level neutral defaults. But here we want the chip / preview
     wrapper to be brand-neutral *regardless of where the popover is*,
     so we set `--accent` / `--accent-soft` explicitly on
     `[data-provider="all"]` rather than relying on the absence of the
     attribute. Without this rule, an enabled "All" chip while the
     popover sat on "Codex" would still show as Codex-blue. */
  .header-preview[data-provider="all"],
  .chip[data-provider="all"] {
    --accent: var(--t2);
    --accent-soft: rgba(255, 255, 255, 0.08);
  }
  :global([data-theme="light"]) .header-preview[data-provider="all"],
  :global([data-theme="light"]) .chip[data-provider="all"] {
    --accent-soft: rgba(0, 0, 0, 0.06);
  }
  .header-preview[data-provider="claude"],
  .chip[data-provider="claude"] {
    --accent: #E8784A;
    --accent-soft: rgba(232, 120, 74, 0.12);
  }
  :global([data-theme="light"]) .header-preview[data-provider="claude"],
  :global([data-theme="light"]) .chip[data-provider="claude"] {
    --accent: #C85E2A;
    --accent-soft: rgba(200, 94, 42, 0.14);
  }
  .header-preview[data-provider="codex"],
  .chip[data-provider="codex"] {
    --accent: #52A8DC;
    --accent-soft: rgba(82, 168, 220, 0.12);
  }
  :global([data-theme="light"]) .header-preview[data-provider="codex"],
  :global([data-theme="light"]) .chip[data-provider="codex"] {
    --accent: #2E7EB5;
    --accent-soft: rgba(46, 126, 181, 0.14);
  }
  .header-preview[data-provider="kimi"],
  .chip[data-provider="kimi"] {
    --accent: #8B6FD4;
    --accent-soft: rgba(139, 111, 212, 0.12);
  }
  :global([data-theme="light"]) .header-preview[data-provider="kimi"],
  :global([data-theme="light"]) .chip[data-provider="kimi"] {
    --accent: #6E52C6;
    --accent-soft: rgba(110, 82, 198, 0.14);
  }
</style>
