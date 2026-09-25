# TokenMonitor — Agent Guide

Local-first, cross-platform (macOS/Windows/Linux) system-tray app that monitors Claude Code, Codex CLI,
Cursor IDE, and Kimi Code token usage. Stack: Tauri v2 + Svelte 5 frontend (`src/`), Rust backend (`src-tauri/`). It parses JSONL
session logs from disk, prices them in Rust, and shows spend + rate limits in a tray popover and an optional
FloatBall overlay. Entry points: `src/main.ts` (main window), `src/float-ball.ts` (FloatBall, separate Vite
entry), `src-tauri/src/main.rs` → `lib.rs` (backend). Root `README.md` covers product overview and architecture; `docs/DEVELOPMENT.md` is the
maintained dev guide; `CHANGELOG.md` at the repository root is the release history. Current version: 0.15.x.

## Commands

- `npm ci` — install frontend deps from the lockfile
- `npx tauri dev` — full app (hot-reload frontend + debug Rust backend); runs until killed
- `npm run dev` — Vite frontend only at http://localhost:1420 (no native IPC); runs until killed
- `npm test` — Vitest, one-shot (`src/**/*.test.ts`, `build/**/*.test.mjs`, `tests/**/*.test.mjs`)
- `npx vitest run src/lib/stores/usage.test.ts` — single frontend test file
- `npm run test:watch` — Vitest watch mode (never exits); `npm run test:coverage` — V8 coverage into `coverage/`
- `npm run test:rust` — `cd src-tauri && cargo test` (see Gotchas for Windows)
- `cargo test --manifest-path src-tauri/Cargo.toml --lib test_name` — single Rust test
- `npm run test:all` — Rust then frontend tests
- CI parity: `npx svelte-check`, `cargo fmt --manifest-path src-tauri/Cargo.toml --check`,
  `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- `npm run build` — frontend into `dist/`; `npx tauri build` — production desktop bundles
- `npm run build:installers -- --platform current` — build + collect installers under `outputs/<platform>/`
- `npm run release -- X.Y.Z` — bump versions, commit, tag, push (must be on up-to-date `main`; the tag push
  triggers the release workflow — do not run casually)

## Architecture

```
src/                     Svelte 5 frontend
  App.svelte               main popover shell
  lib/bootstrap.ts         startup entry (settings → stores → native IPC, deps injected for testability)
  lib/providerMetadata.ts  single source of truth for provider UI behavior (tabs, labels, colors, plans)
  lib/components/          UI components; settings/ and float-ball/ own feature files
  lib/stores/              usage / rateLimits / settings / updater state + IPC calls
  lib/permissions/         privacy disclosures and Claude statusline setup
  lib/tray/  lib/views/    tray title/sync formatting; view-model calculations
  lib/window/              appearance, sizing, resizeOrchestrator (content-height → window-resize IPC loop)
  lib/types/               shared payload types mirroring Rust structs
src-tauri/src/           Rust backend
  commands/                Tauri IPC dispatch, split by domain (usage_query, calendar, tray, ssh, statusline…)
  usage/                   parsers (claude/codex/cursor/kimi), pricing, money (display currency), caches, archive, SSH remote sync
  rate_limits/             claude + claude_cli (`claude -p /usage` probe), codex + codex_cli, cursor, kimi
  statusline/              installs a shell/PowerShell statusline script into Claude Code; reads its JSONL events
  stats/  secrets/         change/subagent aggregation; keyring-backed credential access
  single_instance/         process ownership + focus protocol
  tray/  platform/         RGBA tray icon rendering in pure Rust; OS-specific window behavior
  updater/  paths.rs       update scheduling/state/channels; central registry of every filesystem path the app reads
  refresh.rs               the refresh loop: owns all periodic work (sample → publish, then spaced slot jobs)
  plan_budget.rs           $ per rate-limit window, learned from meter readings vs local spend per model
  ops.rs / ops.json        vendor-facing constants (API hosts, plan budgets, LiteLLM cache TTL)
