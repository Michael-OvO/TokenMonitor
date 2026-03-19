# GitHub Workflows Design

**Date:** 2026-03-19
**Status:** Approved

## Overview

Two GitHub Actions workflows for TokenMonitor: a CI workflow that validates every push and pull request, and a release workflow that builds, signs, notarizes, and publishes a DMG on version tags.

The app is macOS-only (Tauri 2 with ObjC bindings), so both workflows run on `macos-latest`.

---

## Workflow 1: CI (`ci.yml`)

### Triggers

- Push to `main`
- All pull requests

### Runner

`macos-latest` — required because the Rust backend uses `#[cfg(target_os = "macos")]` and ObjC framework bindings that cannot compile on Linux.

### Steps (single job, sequential)

1. **Checkout** — `actions/checkout@v4`
2. **Cache node_modules** — keyed on `package-lock.json` hash
3. **Cache Cargo** — registry + build artifacts keyed on `src-tauri/Cargo.lock` hash (the lock file lives at `src-tauri/Cargo.lock`, not the repo root)
4. **Install Node deps** — `npm ci`
5. **Svelte type check** — `npx svelte-check` covers both `.svelte` components and `.ts` files with Svelte-aware resolution. A bare `tsc --noEmit` would fail because `tsconfig.json` includes `src/**/*.svelte` and vanilla `tsc` cannot process `.svelte` imports — `svelte-check` is the correct single tool for full type coverage in this project.
6. **Vitest** — `npm test`
7. **Rust format check** — `cargo fmt --check` (`working-directory: src-tauri`)
8. **Clippy** — `cargo clippy -- -D warnings` (`working-directory: src-tauri`)
9. **Rust tests** — `cargo test` (`working-directory: src-tauri`)

### Design notes

- Steps are sequential so a fast failure (e.g. type error) doesn't waste time waiting for Rust compilation.
- `cargo clippy -- -D warnings` treats warnings as errors, enforcing lint hygiene on every PR.
- Caching is critical — cold Rust builds are ~3–5 minutes; warm cache brings it under a minute.
- All `cargo` commands run with `working-directory: src-tauri` — there is no root-level `Cargo.toml` or workspace manifest.
- `svelte-check` must be in `devDependencies` so `npm ci` installs a pinned version. It is not currently listed there and must be added as part of implementing this workflow.

---

## Workflow 2: Release (`release.yml`)

### Trigger

Push of a tag matching `v*.*.*` (e.g. `v0.2.1`).

### Runner

`macos-latest`

### Steps

1. **Checkout** — `actions/checkout@v4`
2. **Cache** — same Cargo and node_modules caches as CI (`src-tauri/Cargo.lock` for cargo cache key)
3. **Install Node deps** — `npm ci`
4. **Version consistency check** — assert that the versions in all three version files match the pushed tag:
   - `src-tauri/Cargo.toml` → `version = "X.Y.Z"`
   - `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"` (Tauri reads this for `Info.plist` and the DMG filename)
   - `package.json` → `"version": "X.Y.Z"`

   Fails early with a clear error message if any file is out of sync.

5. **Keychain setup**:
   - Decode `APPLE_CERTIFICATE` secret (base64 → temp `.p12` file)
   - Create ephemeral keychain with a random password
   - Import `DeveloperIDG2CA.cer` intermediate CA from Apple's CA page (required for a complete trust chain — the runner's system keychain may not have it)
   - Import `.p12` into the ephemeral keychain
   - Set ephemeral keychain as default

6. **Write API key** — write `APPLE_API_KEY` secret content to a temp `.p8` file at a known path

7. **Build** — run `tauri build -- --bundles dmg` with env vars:
   - `APPLE_SIGNING_IDENTITY`
   - `APPLE_TEAM_ID`
   - `APPLE_API_KEY_ID`
   - `APPLE_API_ISSUER`
   - `APPLE_API_KEY_PATH` (pointing to the temp `.p8`)

8. **Cleanup** — runs with `if: always()` so secrets are cleaned up even on build failure. Deletes:
   - The ephemeral keychain
   - The temp `.p8` file
   - The decoded temp `.p12` file

9. **Publish release** — `softprops/action-gh-release` uploads `src-tauri/target/release/bundle/dmg/*.dmg` as a release asset. Release name is the tag. Publishes immediately (not a draft).

### Required GitHub Secrets

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | Developer ID `.p12` cert, base64-encoded |
| `APPLE_CERTIFICATE_PASSWORD` | Password for the `.p12` |
| `APPLE_API_KEY` | Contents of `AuthKey_55WD7ZCG9H.p8` |
| `APPLE_API_KEY_ID` | `55WD7ZCG9H` |
| `APPLE_API_ISSUER` | `0879863a-8541-46ac-8b53-7e3f2dc3f821` |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Zimo Luo (DY9X92M8C7)` |
| `APPLE_TEAM_ID` | `DY9X92M8C7` |

### How to export the `.p12`

```bash
# In Keychain Access: right-click the cert → Export → .p12 format, set a password
# Then base64-encode it for the secret:
base64 -i DeveloperID.p12 | pbcopy
```

### Release process

When releasing, bump the version in **all three places** before tagging:

1. `src-tauri/Cargo.toml` → `version = "X.Y.Z"`
2. `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"`
3. `package.json` → `"version": "X.Y.Z"`

Then:

```bash
git commit -m "chore: bump version to X.Y.Z"
git tag vX.Y.Z && git push origin vX.Y.Z
```

The release workflow triggers automatically, builds the signed and notarized DMG, and publishes it to GitHub Releases.

---

## Implementation notes

- **Add `svelte-check` to `devDependencies`** before implementing CI — it is not currently listed in `package.json` and `npm ci` will not install it.
- The `tauri.conf.json` `"targets": "all"` setting is overridden by `--bundles dmg` on the CLI, which is the desired behavior for release builds.

---

## File Layout

```
.github/
  workflows/
    ci.yml
    release.yml
```

---

## Out of Scope

- Linux/Windows builds (app is macOS-only)
- Automatic version bumping or changelog generation
- Draft releases or pre-release channels
- Dependabot (can be added independently)
