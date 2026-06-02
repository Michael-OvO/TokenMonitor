# Repo Audit Report

**Project:** TokenMonitor
**Date:** 2026-04-22
**Audited by:** repo-audit skill
**Scope:** 125/125 source files analyzed (100%)

---

## Executive Summary

TokenMonitor is a well-architected cross-platform Tauri v2 desktop app with strong test coverage (162+ frontend tests, 100+ Rust tests) and production-ready CI/CD. The primary concerns are: (1) a **critical dedup bug** causing 2.34x fee overestimation, (2) a monolithic parser file (4,198 lines), and (3) 32 `#[allow(dead_code)]` annotations suggesting incomplete cleanup. Overall health grade: **B+** -- solid fundamentals with targeted issues to address.

---

## Project Understanding

### Overview

A local-first system tray app that monitors Claude Code and Codex CLI token usage by reading JSONL session logs from disk, applying pricing rules in Rust, and presenting spend/rate-limit data through a native popover. Supported on macOS, Windows, and Linux. No API keys or cloud sync required.

### File Index

```
TokenMonitor/
├── src/                          — Svelte 5 frontend (WebView)
│   ├── App.svelte                — Root shell: layout orchestration + event routing
│   ├── main.ts                   — Svelte mount point
│   ├── float-ball.ts             — Separate entry for FloatBall overlay
│   └── lib/
│       ├── bootstrap.ts          — Runtime init: settings → stores → IPC wiring
│       ├── providerMetadata.ts   — Provider registry (labels, colors, plan tiers)
│       ├── resizeOrchestrator.ts — Window resize animation + scroll locking
│       ├── windowSizing.ts       — Height calc, monitor constraints
│       ├── uiStability.ts        — Debug stubs for resize tracing
│       ├── types/index.ts        — Shared TS interfaces (mirrors Rust structs)
│       ├── stores/
│       │   ├── usage.ts          — Usage data store + stale-while-revalidate cache
│       │   ├── settings.ts       — Settings persistence + migration
│       │   ├── rateLimits.ts     — Per-provider rate limit cache + retry
│       │   └── updater.ts        — Auto-updater state management
│       ├── components/           — 26 Svelte UI components
│       │   ├── Settings.svelte   — Settings panel (delegates to sub-components)
│       │   ├── Chart.svelte      — Stacked bar/line/pie chart (893 lines)
│       │   ├── FloatBall.svelte  — Always-on-top overlay (1,063 lines)
│       │   └── ...
│       ├── views/                — View-layer logic (no rendering)
│       │   ├── rateLimitMonitor.ts — Rate limit merge + peak stabilization
│       │   ├── rateLimits.ts     — Per-provider window filtering
│       │   ├── footer.ts         — Footer utilization extraction
│       │   └── deviceStats.ts    — Device inclusion flag mutations
│       ├── tray/
│       │   ├── sync.ts           — Tray config IPC sync
│       │   └── title.ts          — Tray title formatting
│       ├── window/
│       │   └── appearance.ts     — Native surface/theme/glass effects
│       └── utils/
│           ├── format.ts         — Cost/token/time formatting + model colors
│           ├── calendar.ts       — Heatmap intensity + colors
│           ├── plans.ts          — Plan tier cost lookup
│           ├── platform.ts       — OS detection (cached)
│           └── logger.ts         — Level-filtered logger → Rust IPC
├── src-tauri/                    — Rust backend
│   ├── src/
│   │   ├── main.rs              — Entry point (delegates to lib.rs)
│   │   ├── lib.rs               — App setup, tray, background loop (701 lines)
│   │   ├── models.rs            — Model family detection/normalization (1,003 lines)
│   │   ├── paths.rs             — Filesystem path registry
│   │   ├── logging.rs           — tracing + rolling file appender
│   │   ├── commands/            — IPC dispatch hub
│   │   │   ├── commands.rs      — AppState definition (12 Arc<RwLock<>> fields)
│   │   │   ├── usage_query.rs   — Usage data fetching + caching (1,407 lines)
│   │   │   ├── calendar.rs      — Heatmap queries (490 lines)
│   │   │   ├── config.rs        — Settings sync + window mgmt
│   │   │   ├── tray.rs          — Title/utilization rendering (528 lines)
│   │   │   ├── ssh.rs           — Remote device management (1,536 lines)
│   │   │   ├── float_ball.rs    — Overlay ball state (1,818 lines)
│   │   │   ├── updater.rs       — Update commands
│   │   │   └── logging.rs       — Log-level control
│   │   ├── usage/               — Core parsing + pricing
│   │   │   ├── parser.rs        — JSONL parser + file cache (4,198 lines)
│   │   │   ├── pricing.rs       — Static + dynamic pricing tables
│   │   │   ├── litellm.rs       — LiteLLM GitHub pricing fetch
│   │   │   ├── openrouter.rs    — OpenRouter API pricing fetch
│   │   │   ├── integrations.rs  — Provider registration
│   │   │   ├── archive.rs       — Hourly aggregate archival (data loss prevention)
│   │   │   ├── ccusage.rs       — Legacy parser fallback (994 lines)
│   │   │   ├── ssh_remote.rs    — SSH host sync + cache (726 lines)
│   │   │   └── ssh_config.rs    — ~/.ssh/config parser (491 lines)
│   │   ├── rate_limits/         — Provider rate limit fetching
│   │   │   ├── mod.rs           — Orchestration + merge strategy
│   │   │   ├── claude.rs        — OAuth Keychain + API (macOS)
│   │   │   ├── claude_cli.rs    — CLI probe fallback (all platforms)
│   │   │   ├── codex.rs         — Session file parsing
│   │   │   └── http.rs          — Shared HTTP client
│   │   ├── tray/render.rs       — RGBA pixel buffer rendering
│   │   ├── stats/               — Usage analytics
│   │   │   ├── subagent.rs      — Main/subagent breakdown
│   │   │   └── change.rs        — Code change tracking
│   │   ├── platform/            — OS-specific code
│   │   │   ├── mod.rs           — Cross-platform helpers
│   │   │   ├── macos/mod.rs     — Dock icon toggle
│   │   │   ├── windows/window.rs — Window positioning (Win32)
│   │   │   ├── windows/taskbar.rs — Taskbar panel embed (GDI)
│   │   │   └── linux/mod.rs     — X11/Wayland positioning
│   │   └── updater/             — Auto-update system
│   │       ├── state.rs         — State machine
│   │       ├── scheduler.rs     — Background check with backoff
│   │       └── persistence.rs   — Store save/load
│   └── tauri.conf.json          — Window, bundle, updater config
├── scripts/release.sh           — Version bump + tag push
├── build/                       — Installer build scripts
├── docs/                        — Design docs, ECL specs, debug logs
└── archive/                     — Past code (ccusage CLI, MCP modules)
```

