# GitHub Workflows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add two GitHub Actions workflows — `ci.yml` (tests + lint on every push/PR) and `release.yml` (signed/notarized DMG published to GitHub Releases on version tags).

**Architecture:** Two focused workflow files in `.github/workflows/`. CI runs sequentially on `macos-latest` for fast-fail feedback. Release imports signing credentials into an ephemeral keychain, builds the DMG via Tauri, cleans up secrets, then publishes. `svelte-check` replaces bare `tsc --noEmit` for Svelte-aware type checking.

**Tech Stack:** GitHub Actions, `actions/checkout@v4`, `actions/cache@v4`, `softprops/action-gh-release@v2`, Tauri CLI, `security` (macOS keychain CLI), `svelte-check`

---

## File Map

| File | Action | Purpose |
|---|---|---|
| `package.json` | Modify | Add `svelte-check` to `devDependencies` |
| `.github/workflows/ci.yml` | Create | CI workflow: type-check, test, lint |
| `.github/workflows/release.yml` | Create | Release workflow: sign, build, publish DMG |

---

## Task 1: Add `svelte-check` to devDependencies

`svelte-check` is required by the CI workflow and must be installed via `npm ci` on the runner. It is not currently in `package.json`.

**Files:**
- Modify: `package.json`

- [ ] **Step 1: Install `svelte-check`**

```bash
npm install --save-dev svelte-check
```

- [ ] **Step 2: Verify it was added**

```bash
node -e "const p = require('./package.json'); console.log(p.devDependencies['svelte-check'])"
```

Expected: a version string like `^3.x.x` (not undefined).

- [ ] **Step 3: Verify it runs locally**

```bash
npx svelte-check
```

Expected: exits 0 with no type errors (or lists any real existing errors for you to note, but should not crash with "command not found").

- [ ] **Step 4: Commit**

```bash
git add package.json package-lock.json
git commit -m "chore: add svelte-check to devDependencies for CI"
```

---

## Task 2: Create CI Workflow

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create the workflows directory**

```bash
mkdir -p .github/workflows
```

- [ ] **Step 2: Create `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  ci:
    runs-on: macos-latest

    steps:
      - uses: actions/checkout@v4

      - name: Cache node_modules
        uses: actions/cache@v4
        with:
          path: node_modules
          key: node-${{ hashFiles('package-lock.json') }}

      - name: Cache Cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            src-tauri/target
          key: cargo-${{ hashFiles('src-tauri/Cargo.lock') }}
          restore-keys: |
            cargo-

      - name: Install Node deps
        run: npm ci

      - name: Svelte type check
        run: npx svelte-check

      - name: Vitest
        run: npm test

      - name: Rust format check
        working-directory: src-tauri
        run: cargo fmt --check

      - name: Clippy
        working-directory: src-tauri
        run: cargo clippy -- -D warnings

      - name: Rust tests
        working-directory: src-tauri
        run: cargo test
```

- [ ] **Step 3: Validate the YAML is well-formed**

```bash
python3 -c "import yaml, sys; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo "YAML valid"
```

Expected: `YAML valid`

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add CI workflow (svelte-check, vitest, clippy, cargo test)"
```

---

## Task 3: Create Release Workflow

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Create `.github/workflows/release.yml`**

```yaml
name: Release

on:
  push:
    tags:
      - 'v*.*.*'

