# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is TokenMonitor

A local-first **cross-platform** system tray app (Tauri v2 + Svelte 5 + Rust) that monitors Claude Code, Codex CLI, and Cursor IDE token usage. It reads JSONL session logs from disk, applies pricing rules in Rust, and presents spend/rate-limit data through a native system tray popover. No cloud sync.

Supported platforms: **macOS**, **Windows**, **Linux**. Current version: **0.13.1**.

### Platform differences

| Feature | macOS | Windows | Linux |
|---------|-------|---------|-------|
| System tray icon | Menu bar | System tray | System tray |
| Cost display | `set_title()` text beside icon | Tooltip on hover | Tooltip on hover |
| Rate limits (Claude) | OAuth via Keychain + API, statusline fallback | Statusline (from Claude Code CLI) | Statusline (from Claude Code CLI) |
| Rate limits (Codex) | JSONL session files | JSONL session files | JSONL session files |
| Rate limits (Cursor) | API (auto-detected or manual token) | API (auto-detected or manual token) | API (manual token) |
| Glass blur effect | Supported (toggle in Settings) | Not available (opaque) | Not available (opaque) |
| Dock icon toggle | Supported | Not applicable | Not applicable |
| Autostart | LaunchAgent | Registry | XDG autostart |
| Auto-update | DMG in-place replace | NSIS passive install | AppImage replace (.deb: download link) |
| Installer | DMG (signed + notarized) | NSIS .exe | .deb / .AppImage |

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

### Data flow

Local JSONL files → Rust `usage/parser` + `usage/claude_parser` + `usage/pricing` → in-memory cache (`Arc<RwLock<>>`, 2-min TTL) → Tauri IPC → Svelte stores → UI components. Background loop refreshes tray and emits `data-updated` events every 120s. Frontend also maintains a payload cache to eliminate IPC round-trips on tab switches. SSH remote logs are synced via `usage/ssh_remote.rs` and merged into the same pipeline. Completed hours are archived to `usage/archive.rs` persistent storage to survive log deletion.

### Key modules

**Frontend (`src/lib/`):** `bootstrap.ts` is the startup entry point (settings → stores → native IPC, dependencies injected for testability). Svelte stores (`stores/usage.ts`, `stores/rateLimits.ts`, `stores/settings.ts`, `stores/updater.ts`) own all reactive state and IPC calls. `providerMetadata.ts` is the single source of truth for provider-specific UI behavior (tab order, labels, logos, rate-limit support, brand colors, plan tiers). `lib/types/index.ts` defines shared TypeScript interfaces mirroring Rust structs.

**Window sizing (`windowSizing.ts` + `resizeOrchestrator.ts`):** Window height follows content height via a ResizeObserver → `set_window_size_and_align` IPC loop. Two mechanisms prevent the "small → big" cold-launch pop: (1) last applied height is persisted to `settings.json` under `window_state.height` and restored before the chart renders; (2) the orchestrator holds an `initialContentReady` gate that blocks shrink-direction requests until the first payload + first paint settle.

**Rust backend (`src-tauri/src/`):** `commands.rs` is the IPC dispatch hub, split into domain submodules (`usage_query`, `calendar`, `period`, `config`, `tray`, `ssh`, `float_ball/`, `statusline`, `updater`, `logging`). `AppState` holds all shared state as `Arc<RwLock<>>` fields. `paths.rs` is the central registry of every filesystem path the app reads.

**Pricing:** `usage/pricing.rs` has a hardcoded pricing table with `PRICING_VERSION` constant. When providers update pricing, update `get_rates()` and bump `PRICING_VERSION`. Cache-write tiers: 5m = 1.25x, 1h = 2x, read = 0.1x. Models not in the static table are resolved via LiteLLM (`usage/litellm.rs`) and OpenRouter (`usage/openrouter.rs`) APIs with 24h TTL.

**Rate limits:** `rate_limits/claude.rs` (OAuth Keychain + API on macOS; statusline-first on all platforms), `rate_limits/codex.rs` (session file parsing), `rate_limits/codex_cli.rs` (Codex CLI session rate limits), `rate_limits/cursor.rs` (Cursor API). The statusline integration is the primary rate-limit source for Claude on Windows/Linux. Cursor token is auto-detected from `state.vscdb` or manually stored via `secrets/cursor.rs` keyring layer.

**Usage integrations:** Registered in `usage/integrations.rs`. Adding a new provider means adding an integration ID, its log root discovery, and a parser normalization path — no modification to existing provider branches.

**Platform:** `platform/` has OS-specific code per target. Frontend `utils/platform.ts` detects OS and conditionally shows macOS-only settings (glass blur, dock icon). IPC commands like `set_glass_effect` are retained as noops on non-macOS for frontend compatibility.

**FloatBall:** Separate Vite entry point (`float-ball.html` → `float-ball.ts` → `FloatBall.svelte`), independent of the main window. Multi-entry configured in `vite.config.ts` via `rollupOptions.input`.

**Usage archive:** `usage/archive.rs` persists completed hourly aggregates into per-month JSONL files. Uses time-boundary partitioning: archive covers `[0..frontier]`, live source covers `(frontier..now]`.

**Tray rendering:** `tray/render.rs` generates RGBA pixel buffers for the menu bar icon + utilization bars entirely in Rust (no image library), at @2x retina resolution.

