# TokenMonitor Development Guide

This guide describes the current repository. User-facing setup and features live in
[README.md](../README.md); release history lives in [CHANGELOG.md](../CHANGELOG.md).

## Prerequisites

- Node.js 18 or newer and npm
- A current stable Rust toolchain
- Tauri system dependencies for your platform:
  - macOS: Xcode Command Line Tools
  - Windows: Visual Studio C++ Build Tools and WebView2
  - Linux: WebKitGTK 4.1, AppIndicator, librsvg, and `patchelf`

## Setup and Run

```bash
npm ci
npx tauri dev
```

For frontend-only layout work, run `npm run dev` and open
`http://localhost:1420`. Native IPC calls are unavailable in that mode.

## Repository Layout

```text
.
├── src/                         Svelte 5 frontend
│   ├── App.svelte                 Main popover shell
│   ├── float-ball.ts              FloatBall entry point
│   └── lib/
│       ├── components/            UI components; settings/ and float-ball/ own feature files
│       ├── permissions/           Permission disclosures and statusline setup
│       ├── stores/                Settings, usage, rate-limit, and updater state
│       ├── tray/                  Tray synchronization and title formatting
│       ├── types/                 Shared frontend payload types
│       ├── utils/                 General formatting and platform helpers
│       ├── views/                 View-model calculations
│       └── window/                Appearance, sizing, and resize orchestration
├── src-tauri/                   Rust/Tauri backend
│   ├── capabilities/             Webview permission grants
│   ├── icons/                    Application and tray icons
│   ├── resources/                Native build/test resources
│   └── src/
│       ├── commands/              Tauri IPC commands
│       ├── platform/              OS-specific window behavior
│       ├── rate_limits/           Provider rate-limit integrations
│       ├── secrets/               Keyring-backed credential access
│       ├── single_instance/       Process ownership and focus protocol
│       ├── stats/                 Change and subagent aggregation
│       ├── statusline/            Claude statusline installation
│       ├── tray/                  Native tray rendering
│       ├── updater/               Update state and scheduling
│       └── usage/                 Parsing, pricing, cache, archive, and SSH sync
├── build/                       Installer build code and platform configs
├── scripts/                     Version/release helpers
├── tests/                       Cross-layer repository invariant tests
└── docs/                        User guides and maintained test procedures
```

Tests are colocated with frontend and build modules. Rust unit tests use inline
`#[cfg(test)]` modules. `tests/` is reserved for checks that span multiple layers.

## Runtime Flow

1. The frontend requests usage for a provider, period, and offset through Tauri IPC.
2. Rust reads local Claude, Codex, or Cursor data, plus configured SSH sources.
3. Provider parsers normalize events and the pricing layer computes costs. Claude
   rate limits prefer fresh statusline events; other configured sources are fallbacks.
4. Memory and disk caches speed up repeat queries; completed hours are persisted in
   the usage archive.
5. Svelte stores project the payload into charts, summaries, tray state, and the
   FloatBall overlay.

All filesystem locations read by the application must be registered in
`src-tauri/src/paths.rs`. Runtime network access is limited to enabled features such
as pricing/exchange-rate refreshes, provider rate limits, SSH connections, and update
checks.

## Validation

Run the smallest relevant check while developing, then the complete set before a PR:

```bash
npx svelte-check
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run test:rust
```

Useful focused commands:

```bash
npx vitest run src/lib/stores/usage.test.ts
cargo test --manifest-path src-tauri/Cargo.toml test_name
```

Windows CI sets `TM_EMBED_TEST_MANIFEST=1` and runs `cargo test --lib`; the build
script then embeds `src-tauri/resources/windows-test.manifest` in the test binary.

## Build and Release

Build the local platform directly with Tauri:

```bash
npx tauri build
```

Build and collect installer artifacts under `outputs/<platform>/`:

```bash
npm run build:installers -- --platform current
```

Versions must match in `package.json`, `src-tauri/Cargo.toml`,
`src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json`. The release helper updates
them, commits, tags, and pushes:

```bash
npm run release -- X.Y.Z
```

Tag pushes trigger the cross-platform release workflow. See
[`testing/auto-update.md`](testing/auto-update.md) for the maintained updater
smoke-test matrix.

## Conventions

- TypeScript/Svelte: 2 spaces, double quotes, semicolons.
- Rust: `cargo fmt` defaults.
- Keep frontend payload types in `src/lib/types/`.
- Keep path discovery centralized in `src-tauri/src/paths.rs`.
- Add focused tests for parsing, pricing, provider merges, stores, updater behavior,
  and secret handling.
- Do not commit `dist/`, `coverage/`, `outputs/`, `src-tauri/target/`, generated Tauri
  schemas, credentials, logs, or local environment files.

## Troubleshooting

- Blank UI: run `npx svelte-check`, then inspect the Tauri terminal and app log.
- No usage: verify Claude/Codex/Cursor have created local data and that usage access is
  enabled in the app.
- Stale local process: quit TokenMonitor from the tray before restarting `tauri dev`.
- Rust dependency errors: update the stable toolchain and verify the platform-specific
  Tauri packages above are installed.