**Navigation hints:**

| I want to... | Look at |
|--------------|---------|
| Understand the project | README.md, CLAUDE.md |
| Modify usage parsing/pricing | src-tauri/src/usage/parser.rs, pricing.rs |
| Add a new CLI provider | src-tauri/src/usage/integrations.rs + parser.rs |
| Change the UI layout/components | src/lib/components/, App.svelte |
| Modify rate limit behavior | src-tauri/src/rate_limits/, src/lib/stores/rateLimits.ts |
| Add a new setting | src/lib/stores/settings.ts + Settings.svelte + commands/config.rs |
| Run tests | `npm test` (frontend), `cd src-tauri && cargo test` (Rust) |
| Debug tray icon rendering | src-tauri/src/tray/render.rs |
| Fix platform-specific behavior | src-tauri/src/platform/{macos,windows,linux}/ |
| Release a new version | `npm run release -- X.Y.Z` |

### Tech Stack

| Layer | Technology | Version |
|-------|-----------|---------|
| Frontend | Svelte 5 + TypeScript 5.7 | ^5.0, ^5.7 |
| Backend | Rust (2021 edition) + Tauri v2 | v2.0 |
| Build | Vite 6, rollup multi-entry | ^6.0 |
| Testing | vitest 4.1 (frontend), cargo test (Rust) | ^4.1 |
| CI/CD | GitHub Actions (3-OS matrix) | ci.yml + release.yml |
| Packaging | DMG (macOS), NSIS (Windows), deb/AppImage (Linux) | tag-triggered |

