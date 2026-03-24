# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is TokenMonitor

A local-first macOS menu bar app (Tauri v2 + Svelte 5 + Rust) that monitors Claude Code and Codex CLI token usage. It reads JSONL session logs from disk, applies pricing rules in Rust, and presents spend/rate-limit data through a native menu bar popover. No API keys, no cloud sync.

## Common Commands

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

For signed/notarized DMG release builds, see the "Building a release DMG" section below.

## Architecture

```
Frontend (Svelte 5 + TS)          Backend (Rust)
─────────────────────────         ──────────────────────────
App.svelte                        lib.rs (app setup, tray, background refresh)
 ├─ stores/usage.ts         ←IPC→ commands.rs (IPC handlers)
 ├─ stores/rateLimits.ts    ←IPC→ integrations.rs (provider selection, log root discovery)
 ├─ stores/settings.ts            parser.rs (JSONL scanning, normalization, aggregation)
 ├─ providerMetadata.ts           pricing.rs (model-family pricing, cache-write tiers)
 ├─ components/*.svelte           rate_limits.rs (Claude/Codex rate-limit acquisition)
 ├─ traySync.ts                   tray_render.rs (native tray title/icon)
 └─ windowAppearance.ts           models.rs (shared serde payload types)
                                  change_stats.rs, subagent_stats.rs
```

**Data flow:** Local JSONL files → Rust parser + pricing → in-memory cache (Mutex<HashMap>, 2-min TTL) → Tauri IPC → Svelte stores → UI components. Background loop refreshes tray and emits `data-updated` events every 120s.

**Frontend tests** live alongside source files as `*.test.ts` (vitest, node environment). Tests mock `@tauri-apps/api` IPC calls.

**Rust tests** live in `#[cfg(test)]` modules within each `.rs` file. Use `tempfile` crate for fixtures.

**Usage integrations** are registered in `integrations.rs`. Adding a new CLI provider means adding an integration ID, its log root discovery, and a parser normalization path — without modifying existing provider branches.

**Provider metadata** for the UI (tab order, labels, logos, rate-limit support) is centralized in `providerMetadata.ts`.

## Versioning and Releases

Version must stay in sync across three files: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.

Bump policy:
- **Patch** (0.0.x): bug fixes, config tweaks, build/CI changes
- **Minor** (0.x.0): new features, new UI elements, new settings
- **Major** (x.0.0): breaking changes, major redesigns, data format changes

Release steps:
1. Update version in all three files
2. `cd src-tauri && cargo generate-lockfile` to sync `Cargo.lock`
3. Commit: `chore(release): bump version to X.Y.Z`
4. Tag: `git tag -a vX.Y.Z -m "vX.Y.Z"`
5. Push: `git push origin main --follow-tags`

Tag push triggers `.github/workflows/release.yml` (build + sign + notarize + publish DMG).

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
