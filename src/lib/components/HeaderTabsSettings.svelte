<script lang="ts">
  import SettingsDisclosure from "./SettingsDisclosure.svelte";
  import {
    DEFAULT_HEADER_TABS,
    settings,
    updateSetting,
    type Settings as SettingsType,
  } from "../stores/settings.js";
  import {
    getUsageProviderLogoKind,
    getUsageProviderTitle,
    USAGE_PROVIDER_ORDER,
    type UsageProviderLogoKind,
  } from "../providerMetadata.js";
  import type { HeaderTabConfig, UsageProvider } from "../types/index.js";
  import Toggle from "./Toggle.svelte";

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
              {#if tab.logoKind === "all"}
                <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd">
                  <path d="M2.4,12 A9.6,9.6 0 1,1 21.6,12 A9.6,9.6 0 1,1 2.4,12 M7.35,9.9 A1.65,1.65 0 1,1 10.65,9.9 A1.65,1.65 0 1,1 7.35,9.9 M13.35,9.9 A1.65,1.65 0 1,1 16.65,9.9 A1.65,1.65 0 1,1 13.35,9.9"/>
                </svg>
              {:else if tab.logoKind === "claude"}
                <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd">
                  <path clip-rule="evenodd" d="M20.998 10.949H24v3.102h-3v3.028h-1.487V20H18v-2.921h-1.487V20H15v-2.921H9V20H7.488v-2.921H6V20H4.487v-2.921H3V14.05H0V10.95h3V5h17.998v5.949zM6 10.949h1.488V8.102H6v2.847zm10.51 0H18V8.102h-1.49v2.847z"/>
                </svg>
              {:else if tab.logoKind === "codex"}
                <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd">
                  <path clip-rule="evenodd" d="M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z"/>
                </svg>
              {:else if tab.logoKind === "cursor"}
                <svg width="13" height="13" viewBox="0 0 512 512" fill="currentColor">
                  <path d="m415.035 156.35-151.503-87.4695c-4.865-2.8094-10.868-2.8094-15.733 0l-151.4969 87.4695c-4.0897 2.362-6.6146 6.729-6.6146 11.459v176.383c0 4.73 2.5249 9.097 6.6146 11.458l151.5039 87.47c4.865 2.809 10.868 2.809 15.733 0l151.504-87.47c4.089-2.361 6.614-6.728 6.614-11.458v-176.383c0-4.73-2.525-9.097-6.614-11.459zm-9.516 18.528-146.255 253.32c-.988 1.707-3.599 1.01-3.599-.967v-165.872c0-3.314-1.771-6.379-4.644-8.044l-143.645-82.932c-1.707-.988-1.01-3.599.968-3.599h292.509c4.154 0 6.75 4.503 4.673 8.101h-.007z"/>
                </svg>
              {:else if tab.logoKind === "kimi"}
                <!-- Kimi wordmark glyph (LobeHub icon set, https://lobehub.com/icons/kimi) -->
                <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd">
                  <path d="M21.846 0a1.923 1.923 0 110 3.846H20.15a.226.226 0 01-.227-.226V1.923C19.923.861 20.784 0 21.846 0z"/>
                  <path d="M11.065 11.199l7.257-7.2c.137-.136.06-.41-.116-.41H14.3a.164.164 0 00-.117.051l-7.82 7.756c-.122.12-.302.013-.302-.179V3.82c0-.127-.083-.23-.185-.23H3.186c-.103 0-.186.103-.186.23V19.77c0 .128.083.23.186.23h2.69c.103 0 .186-.102.186-.23v-3.25c0-.069.025-.135.069-.178l2.424-2.406a.158.158 0 01.205-.023l6.484 4.772a7.677 7.677 0 003.453 1.283c.108.012.2-.095.2-.23v-3.06c0-.117-.07-.212-.164-.227a5.028 5.028 0 01-2.027-.807l-5.613-4.064c-.117-.078-.132-.279-.028-.381z"/>
                </svg>
              {:else}
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                  <rect x="3.5" y="3.5" width="17" height="17" rx="4"></rect>
                  <path d="M8 12h8"></path>
                  <path d="M12 8v8"></path>
                </svg>
              {/if}
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
