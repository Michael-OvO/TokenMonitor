# UI Theming & Chart Palette Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the chart color palette with a Warm & Cool scheme (terracotta for Claude, steel blue for OpenAI), and add live provider brand theming that subtly tints the UI when toggled to Claude or Codex mode.

**Architecture:** CSS-variable layering via a `data-provider` attribute on `<html>`, alongside the existing `data-theme`. Provider themes override accent colors and add subtle background tints. A new `brandTheming` boolean in Settings controls whether theming is active. The Toggle component gains a small provider logo in the header area above it.

**Tech Stack:** Svelte 5 (runes), CSS custom properties, Tauri plugin-store for persistence, inline SVG logos.

---

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `src/app.css` | New Warm & Cool model colors; provider theme CSS variable overrides |
| Modify | `src/lib/stores/settings.ts` | Add `brandTheming: boolean` to Settings interface and defaults |
| Modify | `src/App.svelte` | Apply/remove `data-provider` attribute; pass provider to logo area |
| Modify | `src/lib/components/Toggle.svelte` | Show small provider SVG logo above toggle when in Claude/Codex mode |
| Modify | `src/lib/components/Settings.svelte` | Add "Brand Theming" toggle in General section |
| Modify | `src/lib/components/TimeTabs.svelte` | Use `var(--accent)` for active tab underline |
| Modify | `src/lib/components/Footer.svelte` | Use `var(--accent)` for active dot |

## Chunk 1: Chart Palette & Provider CSS Variables

### Task 1: Update model colors to Warm & Cool palette

**Files:**
- Modify: `src/app.css:4-15`

- [ ] **Step 1: Replace model color variables**

In `src/app.css`, replace the existing model color block (lines 4-15) with the new Warm & Cool palette:

```css
/* ── Model colors — Warm & Cool palette ── */
:root {
  --opus: #C4704B;
  --opus-soft: rgba(196,112,75,0.12);
  --sonnet: #D4956A;
  --sonnet-soft: rgba(212,149,106,0.12);
  --haiku: #E2A987;
  --haiku-soft: rgba(226,169,135,0.12);
  --gpt54: #4A7B9D;
  --gpt53: #6B9DBF;
  --gpt52: #8BB8D4;
  --codex: #6B9DBF;
}
```

**Palette rationale:**
- Claude models use a terracotta family (warm): deep → medium → light
- OpenAI models use a steel blue family (cool): deep → medium → light
- Each model within a family is clearly distinguishable (40+ hue/lightness delta)
- Both families work well on dark and light backgrounds

- [ ] **Step 2: Verify the app builds**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npm run build`
Expected: Build succeeds. All existing `var(--opus)`, `var(--sonnet)`, etc. references resolve correctly since variable names are unchanged.

- [ ] **Step 3: Commit**

```bash
git add src/app.css
git commit -m "style: update chart palette to Warm & Cool scheme

Terracotta family for Claude models, steel blue family for OpenAI models.
Improves visual harmony and reduces distraction from too-divergent colors."
```

### Task 2: Add provider theme CSS variables

**Files:**
- Modify: `src/app.css` (append after light theme block, before `@media` system theme block)

- [ ] **Step 1: Add accent variable defaults and provider theme blocks**

Insert the following CSS after the `[data-theme="light"]` block (after line 46) and before the `@media (prefers-color-scheme: light)` block:

```css
/* ── Accent defaults (neutral — no provider selected) ── */
:root {
  --accent: var(--t2);
  --accent-soft: rgba(255,255,255,0.06);
  --provider-bg: transparent;
}
[data-theme="light"] {
  --accent-soft: rgba(0,0,0,0.04);
}

/* ── Provider: Claude (warm terracotta) ── */
[data-provider="claude"] {
  --accent: #C4704B;
  --accent-soft: rgba(196,112,75,0.08);
  --provider-bg: rgba(196,112,75,0.03);
}
[data-theme="light"][data-provider="claude"] {
  --accent-soft: rgba(196,112,75,0.10);
  --provider-bg: rgba(196,112,75,0.025);
}