jobs:
  release:
    runs-on: macos-latest

    steps:
      - uses: actions/checkout@v4

      - name: Cache node_modules
        uses: actions/cache@v4
        with:
          path: node_modules
          key: node-${{ hashFiles('package-lock.json') }}

      - name: Cache Cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            src-tauri/target
          key: cargo-${{ hashFiles('src-tauri/Cargo.lock') }}
          restore-keys: |
            cargo-

      - name: Install Node deps
        run: npm ci

      - name: Version consistency check
        run: |
          TAG="${GITHUB_REF_NAME#v}"
          CARGO_VERSION=$(grep '^version' src-tauri/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
          TAURI_VERSION=$(python3 -c "import json; print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
          NPM_VERSION=$(node -p "require('./package.json').version")

          FAIL=0
          if [ "$CARGO_VERSION" != "$TAG" ]; then
            echo "::error::src-tauri/Cargo.toml version ($CARGO_VERSION) does not match tag ($TAG)"
            FAIL=1
          fi
          if [ "$TAURI_VERSION" != "$TAG" ]; then
            echo "::error::src-tauri/tauri.conf.json version ($TAURI_VERSION) does not match tag ($TAG)"
            FAIL=1
          fi
          if [ "$NPM_VERSION" != "$TAG" ]; then
            echo "::error::package.json version ($NPM_VERSION) does not match tag ($TAG)"
            FAIL=1
          fi
          exit $FAIL

      - name: Set up ephemeral keychain
        env:
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
        run: |
          KEYCHAIN_PATH="$RUNNER_TEMP/build.keychain"
          KEYCHAIN_PASSWORD=$(openssl rand -base64 32)
          P12_PATH="$RUNNER_TEMP/certificate.p12"

          echo "$APPLE_CERTIFICATE" | base64 --decode > "$P12_PATH"
          security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
          security set-keychain-settings -lut 21600 "$KEYCHAIN_PATH"
          security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"

          # Import Developer ID G2 CA intermediate (may not be in runner system keychain)
          curl -fsSL "https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer" \
            -o "$RUNNER_TEMP/DeveloperIDG2CA.cer"
          security import "$RUNNER_TEMP/DeveloperIDG2CA.cer" \
            -k "$KEYCHAIN_PATH" -T /usr/bin/codesign 2>/dev/null || true

          # Import Developer ID Application cert + private key
          security import "$P12_PATH" \
            -k "$KEYCHAIN_PATH" \
            -P "$APPLE_CERTIFICATE_PASSWORD" \
            -T /usr/bin/codesign
          security list-keychain -d user -s "$KEYCHAIN_PATH"
          security set-key-partition-list \
            -S apple-tool:,apple: -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"

      - name: Write API key
        env:
          APPLE_API_KEY: ${{ secrets.APPLE_API_KEY }}
        run: |
          P8_PATH="$RUNNER_TEMP/apikey.p8"
          echo "$APPLE_API_KEY" > "$P8_PATH"
          echo "APPLE_API_KEY_PATH=$P8_PATH" >> "$GITHUB_ENV"

      - name: Build DMG
        env:
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
          APPLE_API_KEY_ID: ${{ secrets.APPLE_API_KEY_ID }}
          APPLE_API_ISSUER: ${{ secrets.APPLE_API_ISSUER }}
        run: npm run tauri build -- --bundles dmg

      - name: Cleanup secrets
        if: always()
        run: |
          security delete-keychain "$RUNNER_TEMP/build.keychain" 2>/dev/null || true
          rm -f "$RUNNER_TEMP/apikey.p8" \
                "$RUNNER_TEMP/certificate.p12" \
                "$RUNNER_TEMP/DeveloperIDG2CA.cer"

      - name: Publish release
        uses: softprops/action-gh-release@v2
        with:
          files: src-tauri/target/release/bundle/dmg/*.dmg
```

- [ ] **Step 2: Validate the YAML is well-formed**

```bash
python3 -c "import yaml, sys; yaml.safe_load(open('.github/workflows/release.yml'))" && echo "YAML valid"
```

Expected: `YAML valid`

- [ ] **Step 3: Test the version check script locally**

Run this to verify the script logic works before pushing (use the current tag `v0.2.0` since all three files are at `0.2.0`):

```bash
GITHUB_REF_NAME="v0.2.0"
TAG="${GITHUB_REF_NAME#v}"
CARGO_VERSION=$(grep '^version' src-tauri/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
TAURI_VERSION=$(python3 -c "import json; print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
NPM_VERSION=$(node -p "require('./package.json').version")

echo "Tag:   $TAG"
echo "Cargo: $CARGO_VERSION"
echo "Tauri: $TAURI_VERSION"
echo "npm:   $NPM_VERSION"
```

Expected: all four values are `0.2.0`.

Then test the mismatch case to confirm it fails correctly:

```bash
GITHUB_REF_NAME="v0.3.0"
TAG="${GITHUB_REF_NAME#v}"
CARGO_VERSION=$(grep '^version' src-tauri/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
[ "$CARGO_VERSION" != "$TAG" ] && echo "FAIL: Cargo mismatch detected (expected)" || echo "ERROR: should have failed"
```

Expected: `FAIL: Cargo mismatch detected (expected)`

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add release workflow with macOS signing and DMG publish"
```

---

## Task 4: Add GitHub Secrets

This is a manual step done in the GitHub UI at **Settings → Secrets and variables → Actions → New repository secret**.

- [ ] **Step 1: Export the Developer ID cert as `.p12`**

In Keychain Access on your Mac:
1. Find "Developer ID Application: Zimo Luo (DY9X92M8C7)"
2. Right-click → Export → save as `.p12` with a strong password

- [ ] **Step 2: Base64-encode the `.p12`**

```bash
base64 -i /path/to/DeveloperID.p12 | pbcopy
```

- [ ] **Step 3: Add all 7 secrets to GitHub**

| Secret name | Value |
|---|---|
| `APPLE_CERTIFICATE` | Paste the base64 string from Step 2 |
| `APPLE_CERTIFICATE_PASSWORD` | The password you set when exporting the `.p12` |
| `APPLE_API_KEY` | Full contents of `signing/AuthKey_55WD7ZCG9H.p8` |
| `APPLE_API_KEY_ID` | `55WD7ZCG9H` |
| `APPLE_API_ISSUER` | `0879863a-8541-46ac-8b53-7e3f2dc3f821` |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Zimo Luo (DY9X92M8C7)` |
| `APPLE_TEAM_ID` | `DY9X92M8C7` |

For `APPLE_API_KEY`:
```bash
cat signing/AuthKey_55WD7ZCG9H.p8 | pbcopy
```

- [ ] **Step 4: Verify secrets are listed** in GitHub → Settings → Secrets (values are hidden, just confirm the names appear)

---

## Task 5: Verify CI Runs

- [ ] **Step 1: Push to main (or open a PR)**

```bash
git push origin main
```

- [ ] **Step 2: Check the Actions tab on GitHub**

Navigate to the repo → Actions → "CI" workflow. Confirm it triggered and all steps pass (or investigate any failures).

- [ ] **Step 3: Confirm the `macos-latest` runner was used**

In the workflow run log, the runner OS should show as `macOS`.

---

## Task 6: Verify Release Workflow (dry-run)

> Note: A full release test will build and sign the DMG and create a real GitHub Release. Only do this when ready to ship a real version, or use a test tag like `v0.2.0-rc1` (delete the release/tag after confirming it works).

- [ ] **Step 1: Bump all three version files to the next version** (e.g. `0.2.1`)

```
src-tauri/Cargo.toml   → version = "0.2.1"
src-tauri/tauri.conf.json → "version": "0.2.1"
package.json           → "version": "0.2.1"
```

- [ ] **Step 2: Commit and tag**

```bash
git add src-tauri/Cargo.toml src-tauri/tauri.conf.json package.json
git commit -m "chore: bump version to 0.2.1"
git tag v0.2.1
git push origin main --tags
```

- [ ] **Step 3: Monitor the release workflow on GitHub Actions**

Watch for each step. Common failure points:
- Version check failing → one of the three files wasn't bumped
- Keychain setup failing → `APPLE_CERTIFICATE` secret is malformed or password wrong
- Build failing → missing env var or Tauri compilation error
- Publish failing → `GITHUB_TOKEN` permissions (needs `contents: write` — this is automatic for public repos)

- [ ] **Step 4: Verify the GitHub Release was created** with the DMG attached under Releases.
