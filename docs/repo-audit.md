# Repo Audit Report

**Project:** TokenMonitor
**Date:** 2026-04-26
**Audited by:** repo-audit skill
**Scope:** 142/142 source files analyzed (100%)

---

## Executive Summary

TokenMonitor is a well-architected cross-platform Tauri v2 + Svelte 5 + Rust system tray app with clear module separation and good test coverage for business logic. The primary structural concern is the monolithic `usage/parser.rs` (5,578 lines) which handles three provider parsers in a single file. Configuration health has two urgent issues: a stale `package-lock.json` (version 0.10.6 vs 0.11.1) and `tmp/` development artifacts tracked in git. Code quality is generally high with no TODO/FIXME debt, but 328 `unwrap()` calls in Rust and 15 silent `.catch(() => {})` blocks in the frontend represent latent error-handling risk.

---

## Project Understanding

### Overview

TokenMonitor is a local-first system tray application that monitors Claude Code, Codex, and Cursor CLI token usage. It reads JSONL session logs from disk, applies pricing rules in Rust, and presents spend/rate-limit data through a native system tray popover. No API keys required for core usage tracking, no cloud sync. Supports macOS, Windows, and Linux with platform-specific features (menu bar cost display on macOS, taskbar panel on Windows, system tray on Linux).

### File Index