### Architecture

**Data flow:** Local JSONL files -> Rust `usage/parser` + `usage/pricing` -> in-memory cache (`Arc<RwLock<>>`, 2-min TTL) -> Tauri IPC -> Svelte stores -> UI components. Background loop refreshes tray every 30s and emits `data-updated` events. Frontend also maintains a payload cache (5-min TTL) with stale-while-revalidate for tab switches.

**Key design patterns:**
- **Provider abstraction:** `providerMetadata.ts` (frontend) + `integrations.rs` (backend) define provider-agnostic interfaces. Adding a provider means registering metadata + parser, not modifying existing code.
- **Stale-while-revalidate:** Frontend `usage.ts` serves cached data immediately while fetching fresh data in background.
- **Platform dispatch:** `#[cfg(target_os)]` in Rust, `utils/platform.ts` in frontend. Platform-specific UX (glass blur, dock icon) hidden on non-supporting OSes.
- **IPC boundary:** Rust handles all file I/O, parsing, and pricing. Frontend is purely display logic + store management.

### Implementation Rationale

- **Native Rust parsing** (not shelling out to ccusage) for performance and no runtime dependency.
- **File-level caching with hash invalidation** in parser.rs avoids reparsing unchanged JSONL files.
- **Archive system** (usage/archive.rs) provides data loss prevention by persisting hourly aggregates.
- **Dynamic pricing** (litellm.rs + openrouter.rs) supplements static tables with auto-refreshing rates from public APIs (7-day TTL).
- **Separate Vite entry** for FloatBall enables independent lifecycle from main window.
- [Speculative] **X11 forced on Wayland** because Wayland compositors ignore client-side positioning, breaking the popover UX.

### Usage Guide

```bash
npm install                    # Install frontend deps
npm run tauri dev              # Full app (hot-reload frontend + debug Rust)
npm run dev                    # Frontend only at http://localhost:1420
npm test                       # Frontend tests (vitest)
cd src-tauri && cargo test     # Rust tests
npx svelte-check               # Type checking
npm run release -- X.Y.Z       # Bump version, tag, push (triggers release)
```

### Key Files

| File | Function | Why important | Dependencies |
|------|----------|---------------|-------------|
| src-tauri/src/usage/parser.rs | JSONL parsing + file cache | Core data pipeline | pricing.rs, integrations.rs |
| src-tauri/src/lib.rs | App setup, tray, background loop | Orchestrates everything | commands/, usage/, rate_limits/ |
| src-tauri/src/models.rs | Model normalization + family detection | All data goes through this | stats/ |
| src-tauri/src/commands.rs | AppState definition | Shared mutable state hub | All command modules |
| src/lib/stores/usage.ts | Frontend usage data store | Primary data flow to UI | providerMetadata.ts, types/ |
| src/lib/stores/settings.ts | Settings persistence + migration | All user preferences | format.ts, platform.ts |
| src/lib/providerMetadata.ts | Provider registry | Single source of truth for provider UI | types/index.ts |
| src/lib/bootstrap.ts | Runtime initialization | Wires settings -> stores -> IPC | All stores, tray/sync |
| src-tauri/src/usage/pricing.rs | Static + dynamic pricing | Cost calculation accuracy | models.rs, litellm.rs |
| src-tauri/src/rate_limits/claude.rs | OAuth Keychain + API rate limits | macOS rate limit display | http.rs, paths.rs |
| src-tauri/tauri.conf.json | Window, bundle, updater config | Build + runtime behavior | - |
| .github/workflows/release.yml | Multi-platform release pipeline | Builds + signs + publishes | signing secrets |

---

## Findings