/* ── Provider: Codex (cool blue) ── */
[data-provider="codex"] {
  --accent: #4A7B9D;
  --accent-soft: rgba(74,123,157,0.08);
  --provider-bg: rgba(74,123,157,0.03);
}
[data-theme="light"][data-provider="codex"] {
  --accent-soft: rgba(74,123,157,0.10);
  --provider-bg: rgba(74,123,157,0.025);
}
```

Also add the same provider overrides for the system-theme media query. Inside `@media (prefers-color-scheme: light)`, after the existing `:root:not([data-theme])` block, add:

```css
  :root:not([data-theme]) {
    --accent-soft: rgba(0,0,0,0.04);
  }
  :root:not([data-theme])[data-provider="claude"] {
    --accent-soft: rgba(196,112,75,0.10);
    --provider-bg: rgba(196,112,75,0.025);
  }
  :root:not([data-theme])[data-provider="codex"] {
    --accent-soft: rgba(74,123,157,0.10);
    --provider-bg: rgba(74,123,157,0.025);
  }
```

- [ ] **Step 2: Apply `--provider-bg` to the `.pop` container**

In `src/App.svelte`, update the `.pop` CSS rule (line 192-205) to layer the provider tint:

```css
  .pop {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 14px;
    width: 340px;
    min-height: 200px;
    overflow: hidden;
    box-shadow: none;
    animation: popIn .32s cubic-bezier(.25,.8,.25,1) both;
    /* Force GPU compositing layer — prevents macOS transparent window
       compositor from retaining stale pixels during resize */
    isolation: isolate;
    -webkit-backface-visibility: hidden;
    /* Provider theme tint — transparent when neutral */
    background-image: linear-gradient(var(--provider-bg), var(--provider-bg));
  }
```

- [ ] **Step 3: Use `--accent` in TimeTabs active underline**

In `src/lib/components/TimeTabs.svelte`, change line 42:
```css
    /* before */ height: 1.5px; background: var(--t2); border-radius: .5px;
    /* after  */ height: 1.5px; background: var(--accent); border-radius: .5px;
