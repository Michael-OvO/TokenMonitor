# TokenMonitor Development Guide

This guide describes the current repository. User-facing setup and features live in
[README.md](../README.md); release history lives in [CHANGELOG.md](../CHANGELOG.md).

## Prerequisites

- Node.js 18 or newer and npm (CI and the lockfile use Node 26 / npm 11)
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
2. Rust reads local Claude, Codex, Cursor, or Kimi data, plus configured SSH sources.
3. Provider parsers normalize events and the pricing layer computes costs. Claude
   rate limits prefer fresh statusline events; other configured sources are fallbacks.
4. Memory and disk caches keep each computed view until the next refresh; completed
   hours are persisted in the usage archive.
5. Svelte stores project the payload into charts, summaries, tray state, and the
   FloatBall overlay.

All filesystem locations read by the application must be registered in
`src-tauri/src/paths.rs`. Runtime network access is limited to enabled features such
as pricing/exchange-rate refreshes, provider rate limits, SSH connections, and update
checks.

### Refresh cycle

`src-tauri/src/refresh.rs` owns all periodic work; nothing else sweeps the logs or
publishes on a timer. Its loop runs cycle 0 at launch (after the frontend's
`refresh_ready`, or 10 s), then one cycle per aligned tick (local midnight + 1 s +
k × interval) and whenever `refresh::request_refresh` is called. A cycle runs in this
order (`cycle_steps`):

1. **CursorToday** and **RateLimits**: network and child-process I/O, outside the
   compute gate. Rate limits are probed one provider at a time; providers not reached
   within min(20 s, interval / 2) keep their cached value. The Claude statusline is read
   here first, so there is no separate statusline poll.
2. **Cleanup** (cycle 0 only): purge duplicate device sources. After the I/O, so the
   launch's cold tray compute, which holds the gate, runs beside that I/O.
3. **Sample**: the only log sweep and cache invalidation. It applies price and FX
   tables fetched since the last sample, revalidates SSH records, and drops stale
   payloads (cycle 0 also drops the previous session's disk copies). The sweep
   (`UsageParser::sweep`) stats every listed directory and reads again only those
   whose stamp moved; it stats the files written within a week, and every file at a
   full sweep (cycle 0, hourly, in a new day, after `request_refresh`, and after a
   popover show). A path that cannot be read but is not gone keeps its stamp. Lines
   appended to listed logs drop only the views that count that integration and reach
   the days they may be dated (`view_built_on_appended_logs`); a file that came, went
   or shrank, a Cursor chat change, Cursor remote data, SSH records, prices, Kimi model
   names or an archive change drop them all. A 5h view whose official reset is still
   ahead keeps its key across samples; its burn rate is recomputed at each hit. Today's
   Day view key carries the hour, so its chart still reaches the current hour.
4. **Tray** and **ActiveView**: compute the day cost and the view the popover last
   asked for.
5. **Publish**: store the tray cost, publish staged plan budgets, paint the tray, and
   emit one `data-updated`, so the tray, FloatBall and popover change together.

Independent jobs then run in evenly spaced slots until 1 s before the next tick
(`plan_slots` / `slot_offsets`): archive (then a detached auto-export pass),
plan-budget recomputes (one per provider, skipped while its data version, meter
readings and price table are unchanged, for up to an hour; `plan_budget::due`), and an
hourly price-table check. SSH hosts sync in a detached task every 10th cycle; a host
whose host key fails verification is left out for an hour, doubling to a day, until a
manual sync, a passing Test or an SSH host change. Results that arrive in the
background are shown at the next publish.

Rules that keep this working:

- Heavy CPU work holds `AppState.compute`, a FIFO mutex, so at most one such job runs
  at a time. Network and child-process waits never hold it. The rayon pool uses half
  the cores.
- An interactive `get_usage_data` miss does not wait for a tick: it computes under the
  gate (usually behind one job at most) from the current sample, since the session
  listings stay frozen between samples. The popover's warm-ups pass `background: true`
  and are queued one at a time. They start after the first publish, never ask for
  `year` or for a view fetched since the last publish, and a navigation drops those
  still queued.
- Only the Publish step and a Cursor widening fetch (`usage_query.rs`) emit
  `data-updated`. A widening drops only the views that count Cursor and start before
  the days the cache had. User actions that change data (clear cache, Cursor auth, import,
  Sync All, manual SSH sync, SSH host changes, Header Tabs changes, enabling usage
  access or rate limits) call `request_refresh`.
- Archiving runs only in a cycle slot after that cycle's full sweep, or after a sweep
  in a user export/import (`sweep_before_user_archive`), and only for the hours before
  the sweep's own: what was logged after it is not in the file cache yet, and between
  full sweeps an append to a file not written for a week may not be either.
- An auto-export pass reads and writes its folder (possibly a network or cloud share)
  on the blocking pool outside the gate, and gives up waiting after 30 s; only its
  archive merge takes the gate. One pass runs at a time. The archive keeps each peer
  file's mtime at its merge (`.merged-peers.json`, for one folder), so an unchanged
  file is not merged again, across restarts too.
- Refresh interval Off (0) runs cycles only at launch, 00:00:01, user actions, and
  `refresh_on_focus` when the last sample is at least 30 s old. Its slots run within
  60 s of the cycle.
- There is no automatic payload warmup at startup; the Settings warm-up runs through
  the same gate.
- Every request the refresh makes is bounded: 15 s for rate-limit HTTP, 30 s for
  pricing and FX tables, 12 s per Cursor usage page, and 60 s per SSH host sync,
  manual or background (a timed-out ssh child is killed). A forced rate-limit probe
  starts only within 30 s of the ask, and skips a provider another probe read while
  it waited.

`backend.log` has one `[PROFILE] cycle` line per cycle and one `[PROFILE] slot` line
per slot job.

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