### P1: Project Structure

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-001 | high | src-tauri/src/usage/parser.rs | **4,198 lines** in a single file. Contains Claude parsing, Codex parsing, file caching, change tracking, archive integration, and debug reporting. | Split into claude_parser.rs, codex_parser.rs, and cache.rs (target <1,500 lines each). | 0.90 |
| F-002 | medium | src-tauri/src/commands/float_ball.rs | **1,818 lines** for float ball commands, including complex platform-specific window management. | Extract platform-specific positioning into platform/ modules. | 0.80 |
| F-003 | medium | src-tauri/src/commands/ssh.rs | **1,536 lines** mixing IPC commands with data aggregation logic. | Extract data aggregation into usage/ssh_aggregation.rs. | 0.80 |
| F-004 | low | src/lib/components/FloatBall.svelte | **1,063 lines** — largest Svelte component. | Extract drag interaction and platform logic into separate modules. | 0.75 |
| F-005 | info | src-tauri/src/usage/ccusage.rs | **994 lines** of legacy fallback parser, `#[allow(dead_code)]`. No runtime code path reaches it. | Move to archive/. | 0.90 |

### P2: Logic Issues

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-006 | ~~critical~~ **fixed** | src-tauri/src/usage/parser.rs:404-408 | ~~Dedup hash included `isSidechain` + `agentId`~~ **Already fixed:** hash now uses only `message_id:request_id`. Test: `claude_dedupe_collapses_root_and_sidechain_and_prefers_subagent_scope`. | No action needed. | 0.95 |
| F-007 | ~~critical~~ **fixed** | src-tauri/src/usage/parser.rs:990-1013 | ~~Dedup preserved first entry~~ **Already fixed:** `upsert_claude_entry()` + `should_prefer_claude_entry()` keeps entry with highest `output_tokens`. Test: `parse_claude_dedupe_keeps_latest_output_tokens`. | No action needed. | 0.95 |
| F-008 | medium | src/lib/resizeOrchestrator.ts:48-573 | **525-line factory closure** with 19 internal state variables and 11 methods. High cognitive complexity. | Break into smaller composable functions or a class. | 0.80 |
| F-009 | low | src-tauri/src/lib.rs:77-85 | `catch_unwind()` for window positioning fallback. Defensive but masks panics. | Log the panic cause before falling back. | 0.75 |
| F-010 | info | 6 locations (frontend) | 6 empty catch blocks. All intentionally documented with comments. | Acceptable — no action needed. | 0.90 |

### P3: Code Duplication

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-011 | medium | src-tauri/src/lib.rs:158-227 + 186-228 | **Tray icon click handler and menu "Show" handler** contain nearly identical platform-dispatch positioning code (3 duplicated blocks for macOS/Windows/Linux). | Extract `show_and_position_window(window, tray_rect?)` helper. | 0.85 |
| F-012 | low | src-tauri/src/commands/ssh.rs + usage_query.rs | Similar timestamp parsing with multiple fallback formats duplicated across both files. | Centralize timestamp parsing into a shared util. | 0.70 |

### P4: Configuration Management

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-013 | medium | src/lib/utils/format.ts:1-8 | **Hardcoded exchange rates** (EUR 0.92, GBP 0.79, JPY 149.5, CNY 7.24). Comment says "updated 2025-03" — over a year stale. | Fetch rates from a public API or bump manually more often. | 0.90 |
| F-014 | low | src-tauri/src/updater/scheduler.rs | Hardcoded update check intervals (10s initial, 6h check, 12-24h backoff) without configuration. | Consider making configurable via settings. | 0.70 |
| F-015 | low | src-tauri/src/rate_limits/mod.rs:30 | `CLAUDE_MIN_REFETCH_SECS = 300` (5 min) hardcoded. Should be configurable. | Expose as a setting (backend config or frontend settings). | 0.90 |

### P5: Dead Code

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-016 | medium | 32 locations (Rust) | **32 `#[allow(dead_code)]` annotations** across 12 files. Most in parser.rs (8), updater/ (7), scheduler.rs (6). | Audit each: remove truly dead code, remove annotation for test-only code. | 0.75 |
| F-017 | low | src-tauri/src/usage/ccusage.rs | Legacy parser module (994 lines) marked `#[allow(dead_code)]`. No runtime path reaches it. | Archive to archive/ccusage.rs. | 0.90 |
| F-018 | low | src/lib/uiStability.ts:53 | `captureResizeDebugSnapshot()` returns empty object — stub from removed debug overlay. Debug overlay will not return. | Remove the stub function. | 0.90 |

