# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is TokenMonitor

A local-first **cross-platform** system tray app (Tauri v2 + Svelte 5 + Rust) that monitors Claude Code and Codex CLI token usage. It reads JSONL session logs from disk, applies pricing rules in Rust, and presents spend/rate-limit data through a native system tray popover. No API keys, no cloud sync.

Supported platforms: **macOS**, **Windows**, **Linux**.

### Platform differences

| Feature | macOS | Windows | Linux |
|---------|-------|---------|-------|
| System tray icon | Menu bar | System tray | System tray |
| Cost display | `set_title()` text beside icon | Tooltip on hover | Tooltip on hover |
| Rate limits (Claude) | OAuth via Keychain + API, CLI probe fallback | CLI probe only | CLI probe only |
| Rate limits (Codex) | JSONL session files | JSONL session files | JSONL session files |
| Glass blur effect | Supported (toggle in Settings) | Not available (opaque) | Not available (opaque) |
| Dock icon toggle | Supported | Not applicable | Not applicable |
| Autostart | LaunchAgent | Registry | XDG autostart |
| Installer | DMG (signed + notarized) | NSIS .exe | .deb package |

## Common Commands

### Prerequisites
- **Node.js** >= 18 and npm
- **Rust** (install via [rustup](https://rustup.rs/))
- Platform-specific Tauri dependencies:
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`)
  - **Windows**: Visual Studio C++ Build Tools, WebView2 (pre-installed on Windows 11)
  - **Linux**: `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`

### Development
```bash
npm install                # install frontend deps (first time / after lockfile change)
npm run tauri dev          # full app: hot-reload frontend + debug Rust backend
npm run dev                # frontend only at http://localhost:1420 (no Rust)
cd src-tauri && cargo check # type-check Rust without running
```

### Testing
```bash
npm test                   # vitest (frontend unit tests)
npm run test:watch         # vitest in watch mode
npm run test:rust          # cargo test (Rust backend tests)
npm run test:all           # both Rust and frontend tests sequentially
```

Run a single frontend test file:
```bash
npx vitest run src/lib/stores/usage.test.ts
```

Run a single Rust test:
```bash
cd src-tauri && cargo test test_name
```

### CI checks (what GitHub Actions runs on every push/PR)
```bash
npx svelte-check           # Svelte type checking
npm test                   # Vitest
cd src-tauri && cargo fmt --check   # Rust format
cd src-tauri && cargo clippy -- -D warnings  # Rust lints
cd src-tauri && cargo test  # Rust tests
```

A pre-commit hook runs all CI checks before each commit. If the hook fails, fix the issue — don't skip it.

### Building
```bash
npx tauri build            # production build (unsigned)
```

Platform-specific outputs:
- **macOS**: `src-tauri/target/release/bundle/dmg/TokenMonitor_x.y.z_aarch64.dmg`
- **Windows**: `src-tauri/target/release/bundle/nsis/TokenMonitor_x.y.z_x64-setup.exe`
- **Linux**: `src-tauri/target/release/bundle/deb/token-monitor_x.y.z_amd64.deb`

For signed/notarized macOS DMG release builds, see the "Building a release DMG" section below.

### Releasing
```bash
npm run release -- 0.6.0   # bumps version in all 3 files, commits, tags, pushes
```

The `scripts/release.sh` script handles version sync across `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`. Must be on `main` and up-to-date with origin. Tag push triggers the GitHub Actions release workflow.

## Architecture

```
Frontend (Svelte 5 + TS)           Backend (Rust)
──────────────────────────         ──────────────────────────
App.svelte                         lib.rs (app setup, tray, background refresh)
 ├─ bootstrap.ts              ←→   commands/ (IPC handlers, split by domain)
 ├─ stores/usage.ts          ←IPC→   usage_query, calendar, period, config,
 ├─ stores/rateLimits.ts     ←IPC→   tray, ssh, float_ball, logging
 ├─ stores/settings.ts             logging.rs (tracing + rolling file appender)
 ├─ providerMetadata.ts            models.rs (shared serde payload types)
 ├─ types/index.ts                 usage/ (parser, pricing, integrations)
 ├─ components/*.svelte              ssh_remote.rs, ssh_config.rs
 ├─ tray/sync.ts, title.ts        rate_limits/ (claude, claude_cli, codex, http)
 ├─ window/appearance.ts,sizing.ts tray/render.rs (native tray icon/bars)
 ├─ views/footer.ts, rateLimits.ts stats/ (change.rs, subagent.rs)
 └─ utils/platform.ts, calendar.ts platform/ (macos/, windows/, linux/)
     format.ts, logger.ts

FloatBall (separate Vite entry)
──────────────────────────
float-ball.html → float-ball.ts → FloatBall.svelte (always-on-top overlay)
```

**Data flow:** Local JSONL files → Rust `usage/parser` + `usage/pricing` → in-memory cache (Arc<RwLock<>>, 2-min TTL) → Tauri IPC → Svelte stores → UI components. Background loop refreshes tray and emits `data-updated` events every 120s. Frontend also maintains a payload cache to eliminate IPC round-trips on tab switches. SSH remote logs are synced via `usage/ssh_remote.rs` and merged into the same pipeline.

**Rate limits:** Split into `rate_limits/claude.rs` (OAuth Keychain + API, macOS only), `rate_limits/claude_cli.rs` (CLI probe fallback, all platforms), `rate_limits/codex.rs` (session file parsing), and `rate_limits/http.rs` (shared HTTP client). On Windows/Linux, the CLI probe is the primary method for Claude. Codex rate limits are read from local session files on all platforms. Both are cached and refreshed on configurable intervals per provider (see `rateLimits` in `providerMetadata.ts`).

**Pricing:** Model pricing lives in `usage/pricing.rs` with a `PRICING_VERSION` constant. When Anthropic/OpenAI update pricing, update the rates in `get_rates()` and bump `PRICING_VERSION`. Cache-write tiers follow Anthropic's standard multipliers (5m = 1.25x, 1h = 2x, read = 0.1x).

**Frontend tests** live alongside source files as `*.test.ts` (vitest, node environment). Tests mock `@tauri-apps/api` IPC calls.

**Rust tests** live in `#[cfg(test)]` modules within each `.rs` file. Use `tempfile` crate for fixtures.

**Usage integrations** are registered in `usage/integrations.rs`. Adding a new CLI provider means adding an integration ID, its log root discovery, and a parser normalization path — without modifying existing provider branches.

**Provider metadata** for the UI (tab order, labels, logos, rate-limit support, brand colors, plan tiers) is centralized in `providerMetadata.ts`. This is the single source of truth for provider-specific UI behavior.

**Tray rendering:** `tray/render.rs` generates RGBA pixel buffers for the menu bar icon + utilization bars entirely in Rust (no image library). It composites the app icon with colored progress bars at @2x retina resolution.

**Native window:** On macOS, the popover previously used `NSVisualEffectView` for glass blur effects; this has been replaced with cross-platform opaque backgrounds. Glass effect toggle and Dock icon settings are hidden on non-macOS platforms via `src/lib/utils/platform.ts` detection. The `set_glass_effect`, `set_window_surface`, and `set_dock_icon_visible` IPC commands are retained as noops for frontend compatibility.

**Commands module (Rust):** `commands.rs` is the IPC dispatch hub, split into domain-specific submodules: `usage_query` (data fetching), `calendar` (heatmap queries), `period` (time range selection), `config` (settings sync), `tray` (title/utilization rendering), `ssh` (remote device management), `float_ball` (overlay state), and `logging` (log-level control). `AppState` (defined in `commands.rs`) holds all shared state as `Arc<RwLock<>>` fields.

**Logging:** `logging.rs` initializes `tracing` with a rolling file appender for backend logs and a separate appender for frontend logs forwarded via IPC (`log_frontend_message` command). Frontend uses `utils/logger.ts` which routes through the same Rust file writer. Log files live in the platform app-data directory. Log level is runtime-configurable via a reload handle.

**Bootstrap:** `bootstrap.ts` is the startup entry point that wires settings → stores → native IPC. It applies theme, glass effect, provider/period defaults, and fires macOS-only IPC calls concurrently via `Promise.allSettled`. Dependencies are injected for testability.

**SSH Remote Devices:** The app can fetch usage logs from remote machines via SSH. `usage/ssh_remote.rs` manages per-host sync state and cache; `usage/ssh_config.rs` discovers SSH hosts from `~/.ssh/config`. The frontend `DevicesView.svelte` and `SingleDeviceView.svelte` provide the device management UI. Host configs are persisted in settings.

**Platform modules (Rust):** `platform/` contains OS-specific code compiled per target: `platform/windows/taskbar.rs` embeds a GDI panel into the Windows taskbar (between app list and system tray), `platform/windows/window.rs` handles window positioning aligned to the taskbar. `platform/macos/` and `platform/linux/` contain their respective window management. Cross-platform helpers (e.g., `clamp_window_to_work_area`) live in `platform/mod.rs`.

**Platform detection (frontend):** `utils/platform.ts` detects macOS/Windows/Linux from the user agent. UI components use `isMacOS()` to conditionally show macOS-only settings (glass blur, dock icon). The result is cached after first call.

**FloatBall:** A separate Vite entry point (`float-ball.html` → `float-ball.ts` → `FloatBall.svelte`) that renders an always-on-top draggable overlay ball. It has its own HTML file and mount target (`#float-ball`) independent of the main `App.svelte` window. Multi-entry is configured in `vite.config.ts` via `rollupOptions.input`.

**Shared types:** `lib/types/index.ts` defines shared TypeScript interfaces (`UsagePayload`, `UsagePeriod`, `HeaderTabs`, etc.) used across stores, views, and components.

**MCP integration (archived):** The MCP modules (`detect.rs`, `mcp_process.rs`, `mcp_client.rs`, `mcp_adapter.rs`) have been moved to `archive/mcp/` as they were not yet wired into the active codebase. They can be restored when MCP integration is ready.

**Note:** The `archive/` directory contains past code (ccusage CLI reporter, MCP modules, debug tools, old design docs) — none of it is part of the TokenMonitor build.

## Versioning and Releases

Version must stay in sync across three files: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`. Use `npm run release -- X.Y.Z` to handle this automatically.

Bump policy:
- **Patch** (0.0.x): bug fixes, config tweaks, build/CI changes
- **Minor** (0.x.0): new features, new UI elements, new settings
- **Major** (x.0.0): breaking changes, major redesigns, data format changes

Tag push triggers `.github/workflows/release.yml` which builds for all platforms:
- **macOS**: signed + notarized DMG
- **Windows**: NSIS installer (.exe)
- **Linux**: .deb package

## Building a Release DMG

The build requires macOS code signing and notarization. Signing files live in `signing/` (gitignored).

| What | Where |
|------|-------|
| Developer ID Application cert | Keychain: `Developer ID Application: Zimo Luo (DY9X92M8C7)` |
| App Store Connect API key | `signing/AuthKey_55WD7ZCG9H.p8` |
| API Key ID | `55WD7ZCG9H` |
| Issuer ID | `0879863a-8541-46ac-8b53-7e3f2dc3f821` |
| Team ID | `DY9X92M8C7` |

```bash
# Verify signing identity
security find-identity -v -p codesigning

# Build signed DMG
APPLE_SIGNING_IDENTITY="Developer ID Application: Zimo Luo (DY9X92M8C7)" \
APPLE_TEAM_ID="DY9X92M8C7" \
APPLE_API_KEY="55WD7ZCG9H" \
APPLE_API_ISSUER="0879863a-8541-46ac-8b53-7e3f2dc3f821" \
APPLE_API_KEY_PATH="$(pwd)/signing/AuthKey_55WD7ZCG9H.p8" \
npm run tauri build -- --bundles dmg
```

If the signing identity is missing, re-import the intermediate cert:
```bash
curl -s "https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer" -o /tmp/DeveloperIDG2CA.cer
security import /tmp/DeveloperIDG2CA.cer -k ~/Library/Keychains/login.keychain-db
```