```
TokenMonitor/
├── src/                          — Svelte 5 frontend (TypeScript)
│   ├── App.svelte                — Main window entry: routing, data loading, resize orchestration
│   ├── main.ts                   — Svelte mount point for main window
│   ├── float-ball.ts             — Svelte mount point for always-on-top overlay
│   ├── lib/
│   │   ├── bootstrap.ts          — Startup wiring: settings → stores → native IPC
│   │   ├── providerMetadata.ts   — Single source of truth for provider UI metadata
│   │   ├── resizeOrchestrator.ts — Window resize state machine
│   │   ├── uiStability.ts        — UI stability debug tooling
│   │   ├── windowSizing.ts       — Window height calculation
│   │   ├── components/           — 28 Svelte components (UI layer)
│   │   │   ├── FloatBall.svelte  — Always-on-top draggable overlay (985 lines)
│   │   │   ├── Chart.svelte      — Usage chart with buckets (880 lines)
│   │   │   ├── Settings.svelte   — Settings panel (665 lines)
│   │   │   ├── Calendar.svelte   — Heatmap calendar view
│   │   │   ├── DevicesView.svelte — SSH device list
│   │   │   └── ... (23 more)
│   │   ├── stores/               — Svelte stores (state management)
│   │   │   ├── usage.ts          — Usage data fetching & caching
│   │   │   ├── settings.ts       — Persisted settings (tauri-plugin-store)
│   │   │   ├── rateLimits.ts     — Rate limit polling & caching
│   │   │   └── updater.ts        — Auto-update state machine
│   │   ├── views/                — Business logic (non-UI)
│   │   │   ├── rateLimits.ts     — Rate limit window computation
│   │   │   ├── rateLimitMonitor.ts — Rate limit display logic
│   │   │   ├── footer.ts         — Footer cost/time formatting
│   │   │   └── deviceStats.ts    — Device statistics helpers
│   │   ├── tray/                 — Tray sync logic
│   │   │   ├── sync.ts           — IPC to update tray config
│   │   │   └── title.ts          — Tray title formatting
│   │   ├── window/               — Native window management
│   │   │   └── appearance.ts     — Theme, glass effect, surface sync
│   │   ├── permissions/          — Permission disclosure UI logic
│   │   │   ├── keychain.ts       — macOS Keychain access
│   │   │   └── surfaces.ts       — Permission surface definitions
│   │   ├── types/index.ts        — Shared TypeScript interfaces
│   │   └── utils/                — Pure utility functions
│   │       ├── format.ts         — Number/cost/time formatting
│   │       ├── calendar.ts       — Calendar date math
│   │       ├── plans.ts          — Plan tier cost logic
│   │       ├── platform.ts       — macOS/Windows/Linux detection
│   │       └── logger.ts         — Frontend → Rust log bridge
│   └── *.css                     — Global styles
├── src-tauri/                    — Rust backend
│   ├── src/
│   │   ├── lib.rs                — App setup, tray icon, background refresh loop
│   │   ├── main.rs               — Tauri entry point (6 lines)
│   │   ├── commands.rs           — IPC dispatch hub + AppState definition
│   │   ├── commands/             — Domain-specific IPC handlers
│   │   │   ├── usage_query.rs    — Usage data fetching (1,517 lines)
│   │   │   ├── calendar.rs       — Calendar heatmap queries
│   │   │   ├── config.rs         — Settings sync IPC
│   │   │   ├── tray.rs           — Tray title/utilization rendering
│   │   │   ├── ssh.rs            — Remote device management
│   │   │   ├── float_ball/       — Float ball state + layout (1,786 lines)
│   │   │   ├── updater.rs        — Update check/install commands
│   │   │   ├── period.rs         — Time range selection
│   │   │   └── logging.rs        — Log level control
│   │   ├── models.rs             — Shared serde payload types (1,003 lines)
│   │   ├── logging.rs            — tracing + rolling file appender
│   │   ├── paths.rs              — Platform app data path discovery
│   │   ├── usage/                — Core usage processing
│   │   │   ├── parser.rs         — Main parser engine (5,578 lines, hub file)
│   │   │   ├── claude_parser.rs  — Claude JSONL-specific parsing
│   │   │   ├── pricing.rs        — Model pricing rules
│   │   │   ├── integrations.rs   — Provider integration registry
│   │   │   ├── device_aggregation.rs — Multi-device aggregation (1,071 lines)
│   │   │   ├── ssh_remote.rs     — SSH sync + cache management
│   │   │   ├── ssh_config.rs     — ~/.ssh/config parser
│   │   │   ├── archive.rs        — Hourly aggregate archival
│   │   │   ├── litellm.rs        — Dynamic pricing from LiteLLM
│   │   │   ├── openrouter.rs     — OpenRouter pricing fallback
│   │   │   └── exchange_rates.rs — USD → foreign currency rates
│   │   ├── rate_limits/          — Rate limit fetching per provider
│   │   │   ├── mod.rs            — Orchestrator + merge logic
│   │   │   ├── claude.rs         — OAuth API (macOS) + CLI fallback
│   │   │   ├── claude_cli.rs     — CLI probe (all platforms)
│   │   │   ├── codex.rs          — Codex session file parsing
│   │   │   ├── cursor.rs         — Cursor API rate limits
│   │   │   └── http.rs           — Shared HTTP error utilities
│   │   ├── tray/render.rs        — RGBA pixel buffer generation
│   │   ├── stats/                — Usage statistics
│   │   │   ├── change.rs         — Period-over-period change calc
│   │   │   └── subagent.rs       — Subagent scope detection
│   │   ├── platform/             — OS-specific code
│   │   │   ├── mod.rs            — Cross-platform window helpers
│   │   │   ├── macos/mod.rs      — macOS dock icon
│   │   │   ├── windows/          — Taskbar panel + window positioning
│   │   │   └── linux/mod.rs      — X11 window positioning
│   │   ├── secrets/              — Credential storage
│   │   │   ├── mod.rs            — Keyring abstraction
│   │   │   └── cursor.rs         — Cursor API key storage
│   │   └── updater/              — Auto-update system
│   │       ├── state.rs          — Update state machine
│   │       ├── scheduler.rs      — Background check scheduler
│   │       └── persistence.rs    — State persistence to disk
│   ├── Cargo.toml                — Rust dependencies
│   └── tauri.conf.json           — Tauri app configuration
├── build/                        — Custom multi-platform build orchestration
├── scripts/release.sh            — Version bump + tag + push
├── docs/                         — Documentation, debug notes, ECL configs
├── archive/                      — Legacy code (ccusage.rs)
├── tmp/                          — Dev artifacts (should not be tracked)
├── .github/workflows/            — CI (3-OS matrix) + Release (signed builds)
├── package.json                  — v0.11.1, Tauri v2 + Svelte 5
└── vite.config.ts                — Multi-entry Vite (main + float-ball)
```