build/                   installer build code (index.mjs) + per-platform tauri config overlays
scripts/                 release.sh, sync-tauri-versions.mjs
tests/                   cross-layer repository invariant tests (*.test.mjs)
docs/                    DEVELOPMENT.md, tutorial.md, testing/ procedures
```

Data flow: local JSONL logs → Rust parsers + pricing → in-memory/disk caches → Tauri IPC → Svelte stores → UI.
Claude rate limits prefer a fresh statusline event, then the CLI probe
`claude -p /usage --no-session-persistence --safe-mode` (CLIs that reject those flags get the old fixed-session
form), then the OAuth usage API (cooldown-gated); the OAuth token comes from `~/.claude/.credentials.json` or
Claude Code's own Keychain item read through `/usr/bin/security`, never from an app-owned Keychain entry. Codex
limits use the newest meters Codex logged (`token_count`) when newer than the last reading and under 285 s old,
else the `codex app-server` probe, which on Windows starts the vendored `codex.exe` behind the npm shim. Kimi
limits use the Kimi usage API and refresh their token like the Kimi CLI does. The Usage tab covers the
official 5h reset window from cached rate limits, or a rolling five hours when none is cached. Completed hours
are persisted to the usage archive so history survives log deletion. Models missing from the static pricing
table (`usage/pricing.rs`, bump `PRICING_VERSION` when editing) resolve via LiteLLM/OpenRouter with 24h TTL.

Refresh: `refresh.rs` owns all periodic work. Each refresh (launch, then every aligned interval tick) runs its
I/O first (Cursor today, rate-limit probes one provider at a time), then one sample (the only log sweep and
cache invalidation; it stats the files written within a week, and every file hourly, after user actions and
after a popover show; an append drops only the views it can reach), computes the tray cost and open view
behind the `AppState.compute` FIFO gate (one heavy CPU job at a time), and publishes once; only it and the
Cursor widening fetch (`usage_query.rs`) emit `data-updated`. Archive/export, plan budgets (one job per
provider, skipped while its logs, meter readings and prices are unchanged, for up to an hour) and the hourly
price check then run in evenly spaced slots; SSH syncs detached every 10th cycle (a host whose host key
fails is held back 1 h, doubling to a day). Background results appear at the next refresh; user actions call
`refresh::request_refresh`. Interval Off refreshes only at launch, 00:00:01, user actions, and popover focus
once the last sample is ≥ 30 s old. There is no statusline poll (it is read at each refresh) and no automatic
startup warmup. WebView2 keeps painting a hidden window, so every popover show/hide calls
`emit_popover_visibility` (`lib.rs`); while hidden the page (`lib/visibility.ts`) pauses CSS animations, the
footer/bar timers and rate-limit retries, and refetches on `data-updated` at most every 5 min (the rest wait
for the next show).

Platform notes (verified in code): tray cost text uses `set_title()` beside the icon on macOS; on
Windows/Linux `set_title` is a noop and the cost goes in the tooltip (`commands/tray.rs`). On macOS the tray
menu is detached from the NSStatusItem right after the tray is built and re-attached only while a right-click
presents it (`platform/macos/tray_menu.rs`): macOS 27 stops forwarding clicks to the tray view while a menu is
attached, which made left-click open the menu instead of the popover. Drop that module once Tauri ships
tray-icon >= 0.25, which does the same upstream. Glass effect is set
from the frontend through Tauri's window-effects API (`setNativeGlassEffect` in `lib/window/appearance.ts`):
HudWindow on macOS, Mica/Acrylic on Windows, noop on Linux. The old Rust `set_glass_effect` and
`set_window_surface` commands were no-ops and are gone.

## Conventions

- TypeScript/Svelte: 2-space indent, double quotes, semicolons. Rust: rustfmt defaults (4-space).
- Naming: `PascalCase.svelte` components, `camelCase.ts` modules, `snake_case.rs` modules.
- Tests are colocated: `*.test.ts` beside frontend source, inline `#[cfg(test)]` modules in Rust; `tests/` is
  only for cross-layer checks.
- Every filesystem location the app reads must be registered in `src-tauri/src/paths.rs`.
- Shared frontend payload types live in `src/lib/types/`.
- Commits: short imperative subject with type prefix — `feat:`, `fix:`, `docs:`, `test(scope):`, `chore(release):`.

## Gotchas

- Windows Rust tests: plain `cargo test` builds a test binary without the Common-Controls v6 manifest and it
  fails to load (0xC0000139 STATUS_ENTRYPOINT_NOT_FOUND). Use
  `TM_EMBED_TEST_MANIFEST=1 cargo test --lib` from `src-tauri/` instead (see `src-tauri/build.rs` and
  `.github/workflows/ci.yml`); this is what `npm run test:rust` cannot do for you on Windows.
- `npm run tauri …` triggers the `pretauri` hook (`scripts/sync-tauri-versions.mjs`), which may run
  `npm install` to realign `@tauri-apps/api` with the tauri crate minor version; `npx tauri …` skips it.
- Version must stay in sync across four files: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`
  (token-monitor entry), `src-tauri/tauri.conf.json`. Always use `npm run release -- X.Y.Z`.
- Merging to `main` does NOT release; the release workflow is tag-triggered (`v*.*.*`).
- Quit any running TokenMonitor from the tray before restarting `tauri dev` — it is single-instance.
- `CLAUDE.md`, `.claude/worktrees/` and `.claude/settings.local.json` are gitignored (local-only); `signing/`,
  `outputs/`, `coverage/` too.
