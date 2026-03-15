# Provider Brand Theming & Chart Palette Refresh

**Date:** 2026-03-15
**Status:** Approved

## Overview

Two related UI improvements: (1) replace the current chart color palette with a cohesive warm/cool two-family system, and (2) add provider-specific brand theming that activates when the user toggles between Claude and Codex.

## 1. Chart Color Palette — "Warm & Cool"

Replace the current divergent model colors with two tonal families that share the same visual language.

### Color mapping

| Model   | CSS Variable | Old Value   | New Value   | Description       |
|---------|-------------|-------------|-------------|-------------------|
| Opus    | `--opus`    | `#7B8DEF`   | `#C4724C`   | Deep terracotta   |
| Sonnet  | `--sonnet`  | `#C9956B`   | `#D4956E`   | Warm peach        |
| Haiku   | `--haiku`   | `#5BA88C`   | `#E2B799`   | Sand              |
| GPT-5.4 | `--gpt54`   | `#B07CC6`   | `#3D5A80`   | Deep steel        |
| GPT-5.3 | `--gpt53`   | `#9B8EC4`   | `#6B8EAD`   | Steel blue        |
| GPT-5.2 | `--gpt52`   | `#8E7EB8`   | `#98B8D4`   | Light steel       |
| Codex   | `--codex`   | `#9B8EC4`   | `#6B8EAD`   | Steel blue        |

Soft variants for Claude models updated to match at 0.12 opacity:

| Variable | New Value |
|----------|-----------|
| `--opus-soft` | `rgba(196,114,76,0.12)` |
| `--sonnet-soft` | `rgba(212,149,110,0.12)` |
| `--haiku-soft` | `rgba(226,183,153,0.12)` |

No soft variants exist for OpenAI models in the current codebase (none are referenced by any component), so none are added.

**Note:** `--codex` and `--gpt53` intentionally share the same value (`#6B8EAD`). Codex is OpenAI's coding agent built on the GPT-5.3 family — they are the same product line. They never appear together in the same chart or model list.

### Design rationale

- Claude models use a warm gradient (dark to light terracotta). OpenAI models use a cool gradient (dark to light steel blue).
- Within each family, models are distinguishable by luminance. Between families, they contrast by hue.
- Feels cohesive in both dark and light mode without creating visual noise.

## 2. Provider Brand Theming

When the user toggles to a specific provider, the UI receives a subtle brand treatment. This is a "middle ground" approach: logo + accent color + barely-perceptible background tint, but metric boxes, text colors, and chart colors stay unchanged.

### Implementation: CSS variable layering

Add a `data-provider` attribute on `<html>` alongside the existing `data-theme` attribute. Provider themes are expressed as CSS variable overrides that layer on top of the light/dark base.

```
<html data-theme="dark" data-provider="claude">
```

When "All" is selected or brand theming is disabled, `data-provider` is removed entirely — the UI reverts to the neutral default.

### CSS selector strategy for all theme modes

The existing theme system has three modes: `data-theme="dark"`, `data-theme="light"`, and system (no `data-theme` attribute, uses `@media prefers-color-scheme`). Provider CSS must handle all three:

```css
/* Explicit dark + provider */
[data-theme="dark"][data-provider="claude"] { ... }

/* Explicit light + provider */
[data-theme="light"][data-provider="claude"] { ... }

/* System theme + provider: use @media queries */
@media (prefers-color-scheme: dark) {
  :root:not([data-theme])[data-provider="claude"] { ... }
}
@media (prefers-color-scheme: light) {
  :root:not([data-theme])[data-provider="claude"] { ... }
}
```

This yields 6 selectors per provider (3 theme modes × 2 light/dark variants), but the dark and light values are reused, so the CSS can be structured as shared blocks with selector lists.

### Claude theme

| Property | Dark | Light |
|----------|------|-------|
| `--bg` | `#13110F` | `#FAF6F2` |
| `--surface` | `#181412` | `#FFFFFF` |
| `--accent` | `#D4775C` | `#B45A32` |
| `--accent-soft` | `rgba(196,114,76,0.12)` | `rgba(180,90,50,0.10)` |
| `--border` | `rgba(196,114,76,0.08)` | `rgba(180,90,50,0.08)` |
| `--border-subtle` | `rgba(196,114,76,0.05)` | `rgba(180,90,50,0.05)` |
| Logo | Anthropic hexagon, stroke `#D4775C` | Anthropic hexagon, stroke `#B45A32` |

