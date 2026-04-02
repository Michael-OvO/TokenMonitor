#!/bin/bash
# Usage: ./scripts/release.sh <version>
# Example: ./scripts/release.sh 0.3.0
#
# Bumps version in all 3 files, commits, tags, and pushes.
# The GitHub Actions release workflow picks up the tag and builds the native installers.

set -e

VERSION="$1"

# ── Validate ──────────────────────────────────────────────────────────────────

if [ -z "$VERSION" ]; then
  echo "Usage: $0 <version>"
  echo "  Example: $0 0.3.0"
  exit 1
fi

if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "Error: version must be X.Y.Z (e.g. 0.3.0) — no 'v' prefix"
  exit 1
fi

TAG="v$VERSION"

# Check we're on main and up to date
BRANCH=$(git branch --show-current)
if [ "$BRANCH" != "main" ]; then
  echo "Error: must be on main branch (currently on '$BRANCH')"
  exit 1
fi

git fetch origin main --quiet
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse origin/main)
if [ "$LOCAL" != "$REMOTE" ]; then
  echo "Error: local main is not up to date with origin/main — run 'git pull' first"
  exit 1
fi

# Check tag doesn't already exist
if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "Error: tag $TAG already exists"
  exit 1
fi

echo "Releasing $TAG..."

# ── Bump versions ─────────────────────────────────────────────────────────────

# 1. src-tauri/Cargo.toml
sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" src-tauri/Cargo.toml

# 2. src-tauri/tauri.conf.json
python3 -c "
import json, sys
with open('src-tauri/tauri.conf.json') as f:
    d = json.load(f)
d['version'] = '$VERSION'
with open('src-tauri/tauri.conf.json', 'w') as f:
    json.dump(d, f, indent=2)
    f.write('\n')
"

# 3. package.json
python3 -c "
import json
with open('package.json') as f:
    d = json.load(f)
d['version'] = '$VERSION'
with open('package.json', 'w') as f:
    json.dump(d, f, indent=2)
    f.write('\n')
"

echo "Bumped version to $VERSION in all 3 files."

# ── Verify consistency ────────────────────────────────────────────────────────

CARGO_VERSION=$(grep '^version' src-tauri/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
TAURI_VERSION=$(python3 -c "import json; print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
NPM_VERSION=$(node -p "require('./package.json').version")

if [ "$CARGO_VERSION" != "$VERSION" ] || [ "$TAURI_VERSION" != "$VERSION" ] || [ "$NPM_VERSION" != "$VERSION" ]; then
  echo "Error: version mismatch after bump — check files manually"
  echo "  Cargo.toml:       $CARGO_VERSION"
  echo "  tauri.conf.json:  $TAURI_VERSION"
  echo "  package.json:     $NPM_VERSION"
  exit 1
fi

# ── Commit, tag, push ─────────────────────────────────────────────────────────
# Note: the pre-commit hook runs all checks (svelte-check, vitest, cargo fmt,
# cargo clippy, cargo test) before the commit is created.

git add src-tauri/Cargo.toml src-tauri/tauri.conf.json package.json package-lock.json
git commit -m "chore: bump version to $VERSION"
git tag "$TAG"
git push origin main
git push origin "$TAG"

echo ""
echo "✓ Released $TAG"
echo "  GitHub Actions will now build, sign where configured, and publish the installers."
echo "  Track progress: https://github.com/Michael-OvO/TokenMonitor/actions"