### P6: Hardcoded Values

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-019 | medium | src-tauri/src/usage/litellm.rs:44 | Hardcoded URL: `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json` | Add fallback URL or make configurable. Currently no retry on failure. | 0.80 |
| F-020 | medium | src-tauri/src/usage/openrouter.rs:5 | Hardcoded URL: `https://openrouter.ai/api/v1/models` | Same as above — add fallback or cache-on-failure behavior. | 0.80 |
| F-021 | medium | src-tauri/src/rate_limits/claude.rs:265,271 | Hardcoded Anthropic API URLs for OAuth rate limit fetching. | These are stable API endpoints; acceptable but document in one central place. | 0.70 |
| F-022 | low | src-tauri/src/lib.rs:145 | Tray icon hardcoded at 44x44 pixels (appropriate for @2x retina). | Acceptable for current use. | 0.85 |
| F-023 | low | src/lib/providerMetadata.ts:52-98 | Plan tier costs hardcoded (Pro: $20, Max 5x: $100, etc.). | These are Anthropic/OpenAI public pricing — acceptable to hardcode. | 0.85 |

### P7: Dependency Health

| # | Severity | Location | Finding | Suggestion | Confidence |
|---|----------|----------|---------|------------|------------|
| F-024 | low | package.json | All 7 runtime deps are `@tauri-apps/*` plugins, caret-pinned to ^2.x. Clean and focused. | No action needed. | 0.90 |
| F-025 | low | src-tauri/Cargo.toml | 3 platform-conditional dependency blocks (macOS: objc2/security-framework, Windows: windows 0.58, Linux: gtk/webkit2gtk/cairo). Appropriate for cross-platform. | No action needed. | 0.85 |
| F-026 | info | src-tauri/src/usage/ssh_remote.rs | No explicit SSH connection timeout. `ssh2` crate connections can hang indefinitely on network issues. | Add `session.set_timeout(30_000)` or equivalent. | 0.75 |
| F-027 | info | src-tauri/src/rate_limits/http.rs | reqwest client created without explicit timeout. | Add `.timeout(Duration::from_secs(15))` to client builder. | 0.70 |

---

## Dependency Map

### Hub Files (imported by >30% of other files)

| File | Imported by |
|------|-------------|
| src/lib/types/index.ts | 22+ files (stores, views, components, utils, tray) |
| src/lib/providerMetadata.ts | 12+ files (stores, views, utils, tray) |
| src/lib/utils/format.ts | 10+ files (components, stores) |
| src/lib/stores/settings.ts | 8+ files (components, bootstrap) |
| src-tauri/src/models.rs | All command/usage/stats modules |
| src-tauri/src/usage/integrations.rs | commands/, usage/ |

### Orphan Files

| File | Notes |
|------|-------|
| src/lib/uiStability.ts | Only imported by usage.ts (1 consumer, debug stubs) |
| src-tauri/src/usage/ccusage.rs | `#[allow(dead_code)]`, legacy fallback — verify if still reachable |

### Circular Dependencies

None detected.

---

## Unresolved Questions

All questions resolved by user confirmation (2026-04-22):

- **Q1/F-005:** ccusage.rs is dead code -> archive it
- **Q2/F-015:** 5-min refetch should be configurable -> expose as setting
- **Q3/F-017:** No runtime path reaches ccusage.rs -> archive it
- **Q4/F-018:** Debug overlay will not return -> remove stub

---

## Statistics

- **Total findings:** 27
- **By severity:** 0 critical (2 fixed), 5 high, 10 medium, 8 low, 2 info
- **By priority:** P1: 5, P2: 5, P3: 2, P4: 3, P5: 3, P6: 5, P7: 4
- **Coverage:** 125/125 source files (100%)
- **Codebase size:** ~62,200 lines Rust, ~27,900 lines TS/Svelte/CSS
- **Actionable by repo-tidy:** 3 (F-005, F-017, F-018)
- **Actionable by optimize:** 8 (F-001, F-002, F-003, F-004, F-008, F-011, F-012, F-013)