**Speed Reference — "I want to... → look at":**

| I want to... | Go to |
|--------------|-------|
| Understand the project | CLAUDE.md, README.md |
| Modify core usage parsing | src-tauri/src/usage/parser.rs, claude_parser.rs |
| Add a new CLI provider | src-tauri/src/usage/integrations.rs, then parser.rs |
| Change pricing rules | src-tauri/src/usage/pricing.rs (bump PRICING_VERSION) |
| Add a new rate limit source | src-tauri/src/rate_limits/ + providerMetadata.ts |
| Modify the main UI | src/App.svelte + relevant component in src/lib/components/ |
| Change settings | src/lib/stores/settings.ts + Settings.svelte |
| Modify tray icon rendering | src-tauri/src/tray/render.rs |
| Add platform-specific behavior | src-tauri/src/platform/{macos,windows,linux}/ |
| Run tests | `npm test` (frontend), `npm run test:rust` (backend) |
| Cut a release | `npm run release -- X.Y.Z` |
| Debug window sizing | src/lib/resizeOrchestrator.ts, uiStability.ts |

### Tech Stack

- **Frontend:** Svelte 5 (runes mode) + TypeScript 5.7 + Vite 6
- **Backend:** Rust 2021 edition + Tauri 2.x
- **State:** Svelte stores (frontend) + Arc<RwLock<>> (Rust), tauri-plugin-store for persistence
- **Testing:** Vitest 4.1 (frontend, 24 test files), cargo test (Rust, inline #[cfg(test)] modules)
- **CI/CD:** GitHub Actions (3-OS CI matrix, tag-triggered signed releases)
- **Platform deps:** objc2 + security-framework (macOS), windows crate (Windows), gtk/webkit2gtk (Linux)

### Architecture

**Data flow:** Local JSONL files → Rust `usage/parser` (file-change detection + 2-min TTL cache) → `usage/pricing` (cost calculation) → Tauri IPC → Svelte stores → UI components. Background loop (30s cycle) refreshes tray and emits `data-updated` events.

**Rate limits:** Split by provider: `claude.rs` (OAuth + CLI), `codex.rs` (session files), `cursor.rs` (API). Orchestrated by `rate_limits/mod.rs` with merge logic. Cached in `AppState.cached_rate_limits`.

**Key design patterns:**
- Provider registry (usage/integrations.rs) for extensibility
- Stale-while-revalidate caching in frontend stores
- Optimistic UI updates with rollback (device stats toggle)
- Platform abstraction via `#[cfg(target_os)]` and frontend `platform.ts`

### Implementation Rationale

- **Local-first / no API keys:** Reads existing JSONL logs that Claude/Codex/Cursor already write. Zero setup friction.
- **Rust pricing engine:** Keeps cost calculation deterministic and fast; avoids floating-point inconsistencies across JS runtimes.
- **Tray popover (not a regular window):** Mimics native macOS menu bar apps. Always-on-top + hide-on-blur for quick glance.
- **Multi-entry Vite:** Separate entry for FloatBall so it can be an independent always-on-top window.
- **Dynamic pricing (LiteLLM + OpenRouter):** Fetches model pricing from external sources with 7-day cache TTL, so new models get correct pricing without app updates.

### Usage Guide

**Install:** Download DMG (macOS), NSIS installer (Windows), or .deb (Linux) from GitHub Releases.

**Run:** App lives in the system tray. Left-click to toggle the popover. Right-click for Show/Quit menu.

**Configure:** Click gear icon in footer → Settings panel. Key settings:
- Provider tabs (All/Claude/Codex/Cursor)
- Time period (5h/Day/Week/Month)
- Rate limits toggle
- SSH remote devices
- Float ball overlay
- Theme (system/light/dark)

**CLI release:** `npm run release -- X.Y.Z` bumps version across 3 files, commits, tags, and pushes.

### Key Files

| File | Function | Why Important | Dependencies |
|------|----------|---------------|-------------|
| src-tauri/src/usage/parser.rs | Main parser engine | All usage data flows through here | imports: pricing, integrations, claude_parser; imported by: usage_query, calendar, device_aggregation, ssh, archive |
| src-tauri/src/lib.rs | App setup + background loop | Entry point for Rust; wires tray, plugins, background refresh | imports: commands, logging, rate_limits, usage, platform, updater |
| src-tauri/src/commands.rs | IPC dispatch hub + AppState | Central state container; all IPC handlers branch from here | imports: models, usage::parser; imported by: all command submodules |
| src-tauri/src/models.rs | Shared payload types | Defines all IPC data shapes (UsagePayload, RateLimitsPayload, etc.) | imported by: nearly all Rust modules |
| src-tauri/src/usage/pricing.rs | Model cost calculation | Translates token counts → dollar costs | imports: models, litellm; imported by: parser, device_aggregation |
| src-tauri/src/rate_limits/mod.rs | Rate limit orchestrator | Dispatches to per-provider fetchers, merges results | imports: claude, claude_cli, codex, cursor, http |
| src/App.svelte | Main window UI | Routing, data loading, resize orchestration, all user interactions | imports: 30+ modules (stores, components, views, utils) |
| src/lib/stores/usage.ts | Usage data store | Caching, stale-while-revalidate, IPC bridge for usage data | imported by: App, Chart, Settings |
| src/lib/stores/settings.ts | Settings persistence | All app settings, persisted via tauri-plugin-store | imported by: 12+ components |
| src/lib/providerMetadata.ts | Provider definitions | Single source for provider labels, colors, rate limit config, plan tiers | imported by: 6+ files |
| src/lib/bootstrap.ts | Startup orchestration | Wires settings → stores → native IPC on app launch | imported by: App.svelte |
| src/lib/resizeOrchestrator.ts | Window resize state machine | Handles popover height adjustment with throttling and scroll lock | imported by: App.svelte |
| .github/workflows/release.yml | Release pipeline | Signed builds for 3 platforms, updater artifact signing | triggered by: git tags |
| scripts/release.sh | Version bump script | Ensures version sync across 3 manifest files | invoked by: npm run release |

---

## Findings

### P1: Project Structure

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-001 | high | src-tauri/src/usage/parser.rs | Monolithic parser: 5,578 lines handling Claude + Codex + Cursor parsing in a single file. This is the largest file in the codebase by 3.7x. | Split into provider-specific parser files (cursor_parser.rs, codex_parser.rs) similar to the existing claude_parser.rs pattern. | 0.90 |
| F-002 | medium | src-tauri/src/commands/float_ball/ | Float ball module totals 1,786 lines across mod.rs (891) + layout.rs (895). Layout calculations are tightly coupled with IPC commands. | Consider extracting pure geometry/layout math into a standalone module testable without Tauri. | 0.80 |
| F-003 | medium | src/lib/components/FloatBall.svelte | Largest Svelte component at 985 lines, mixing drag interaction, layout, animation, and IPC. | Decompose into sub-components (FloatBallDrag, FloatBallExpanded, FloatBallLayout). | 0.75 |
| F-004 | low | src/App.svelte | 963 lines with significant business logic (device toggle, keychain flow) mixed with view routing. | Extract handler functions into a dedicated module (e.g., lib/appHandlers.ts). | 0.70 |
| F-005 | medium | tmp/ | 4 development artifacts (bedrock_usage.py, compare_usage.py, etc.) tracked in git. These are not part of the build. | Add `tmp/` to .gitignore, remove from tracking with `git rm --cached tmp/`. | 0.95 |
| F-006 | low | archive/ccusage.rs | 32KB legacy file tracked in git. Not referenced by any build or source file. | Document purpose or move to a separate branch; add `archive/` to .gitignore if purely historical. | 0.80 |

### P2: Logic

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-007 | medium | src-tauri/src/ (328 calls) | 328 `unwrap()` calls across the Rust codebase. While many are safe in context, several are in I/O paths (file reads, SSH operations, JSON parsing) where panics would crash the app. | Audit unwrap() calls in I/O-facing code; replace with `?` or `.unwrap_or_default()` where a panic would crash the tray app. | 0.75 |
| F-008 | medium | src-tauri/src/updater/ | 7 `#[allow(dead_code)]` annotations in scheduler.rs + 4 in state.rs suggest incomplete updater implementation. | Audit dead code: remove if truly unused, or document if planned for future use. | 0.70 |
| F-009 | low | src/ (15 instances) | 15 silent `.catch(() => {})` blocks swallow errors without logging. While intentional for non-blocking UI, they can hide bugs during development. | Replace with `.catch((e) => logger.debug("context", e))` to preserve non-blocking behavior while enabling debug visibility. | 0.80 |
| F-010 | low | src-tauri/src/usage/parser.rs | 8 `#[allow(dead_code)]` annotations in parser.rs, mostly on test helper structs and cache fields. | Clean up: remove annotations if items are used in tests; delete truly dead items. | 0.75 |
| F-011 | info | src-tauri/src/lib.rs:439 | `background_loop()` at 126 lines orchestrates 5 concerns: rate limits, pricing, exchange rates, SSH sync, and archival. | Consider splitting into focused async tasks (one per concern) coordinated by the main loop. | 0.70 |

### P3: Duplication

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-012 | medium | src-tauri/src/rate_limits/ | claude.rs, cursor.rs, and codex.rs each independently implement HTTP fetch → parse → convert-to-RateLimitWindow patterns with similar error handling. | Extract a shared trait or helper for rate-limit-provider fetch/parse, keeping only provider-specific logic in each file. | 0.75 |
| F-013 | low | src-tauri/src/usage/litellm.rs + openrouter.rs | Both fetch external pricing data, parse model entries, and cache with TTL checks. Similar caching pattern also in exchange_rates.rs. | Consolidate the fetch-parse-cache pattern into a generic cached-remote-resource helper. | 0.70 |
| F-014 | low | src/lib/components/Settings.svelte | 8+ similar toggle handler functions following try/invoke/updateSetting/catch pattern. | Extract a generic `handleSettingsToggle(command, settingKey, value)` helper. | 0.75 |
| F-015 | low | src-tauri/src/usage/parser.rs:5320-5492 | 4 nearly identical Cursor API POST requests to different endpoints (cursor.com, api.cursor.com, api2.cursor.sh, api3.cursor.sh). | Refactor into an endpoint-list iteration with a shared request builder. | 0.80 |

### P4: Configuration

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-016 | high | package-lock.json | Lockfile version is 0.10.6 but package.json is 0.11.1. `npm ci` in CI will install stale dependencies. | Run `npm install` to regenerate package-lock.json, commit the updated file. | 0.95 |
| F-017 | low | tsconfig.json | `noUnusedLocals: false` and `noUnusedParameters: false` — permissive settings that allow dead code to accumulate in TypeScript. | Set both to `true` and clean up any resulting errors. | 0.80 |

### P5: Dead Code

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-018 | medium | src-tauri/src/ (33 annotations) | 33 `#[allow(dead_code)]` annotations across the Rust codebase. Heaviest in updater/scheduler.rs (7), usage/parser.rs (8), and updater/state.rs (4). | Audit each annotation: remove the annotation if the item is actually used (e.g., in tests); delete the item if truly dead. | 0.80 |
| F-019 | low | src-tauri/src/usage/mod.rs:1 | Module-level `#[allow(dead_code)]` blanket-suppresses all warnings for the entire usage module tree. | Remove the blanket annotation; add targeted `#[allow(dead_code)]` only where truly needed. | 0.85 |

### P6: Hardcoded Values

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-020 | medium | src-tauri/src/ (13+ URLs) | 13+ hardcoded API URLs scattered across 7 files (anthropic.com, cursor.com, openrouter.ai, frankfurter.dev, github.com/BerriAI). | Centralize external URLs into a `config.rs` or constants module for easier updates when APIs change. | 0.80 |
| F-021 | low | src-tauri/src/lib.rs | Background loop cycle constants (SSH_SYNC_EVERY_N_CYCLES=10, RATE_LIMIT_REFRESH_EVERY_N_CYCLES=5, PRICING_CHECK_EVERY_N_CYCLES=120) are local `const` in the function body. | Move to a central configuration module or make configurable via settings. | 0.70 |
| F-022 | low | src/lib/components/SplashScreen.svelte | Splash screen minimum display time hardcoded to 2900ms. | Consider making this configurable or at least extracting to a named constant. | 0.60 |

### P7: Dependency Health

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-023 | info | package.json | All dependencies use caret ranges (^2, ^5, ^6) which is appropriate for a Tauri app. No obviously outdated or deprecated packages. | No action needed. Dependencies are healthy. | 0.90 |
| F-024 | info | src-tauri/Cargo.toml | Rust dependencies are current. Platform-specific deps (keyring, objc2, windows) use appropriate feature flags. | No action needed. Well-maintained. | 0.90 |

---

## Dependency Map

### Hub Files (imported by >30% of source files)

| File | Role | Imported By |
|------|------|-------------|
| src-tauri/src/usage/parser.rs | Central parser engine | usage_query, calendar, device_aggregation, ssh, archive, lib.rs |
| src-tauri/src/models.rs | Shared payload types | Nearly all Rust modules |
| src-tauri/src/commands.rs | AppState + IPC hub | All command submodules, lib.rs |
| src/lib/types/index.ts | Shared TS interfaces | 12+ frontend files |
| src/lib/stores/settings.ts | Settings store | 12+ components |
| src/lib/providerMetadata.ts | Provider definitions | 6+ files |
| src/lib/utils/format.ts | Formatting utils | 8+ files |

### Orphan Files

| File | Last Modified | Notes |
|------|--------------|-------|
| tmp/bedrock_usage.py | tracked | Dev artifact, not imported anywhere |
| tmp/bedrock_usage_report.json | tracked | Dev artifact, not imported anywhere |
| tmp/compare_usage.py | tracked | Dev artifact, not imported anywhere |
| tmp/usage_comparison.json | tracked | Dev artifact, not imported anywhere |
| archive/ccusage.rs | tracked | Legacy code, not part of build |

### Circular Dependencies

None detected in either frontend or backend. Import graphs are strictly acyclic.

---

## Unresolved Questions

All findings have confidence >= 0.7. No questions require user confirmation.

Note: F-022 (splash screen timing) has confidence 0.60 but is severity "low" and purely cosmetic — classified as informational rather than generating a blocking question.

---

## Test Coverage

### Frontend

| Category | Files With Tests | Files Without Tests | Coverage |
|----------|-----------------|--------------------|---------| 
| Stores (4) | 4/4 | 0 | 100% |
| Views (4) | 4/4 | 0 | 100% |
| Utils (5) | 3/5 | logger.ts, platform.ts | 60% |
| Components (28) | 1/28 | 27 (including FloatBall, Chart, Settings) | 3.6% |
| Core modules (6) | 5/6 | providerMetadata.ts | 83% |
| Tray (2) | 2/2 | 0 | 100% |
| Permissions (2) | 2/2 | 0 | 100% |
| Window (1) | 1/1 | 0 | 100% |

### Backend (Rust)

Rust tests live in `#[cfg(test)]` modules within source files. Coverage not measured by line count but tests exist in parser.rs, pricing.rs, claude_parser.rs, archive.rs, device_aggregation.rs, and other core modules.

---

## Statistics

- **Total findings:** 24
- **By severity:** 2 high, 8 medium, 10 low, 4 info
- **By priority:** P1: 6, P2: 5, P3: 4, P4: 2, P5: 2, P6: 3, P7: 2
- **Coverage:** 142/142 files (100%)
- **Actionable by repo-tidy:** 3 (F-005, F-006, F-019)
- **Actionable by optimize:** 10 (F-001, F-003, F-004, F-007, F-009, F-012, F-013, F-014, F-015, F-021)