```

- [ ] **Step 4: Use `--accent` in Footer active dot**

In `src/lib/components/Footer.svelte`, change line 57:
```css
    /* before */ .dot { width: 5px; height: 5px; border-radius: 50%; background: var(--haiku); ...
    /* after  */ .dot { width: 5px; height: 5px; border-radius: 50%; background: var(--accent); ...
```

- [ ] **Step 5: Use `--accent-soft` in Toggle slider (with neutral fallback)**

In `src/lib/components/Toggle.svelte`, change line 40 (the `.sl` background):
```css
    /* before */ background: rgba(255,255,255,0.07);
    /* after  */ background: var(--accent-soft, rgba(255,255,255,0.07));
```

This ensures the neutral (no-provider) state still has a visible slider on both light and dark backgrounds. The CSS fallback only applies when `--accent-soft` is not set or invalid.

- [ ] **Step 6: Verify build**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npm run build`
Expected: Build succeeds.

- [ ] **Step 7: Commit**

```bash
git add src/app.css src/App.svelte src/lib/components/TimeTabs.svelte src/lib/components/Footer.svelte src/lib/components/Toggle.svelte
git commit -m "style: add provider brand theme CSS variables

Adds --accent, --accent-soft, and --provider-bg variables with
overrides for claude (warm terracotta) and codex (cool blue).
Applied to toggle slider, active tab underline, footer dot, and
pop container background."
```

## Chunk 2: Settings Store, Provider Attribute, Logo, and Settings UI

### Task 3: Add `brandTheming` to settings store

**Files:**
- Modify: `src/lib/stores/settings.ts:5-25`

- [ ] **Step 1: Add brandTheming to Settings interface and defaults**

In `src/lib/stores/settings.ts`, add `brandTheming: boolean` to the `Settings` interface (after `hiddenModels`) and set default to `true`:

```typescript
export interface Settings {
  theme: "light" | "dark" | "system";
  defaultProvider: "claude" | "codex";
  defaultPeriod: "5h" | "day" | "week" | "month";
  refreshInterval: number;
  costAlertThreshold: number;
  launchAtLogin: boolean;
  currency: string;
  hiddenModels: string[];
  brandTheming: boolean;
}

const DEFAULTS: Settings = {
  theme: "dark",
  defaultProvider: "claude",
  defaultPeriod: "day",
  refreshInterval: 30,
  costAlertThreshold: 0,
  launchAtLogin: false,
  currency: "USD",
  hiddenModels: [],
  brandTheming: true,
};
```

- [ ] **Step 2: Add `applyProvider` helper**

Add a new exported function below `applyTheme`:

```typescript
export function applyProvider(provider: "all" | "claude" | "codex", brandTheming: boolean) {
  const root = document.documentElement;
  if (!brandTheming || provider === "all") {
    root.removeAttribute("data-provider");
  } else {
    root.setAttribute("data-provider", provider);
  }
}
```

- [ ] **Step 3: Verify build**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npm run build`
Expected: Build succeeds.

- [ ] **Step 4: Commit**

```bash
git add src/lib/stores/settings.ts
git commit -m "feat: add brandTheming setting and applyProvider helper

New boolean setting (default true) controls whether provider-specific
CSS theming is applied. applyProvider sets/removes data-provider
attribute on the root element."
```

### Task 4: Wire provider attribute in App.svelte

**Files:**
- Modify: `src/App.svelte`

- [ ] **Step 1: Import applyProvider and subscribe to brandTheming**

Update the imports at the top of `src/App.svelte` (line 19):

```typescript
  import { loadSettings, settings, applyTheme, applyProvider } from "./lib/stores/settings.js";
```

Add a `brandTheming` state variable after the existing state declarations (around line 41):

```typescript
  let brandTheming = $state(true);
```

Update the store subscription `$effect` (line 44-49) to also track `brandTheming`:

```typescript
  $effect(() => {
    const unsub1 = usageData.subscribe((v) => (data = v));
    const unsub2 = setupStatus.subscribe((v) => (status = v));
    const unsub3 = isLoading.subscribe((v) => (loading = v));
    const unsub4 = settings.subscribe((s) => (brandTheming = s.brandTheming));
    return () => { unsub1(); unsub2(); unsub3(); unsub4(); };
  });
```

- [ ] **Step 2: Add reactive `$effect` to apply provider theme**

Add a single `$effect` that reacts to both `provider` and `brandTheming` changes (after the loading effect, around line 60). This is the **sole** call site for `applyProvider` — no explicit calls in handlers or `onMount`:

```typescript
  $effect(() => {
    applyProvider(provider, brandTheming);
  });
```

**Important:** Do NOT add `applyProvider` calls in `handleProviderChange` or `onMount`. The `$effect` auto-tracks both `provider` and `brandTheming` and fires whenever either changes, including on initial render. This avoids double-calls and init-time flashes.

- [ ] **Step 5: Verify build**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npm run build`
Expected: Build succeeds.

- [ ] **Step 6: Commit**

```bash
git add src/App.svelte
git commit -m "feat: wire provider theme attribute to toggle and settings

applyProvider is called on provider change, initial load, and when
brandTheming setting changes. data-provider attribute drives CSS
variable overrides for live re-theming."
```

### Task 5: Add provider logo to Toggle component

**Files:**
- Modify: `src/lib/components/Toggle.svelte`

- [ ] **Step 1: Add provider prop and logo SVGs**

Update the Toggle component script to accept a `brandTheming` prop and render logos:

```svelte
<script lang="ts">
  interface Props {
    active: "all" | "claude" | "codex";
    onChange: (provider: "all" | "claude" | "codex") => void;
    brandTheming?: boolean;
  }
  let { active, onChange, brandTheming = true }: Props = $props();

  const options: Array<{ value: "all" | "claude" | "codex"; label: string }> = [
    { value: "all", label: "All" },
    { value: "claude", label: "Claude" },
    { value: "codex", label: "Codex" },
  ];

  let activeIdx = $derived(options.findIndex((o) => o.value === active));
  let showLogo = $derived(brandTheming && active !== "all");
</script>
```

- [ ] **Step 2: Add logo markup above toggle**

Replace the template in Toggle.svelte with:

```svelte
<div class="tog-wrap">
  {#if showLogo}
    <div class="provider-logo" class:claude={active === "claude"} class:codex={active === "codex"}>
      {#if active === "claude"}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
          <path d="M16.98 8.38 12.95 20h-2.2L6.57 8.38h2.15l2.93 9.04h.06l2.92-9.04h2.35Z" fill="currentColor"/>
        </svg>
        <span>Claude</span>
      {:else if active === "codex"}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
          <path d="M12 2L3 7v10l9 5 9-5V7l-9-5zm0 2.18L18.36 7.5 12 10.82 5.64 7.5 12 4.18zM5 8.82l6 3.32V19l-6-3.33V8.82zm8 10.18v-6.86l6-3.32v7.18L13 19z" fill="currentColor"/>
        </svg>
        <span>Codex</span>
      {/if}
    </div>
  {/if}
  <div class="tog">
    <div class="sl" style="width: calc({100 / options.length}% - 2.5px); transform: translateX({activeIdx * 100}%)"></div>
    {#each options as opt}
      <button class:on={active === opt.value} onclick={() => onChange(opt.value)}>
        {opt.label}
      </button>
    {/each}
  </div>
</div>
```

- [ ] **Step 3: Add logo styles**

Append these styles in the `<style>` block:

```css
  .provider-logo {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 2px 6px;
    animation: fadeUp .2s ease both;
  }
  .provider-logo span {
    font: 600 11px/1 'Inter', sans-serif;
    letter-spacing: .2px;
  }
  .provider-logo.claude {
    color: var(--accent);
  }
  .provider-logo.codex {
    color: var(--accent);
  }
```

- [ ] **Step 4: Pass brandTheming prop from App.svelte**

In `src/App.svelte`, update the Toggle usage (around line 165):

```svelte
    <Toggle active={provider} onChange={handleProviderChange} {brandTheming} />
```

- [ ] **Step 5: Verify build**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npm run build`
Expected: Build succeeds.

- [ ] **Step 6: Commit**

```bash
git add src/lib/components/Toggle.svelte src/App.svelte
git commit -m "feat: add provider logo in header above toggle

Shows a small inline SVG logo + provider name above the toggle
when brand theming is enabled and a specific provider is selected.
Fades in/out with the provider switch."
```

### Task 6: Add Brand Theming toggle in Settings

**Files:**
- Modify: `src/lib/components/Settings.svelte`

- [ ] **Step 1: Add `brandTheming` to `current` state initializer**

In `src/lib/components/Settings.svelte`, add `brandTheming: true` to the `current` state initializer (around line 15-24). No import changes needed — the existing imports are sufficient. This step is required for TypeScript — the local state object must match the full `SettingsType` interface:

```typescript
  let current = $state<SettingsType>({
    theme: "dark",
    defaultProvider: "claude",
    defaultPeriod: "day",
    refreshInterval: 30,
    costAlertThreshold: 50,
    launchAtLogin: false,
    currency: "USD",
    hiddenModels: [],
    brandTheming: true,
  });
```

- [ ] **Step 2: Add handler function**

Add after `handleProvider` (around line 55):

```typescript
  function handleBrandTheming(checked: boolean) {
    updateSetting("brandTheming", checked);
  }
```

**Note:** No explicit `applyProvider` call needed here. The `$effect` in App.svelte reacts to the settings store change and applies automatically.

- [ ] **Step 3: Add toggle UI in the General section**

In the template, after the Refresh row (after line 194, before the closing `</div>` of the General card), add:

```svelte
        <div class="row">
          <span class="label">Brand Theming</span>
          <ToggleSwitch
            checked={current.brandTheming}
            onChange={handleBrandTheming}
          />
        </div>
```

And update the Refresh row to have a border (change line 182 from `<div class="row">` to `<div class="row border">`).

- [ ] **Step 4: Verify build**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npm run build`
Expected: Build succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/Settings.svelte
git commit -m "feat: add Brand Theming toggle in Settings

New toggle in General section allows disabling provider-specific
UI theming. When off, the interface stays neutral regardless of
which provider tab is active."
```

### Task 7: Final integration test

- [ ] **Step 1: Run full build**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npm run build`
Expected: Clean build, no errors.

- [ ] **Step 2: Run type check**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npx svelte-check`
Expected: No type errors.

- [ ] **Step 3: Manual test checklist**

Run: `cd /Users/michael/Documents/GitHub/TokenMonitor && npm run dev`

Verify:
1. Charts show terracotta (Claude) and steel blue (OpenAI) colors
2. Toggling to "Claude" → warm terracotta tint on UI, logo appears above toggle
3. Toggling to "Codex" → cool blue tint on UI, logo appears above toggle
4. Toggling to "All" → neutral theme, no logo
5. Light mode: all three states look good
6. Dark mode: all three states look good
7. Settings → Brand Theming off → toggling providers no longer tints UI or shows logo
8. Settings → Brand Theming on → theming resumes
9. Setting persists across app restart

- [ ] **Step 4: Final commit if any fixes needed**