**Statusline (`statusline/`):** Installs a shell/PowerShell script as Claude Code's `statusLine` command. Claude Code invokes the script on every prompt with a JSON envelope (session id, model, rate_limits); the script appends JSONL events to `~/.tokenmonitor/statusline/events.jsonl`. TokenMonitor reads the latest event to relay Claude's own `used_percentage` for 5-hour and 7-day windows — no OAuth, no Keychain, no subprocess spawning. Submodules: `install.rs` (script installation + `~/.claude/settings.json` patching), `scripts.rs` (shell/PowerShell script bodies), `source.rs` (event-file reader with retention trimming), `windows.rs` (rolling-window math fallback for older Claude Code builds).

**Note:** The `archive/` directory contains past code (ccusage CLI reporter, MCP modules, debug tools, old design docs) — none of it is part of the build.

### Network calls at runtime

The app is local-first. Network calls are limited to:
- Dynamic pricing (LiteLLM, OpenRouter) — optional, 24h cached
- Exchange rates (Frankfurter API) — optional, 24h cached
- Cursor rate limits (Cursor API) — when enabled
- Auto-updater (GitHub releases) — configurable interval

### Testing

Frontend tests live alongside source as `*.test.ts` (vitest, node environment). Tests mock `@tauri-apps/api` IPC calls. Rust tests live in `#[cfg(test)]` modules within each `.rs` file, using `tempfile` crate for fixtures.

## Coding Style

- TypeScript / Svelte: 2-space indentation, double quotes, semicolons
- Rust: 4-space indentation (standard `rustfmt`)
- Svelte components: `PascalCase.svelte`; TS modules: `camelCase.ts`; Rust modules: `snake_case.rs`
- Shared frontend payload types go in `lib/types/index.ts`
- Prefer `cargo fmt` and `cargo clippy -- -D warnings` clean before committing Rust changes

## Commit Conventions

Imperative subject, prefixed by type: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`, `chore(release):`. Scope in parentheses when useful (e.g. `fix(macos):`, `test(pricing):`).

## Versioning and Releases

Version must stay in sync across three files: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`. Use `npm run release -- X.Y.Z` to handle this automatically.

Bump policy:
- **Patch** (0.0.x): bug fixes, config tweaks, build/CI changes
- **Minor** (0.x.0): new features, new UI elements, new settings
- **Major** (x.0.0): breaking changes, major redesigns, data format changes

### Release triggers (important)

**Merging a PR to `main` does NOT trigger a release.** It only fires `ci.yml` (tests/lint across three OSes).

The release workflow (`.github/workflows/release.yml`) is **tag-triggered**:

```yaml
on:
  push:
    tags:
      - 'v*.*.*'
  workflow_dispatch:
    inputs:
      tag: { required: true }
```

To cut a release, push a tag matching `v*.*.*`. The `scripts/release.sh` wrapper (invoked via `npm run release -- X.Y.Z`) does all of this in one shot:

1. Verifies you're on `main` and up to date with `origin`
2. Bumps the version in all three files
3. Commits `"chore(release): bump version to X.Y.Z"`
4. Creates and pushes the tag `vX.Y.Z`

The tag push fires the release workflow, which builds for all platforms:
- **macOS**: signed + notarized DMG + `.app.tar.gz` updater tarball + `.sig`
- **Windows**: NSIS installer (.exe) + `.nsis.zip` updater bundle + `.sig`
- **Linux**: `.deb` package + `.AppImage` updater bundle + `.sig`
- **`latest.json`**: updater manifest generated in the `publish` job, attached to the release

### Pre-flight before the first tag after a signing-key change

The `publish` job fails hard if *zero* updater artifacts are found (a safety net for broken signing). Before tagging for the first time after generating/rotating the Tauri updater keypair, confirm these GitHub Actions repository secrets are set:

- `TAURI_SIGNING_PRIVATE_KEY` — contents of `signing/tauri-updater.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — contents of `signing/tauri-updater-password.txt`

If either is missing, every platform job silently skips updater artifact signing, and the `publish` job errors with `No updater artifacts found — updater signing may be disabled.` To fix: upload the secrets, delete the tag (`git push --delete origin vX.Y.Z && git tag -d vX.Y.Z`), and re-run the release.

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

### Updater signing secrets

The auto-updater requires a separate Tauri signing keypair (distinct from Apple code-signing).

| What | Where |
|------|-------|
| Public key | Embedded in `src-tauri/tauri.conf.json` as `plugins.updater.pubkey` |
| Private key (local) | `signing/tauri-updater.key` (gitignored) |
| Private key password (local) | `signing/tauri-updater-password.txt` (gitignored) |
| Private key (CI) | GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY` |
| Key password (CI) | GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |

To regenerate the keypair:

```bash
npx tauri signer generate --ci -p "$(cat signing/tauri-updater-password.txt)" -w signing/tauri-updater.key -f
# Paste the .pub contents into tauri.conf.json plugins.updater.pubkey
# Upload the .key contents + password to GitHub Actions secrets
```

Linux auto-update uses the `.AppImage` bundle (not `.deb` — apt owns `.deb` installations). The release workflow produces both formats: `.deb` for package-manager installs, `.AppImage` + `.AppImage.sig` for auto-update users.