### Codex theme

| Property | Dark | Light |
|----------|------|-------|
| `--bg` | `#0E1015` | `#F2F5F7` |
| `--surface` | `#121418` | `#FFFFFF` |
| `--accent` | `#6B8EAD` | `#4A7094` |
| `--accent-soft` | `rgba(90,130,180,0.12)` | `rgba(90,130,180,0.10)` |
| `--border` | `rgba(90,130,180,0.08)` | `rgba(90,130,180,0.08)` |
| `--border-subtle` | `rgba(90,130,180,0.05)` | `rgba(90,130,180,0.05)` |
| Logo | OpenAI hexagon, stroke `#6B8EAD` | OpenAI hexagon, stroke `#4A7094` |

### What does NOT change per-provider

- Metric box backgrounds — stay `var(--surface-2)` (neutral)
- Text colors — `--t1` through `--t4` remain unchanged
- Chart model colors — stay consistent regardless of provider mode
- Chart grid lines, animations, and interaction behavior

### "All" mode behavior

Reverts to the default neutral theme. No `data-provider` attribute, no logo, no accent tint. This is the existing dark/light theme as-is.

## 3. Logo placement

A small provider logo appears in the header area above the Toggle component when a specific provider is selected and brand theming is enabled.

- Anthropic: simplified hexagon outline (stroke only, 14×14px container)
- OpenAI: simplified hexagon outline (stroke only, 14×14px container)
- Accompanied by a small provider name label
- Animated with existing `fadeUp` keyframes
- Hidden when provider is "All" or brand theming is disabled

## 4. Settings: Brand theming toggle

New boolean setting to disable provider theming.

```typescript
interface Settings {
  // ... existing fields
  brandTheming: boolean; // default: true
}
```

- Added as a row in the "General" group in Settings, labeled "Provider Theming"
- Uses existing `ToggleSwitch` component
- When disabled: `data-provider` attribute is never set, logo is hidden, UI stays neutral regardless of which provider tab is active
- Persisted to disk via Tauri store alongside other settings

## 5. Files to modify

| File | Changes |
|------|---------|
| `src/app.css` | Update model color variables; add provider CSS blocks per the selector strategy in Section 2 (6 selector groups per provider: explicit dark/light + system theme via `@media`) |
| `src/App.svelte` | Call new `applyProvider()` on toggle change; read `brandTheming` setting; pass provider state to Toggle |
| `src/lib/components/Toggle.svelte` | Accept two new props: `brandTheming: boolean` and `provider: "all" \| "claude" \| "codex"`. When `brandTheming` is true and `provider` is not `"all"`, render the inline SVG logo + label above the toggle buttons. Both props are passed down from `App.svelte`. |
| `src/lib/stores/settings.ts` | Add `brandTheming: boolean` to `Settings` interface and `DEFAULTS` |
| `src/lib/components/Settings.svelte` | Add "Provider Theming" toggle row in General group |

No changes needed to: `format.ts` (already uses CSS vars), `Chart.svelte`, `MetricsRow.svelte`, `UsageBars.svelte`, `ModelList.svelte`, `Footer.svelte`.

## 6. Code quality requirements

- Follow existing patterns: scoped CSS, CSS variables, Svelte 5 runes
- No new dependencies
- Provider theme CSS should be organized with clear section comments matching existing `app.css` style
- Logo SVGs should be inline in the component (no external assets)
- Theme transitions should be smooth — add `transition: background 0.25s ease, border-color 0.25s ease` to `.pop` and other elements that change with provider theming
- Clean separation: provider theming logic lives in `applyProvider()` alongside `applyTheme()` in the settings store
- Note: the runtime `provider` state in `App.svelte` is typed as `"all" | "claude" | "codex"`, while `defaultProvider` in `Settings` is `"claude" | "codex"`. These are different — the `data-provider` attribute only uses the runtime value and is removed for `"all"`
