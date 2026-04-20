<script lang="ts">
  import {
    DEFAULT_HEADER_TABS,
    MAX_HEADER_TAB_LABEL_LENGTH,
    areHeaderTabsEqual,
    settings,
    updateSetting,
    type Settings as SettingsType,
  } from "../stores/settings.js";
  import {
    getUsageProviderTitle,
    USAGE_PROVIDER_ORDER,
  } from "../providerMetadata.js";
  import type { HeaderTabConfig, UsageProvider } from "../types/index.js";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  let current = $derived($settings as SettingsType);

  const HEADER_TAB_FIELDS: Array<{ provider: UsageProvider; title: string }> = USAGE_PROVIDER_ORDER.map(
    (provider) => ({
      provider,
      title: getUsageProviderTitle(provider),
    }),
  );

  function createHeaderLabelInputs(nextHeaderTabs: SettingsType["headerTabs"]): Record<string, string> {
    const inputs: Record<string, string> = {};
    for (const provider of USAGE_PROVIDER_ORDER) {
      inputs[provider] = nextHeaderTabs[provider]?.label ?? DEFAULT_HEADER_TABS[provider]?.label ?? provider;
    }
    return inputs;
  }

  function syncHeaderLabelInputs(nextHeaderTabs: SettingsType["headerTabs"]) {
    headerLabelInputs = createHeaderLabelInputs(nextHeaderTabs);
  }

  let headerLabelInputs = $state<Record<string, string>>(createHeaderLabelInputs(DEFAULT_HEADER_TABS));

  let syncedHeaderTabs = $state(DEFAULT_HEADER_TABS);
  $effect(() => {
    const nextHeaderTabs = current.headerTabs;
    if (!areHeaderTabsEqual(syncedHeaderTabs, nextHeaderTabs)) {
      syncHeaderLabelInputs(nextHeaderTabs);
      syncedHeaderTabs = nextHeaderTabs;
    }
  });

  function updateHeaderTab(
    provider: UsageProvider,
    patch: Partial<HeaderTabConfig>,
  ) {
    updateSetting("headerTabs", {
      ...current.headerTabs,
      [provider]: {
        ...current.headerTabs[provider],
        ...patch,
      },
    });
  }

  function handleHeaderTabEnabled(provider: UsageProvider, enabled: boolean) {
    updateHeaderTab(provider, { enabled });
  }

  function handleHeaderLabelInput(provider: UsageProvider, value: string) {
    headerLabelInputs = {
      ...headerLabelInputs,
      [provider]: value,
    };
  }

  function persistHeaderLabel(provider: UsageProvider) {
    updateHeaderTab(provider, { label: headerLabelInputs[provider] });
  }

  function handleHeaderLabelKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      (e.target as HTMLInputElement).blur();
    }
  }
</script>

<div class="group">
  <div class="group-label">Header</div>
  <div class="card">
    {#each HEADER_TAB_FIELDS as tab, index}
      <div class="row header-tab-row" class:border={index < HEADER_TAB_FIELDS.length - 1}>
        <span class="header-source">{tab.title}</span>
        <input
          class="text-input"
          type="text"
          maxlength={MAX_HEADER_TAB_LABEL_LENGTH}
          value={headerLabelInputs[tab.provider]}
          oninput={(e) => handleHeaderLabelInput(tab.provider, (e.target as HTMLInputElement).value)}
          onblur={() => persistHeaderLabel(tab.provider)}
          onkeydown={handleHeaderLabelKeydown}
        />
        <ToggleSwitch
          checked={current.headerTabs[tab.provider].enabled}
          onChange={(checked) => handleHeaderTabEnabled(tab.provider, checked)}
        />
      </div>
    {/each}
  </div>
  <div class="setting-note">Labels are cosmetic. Usage data is still backed by the registered usage integrations.</div>
</div>

<style>
  .group {
    margin-bottom: 8px;
  }
  .group-label {
    font: 500 8px/1 'Inter', sans-serif;

    letter-spacing: 0.8px;
    color: var(--t4);
    padding: 2px 4px 4px;
  }
  .card {
    background: var(--surface-2);
    border-radius: 8px;
    overflow: hidden;
  }
  .row {
    padding: 7px 10px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .row.border {
    border-bottom: 1px solid var(--border-subtle);
  }
  .header-tab-row {
    gap: 8px;
  }
  .header-source {
    width: 46px;
    flex-shrink: 0;
    font: 500 9px/1 'Inter', sans-serif;
    color: var(--t2);
  }
  .text-input {
    min-width: 0;
    flex: 1;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--surface-hover);
    color: var(--t1);
    font: 400 9px/1 'Inter', sans-serif;
    padding: 5px 7px;
  }
  .text-input:focus {
    outline: none;
    border-color: var(--border);
  }
  .setting-note {
    font: 400 8px/1.35 'Inter', sans-serif;
    color: var(--t4);
    padding: 4px 4px 0;
  }
</style>
