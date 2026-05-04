<script lang="ts">
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

<div class="group">
  <div class="group-label">Header</div>

  <!-- Live preview. Renders the real Toggle component with brand
       theming so the user sees the exact look of the popover header.
       `data-provider` on the wrapper restarts the brand-color cascade
       at this scope, so colors track `previewActive` instead of the
       popover's currently-active provider — including the "all" case,
       which has its own neutral-grey scope below (the previous version
       set the attribute to `null`, which made the "All" preview
       inherit whatever brand the popover happened to be on). -->
  <div class="header-preview" data-provider={previewActive}>
    {#if previewOptions.length > 0}
      <div class="header-preview-inner">
        <Toggle
          active={previewActive}
          options={previewOptions}
          brandTheming={current.brandTheming}
          onChange={(p) => (previewActive = p)}
        />
      </div>
    {:else}
      <div class="header-preview-empty">Enable at least one tab to see the preview.</div>
    {/if}
  </div>

  <!-- Chip row replaces the previous three-row table of toggle
       switches + label inputs. Each chip reads as "filled = visible,
       outlined = hidden," so the row itself communicates visibility
       state at a glance, and the brand icon makes the provider obvious
       without a separate title column. Click toggles visibility; the
       last enabled chip is locked so the popover is never left with
       zero tabs. -->
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
          {:else}
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <rect x="3.5" y="3.5" width="17" height="17" rx="4"></rect>
              <path d="M8 12h8"></path>
              <path d="M12 8v8"></path>
            </svg>
          {/if}
        </span>
        <span class="chip-label">{tab.title}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .group {
    margin-bottom: 8px;
  }
  /* `.group-label` is defined globally in `src/app.css`. */

  /* ── Preview surface ─────────────────────────────────────────────
     Compact-by-design: the hosted Toggle is the same component the
     popover renders at full size, but inside this preview we override
     its padding, font sizes, and logo dimensions to fit the smaller
     settings context. The `:global(...)` selectors reach through
     Svelte's CSS scoping into the Toggle's compiled classes — the
     only safe way to restyle a child component without forking it. */
  .header-preview {
    background: var(--surface-2);
    border-radius: 8px;
    padding: 6px 0 8px;
    margin-bottom: 8px;
    overflow: hidden;
  }
  .header-preview-inner {
    /* Settings preview is roughly 75% of the popover header's natural
       size — large enough to read brand colors and labels, small enough
       to leave room for the chip row underneath. */
    transform: scale(0.84);
    transform-origin: top center;
    margin-bottom: -10px; /* claw back the visual height the scale leaves */
  }
  .header-preview :global(.tog-wrap) {
    padding: 0 10px;
    animation: none; /* no entrance anim inside a preview surface */
  }
  .header-preview :global(.provider-logo) {
    padding: 0 2px 5px;
  }
  .header-preview :global(.provider-logo span) {
    font-size: 10.5px;
  }
  .header-preview :global(.tog button) {
    padding: 5px 7px;
    font-size: 10px;
  }
  .header-preview-empty {
    padding: 12px 12px;
    text-align: center;
    font: 400 9.5px/1.4 'Inter', sans-serif;
    color: var(--t4);
  }

  /* ── Chip-row visibility toggles ─────────────────────────────────
     Each chip is a clickable pill that reads as "filled = visible,
     outlined = hidden." The brand icon and the chip's surface treatment
     do all the work — there's no separate label column or toggle
     switch widget. Last-enabled chip locks (cursor: not-allowed +
     muted opacity) so the popover is never left without a tab.

     Layout uses CSS grid with `auto-fit` + `minmax` so the row wraps
     gracefully as more providers ship. With 3 providers all chips
     fit on one row; a 4th or 5th drops to a second row automatically.
     `minmax(96px, 1fr)` keeps each chip at least wide enough to fit
     the longest current title ("Claude Code") plus its icon and
     padding, while letting them grow to fill the row when there's
     extra space. */
  .chip-row {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(96px, 1fr));
    gap: 6px;
    padding: 0 2px;
  }
  .chip {
    min-width: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    padding: 6px 8px;
    border-radius: 7px;
    border: 1px solid var(--border-subtle);
    background: transparent;
    color: var(--t3);
    font: 500 10px/1 'Inter', sans-serif;
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
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
</style>
