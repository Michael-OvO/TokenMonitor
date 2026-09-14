<p align="center">
  <img src="docs/assets/avatar.svg" width="128" height="128" alt="TokenMonitor icon" />
</p>

<h1 align="center">TokenMonitor</h1>

<p align="center">
  <strong>Local-first cross-platform system tray app for monitoring Claude Code, Codex CLI, Cursor IDE, and Kimi Code token usage</strong>
</p>

<p align="center">
  A fast, compact way to understand spend, burn rate, model mix, and usage history without leaving the desktop.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS%20|%20Windows%20|%20Linux-black?style=flat-square" alt="Cross-platform" />
  <img src="https://img.shields.io/badge/Tauri-v2-24C8D8?style=flat-square&logo=tauri&logoColor=white" alt="Tauri v2" />
  <img src="https://img.shields.io/badge/Svelte-5-FF3E00?style=flat-square&logo=svelte&logoColor=white" alt="Svelte 5" />
  <img src="https://img.shields.io/badge/Rust-native-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/local--first-usage%20analytics-2F855A?style=flat-square" alt="Local-first usage analytics" />
  <img src="https://img.shields.io/badge/license-GPL--3.0-blue?style=flat-square" alt="License" />
</p>

<p align="center">
  <img src="docs/assets/hero.png" alt="TokenMonitor hero – Understand Your AI Usage. Instantly." width="800" />
</p>

<p align="center">
  <a href="https://github.com/Michael-OvO/TokenMonitor/releases/latest">
    <img src="https://img.shields.io/badge/Download-macOS%20.dmg-111827?style=for-the-badge&logo=apple&logoColor=white" alt="Download macOS dmg" />
  </a>
  <a href="https://github.com/Michael-OvO/TokenMonitor/releases/latest">
    <img src="https://img.shields.io/badge/Download-Windows%20.exe-0078D4?style=for-the-badge&logo=windows&logoColor=white" alt="Download Windows exe" />
  </a>
  <a href="https://github.com/Michael-OvO/TokenMonitor/releases/latest">
    <img src="https://img.shields.io/badge/Download-Linux%20.deb-FCC624?style=for-the-badge&logo=linux&logoColor=black" alt="Download Linux deb" />
  </a>
  <a href="#build-from-source">
    <img src="https://img.shields.io/badge/Build-from%20source-2563EB?style=for-the-badge&logo=rust&logoColor=white" alt="Build from source" />
  </a>
</p>

---

TokenMonitor is a local-first cross-platform system tray app for people who use Claude Code, Codex, and Cursor IDE heavily and want a cleaner, faster way to monitor usage.

It reads the session logs already on your machine, applies provider-aware pricing rules in Rust, and turns them into a compact desktop interface for current-session spend, history, model mix, and rate-limit context.

No provider API key is required for local usage history. No cloud sync. No runtime dependency on `ccusage` or any other external CLI.

## Quick Install

### Download

Grab the installer for your platform from the [latest release](https://github.com/Michael-OvO/TokenMonitor/releases/latest):

| Platform | Installer | Notes |
|----------|-----------|-------|
| **macOS** | `.dmg` | Open the DMG, drag to Applications |
| **Windows** | `.exe` (NSIS) | Run the installer, follow prompts |
| **Linux** | `.deb` | `sudo dpkg -i token-monitor_*.deb` |

## Features

### Usage Monitoring

- Official 5-hour rate-limit window spend, burn rate, and context
- Period views for `5h`, `day`, `week`, `month`, and `year`
- Historical navigation with offset-based browsing
- Claude-only, Codex-only, Cursor-only, Kimi-only, and merged provider views
- Optional live tray spend display for quick check-ins
- Agent/subagent cost breakdown with proportion visualization

### Analysis & Visualization

- Per-model cost and token breakdowns
- Hidden-model filtering
- Bar-chart, line-chart, and pie-chart modes
- Calendar heatmap for monthly usage patterns
- Active-session footer with pacing and recent spend context

### Rate Limits & Session Context

- Claude, Codex, and Cursor rate-limit panels when provider data is available
- Utilization, reset timing, cooldown state, and pace hints
- Cursor plan usage + spend limit tracking via API
- Claude Code statusline events provide fresh, server-reported limits without a network request
- OAuth, CLI, and local-session fallbacks cover stale or unavailable primary data

### Auto-Updater

- In-app update banner with download progress
- Tray icon red badge dot when an update is available
- In-app update checks every 6 hours with exponential backoff after failures
- Skip / Later / Update Now actions
- Persisted updater state across restarts (skipped versions, last check)
- Platform-aware: macOS/Linux auto-install, Windows passive NSIS, Linux .deb shows "Download" link

### SSH Remote Devices

- Fetch usage logs from remote machines via SSH
- Auto-discover hosts from `~/.ssh/config`
- Per-host sync state and caching
- Unified view merging local and remote usage data

### FloatBall Overlay

- Always-on-top draggable overlay ball showing live spend
- Separate Vite entry point, independent of the main window
- Toggle on/off from settings

### Desktop UX & Settings

- Native system tray popover on all platforms
- Launch-at-login support (LaunchAgent / Registry / XDG autostart)
- Theme, currency, refresh interval, and branding controls
- Native glass effect with a toggle (macOS vibrancy, Windows Mica/Acrylic)
- Integrated settings and calendar panels inside the same popover flow
- First-launch welcome card with permission disclosures and opt-in toggles
- Dynamic exchange rates (USD, EUR, GBP, JPY, CNY) with 24h cache
- Manual import/export and optional automatic usage exports
- Cache clearing and warm-up controls for large histories
- Selectable update channels for the official project and compatible forks

### Pricing Accuracy

- Native Rust parsing of local session logs with no runtime dependency on `ccusage`
- Claude cache-write pricing separated into 5-minute and 1-hour tiers
- Codex/OpenAI cached input separated from standard input
- Codex `token_count` normalization for both per-turn and cumulative log formats
- Reasoning output folded into output billing where applicable
- Dynamic pricing from LiteLLM and OpenRouter APIs (24h TTL cache)
- Usage archive for persistent hourly aggregates (survives log deletion)

#### Claude Cache-Write Tiers

| Model | 5m Cache Write | 1h Cache Write | Difference |
|---|---:|---:|---:|
| Opus 4.6 | $6.25 / MTok | $10.00 / MTok | +60% |
| Sonnet 4.6 | $3.75 / MTok | $6.00 / MTok | +60% |
| Haiku 4.5 | $1.25 / MTok | $2.00 / MTok | +60% |

### Local-First & Privacy

- Reads Claude Code, Codex, and Cursor IDE logs already present on disk
- No cloud sync and no remote account required for usage history
- Works passively until local logs exist
- Optional rate-limit panels only use provider-authenticated state already available on the machine
- Cursor access token auto-detected from Cursor IDE's local storage (zero-config) or manually stored in OS keyring

### Performance

- Parsed-file reuse avoids reparsing unchanged logs
- In-memory caches keyed by provider, period, and offset (Arc<RwLock<>>, 2-min TTL)
- Stale-while-revalidate loading for fast repeat views
- Frontend payload cache eliminates IPC round-trips on tab switches
- Adjacent-window warming for quicker historical navigation
- Window height is restored from the previous session before the chart renders, so cold launches don't visibly grow after data arrives

## Platform Differences

| Feature | macOS | Windows | Linux |
|---------|-------|---------|-------|
| System tray icon | Menu bar | System tray | System tray |
| Cost display | `set_title()` text beside icon | Tooltip on hover | Tooltip on hover |
| Rate limits (Claude) | Statusline, OAuth/CLI fallback | Statusline, CLI fallback | Statusline, CLI fallback |
| Rate limits (Codex) | JSONL session files | JSONL session files | JSONL session files |
| Rate limits (Cursor) | API (auto-detected or manual token) | API (auto-detected or manual token) | API (manual token) |
| Glass blur effect | Vibrancy | Mica/Acrylic | Not available |
| Dock icon toggle | Supported | Not applicable | Not applicable |
| Autostart | LaunchAgent | Registry | XDG autostart |
| Auto-update | DMG in-place replace | NSIS passive install | AppImage replace (.deb: download link) |
| Installer | DMG (unsigned — see note below) | NSIS .exe | .deb / .AppImage |

## Local Data

TokenMonitor works from usage data you already have on disk. If no logs are present yet, the app stays idle until Claude Code or Codex generates them.

### Usage History

| Provider | Default path | Discovery behavior |
|---|---|---|
| Claude Code | `~/.claude/projects/**/*.jsonl` | Also checks `$CLAUDE_CONFIG_DIR/projects` when set |
| Codex CLI | `~/.codex/sessions/YYYY/MM/DD/*.jsonl` | Also respects `$CODEX_HOME/sessions` when set |
| Cursor IDE | Cursor workspace storage `state.vscdb` | Auto-detected from Cursor IDE's local data directory |
| Kimi Code | `~/.kimi-code/sessions/**/wire.jsonl`, `~/.kimi/sessions/**/wire.jsonl` | Also respects `$KIMI_DATA_DIR` (comma-separated) when set |

### Rate-Limit Data

Rate-limit visibility is separate from usage history parsing:

- Claude rate limits prefer fresh events from the optional TokenMonitor statusline installed into Claude Code; OAuth and CLI probes are fallbacks
- Codex rate limits are read from recent session metadata in local Codex JSONL files
- Cursor rate limits are fetched from the Cursor API using a configured Admin API key or locally detected authentication state
- Kimi rate limits are fetched from the Kimi usage API using the token set in `~/.kimi-code/credentials/kimi-code.json`. Kimi access tokens expire after 15 minutes, so when the stored one is expired (or rejected) TokenMonitor performs the same `refresh_token` grant the Kimi CLI does and writes the rotated tokens back to that file

## Documentation

- [Tutorial](docs/tutorial.md) — installation, onboarding, daily use, and troubleshooting
- [Development guide](docs/DEVELOPMENT.md) — repository layout, validation, and releases
- [Changelog](CHANGELOG.md) — release history
- [Updater test matrix](docs/testing/auto-update.md) — release-candidate smoke tests

## Installation

### Download

Grab the latest installer from the [Releases](https://github.com/Michael-OvO/TokenMonitor/releases/latest) page.

### Build From Source

**Prerequisites:**

- Node.js >= 18 and npm
- Rust toolchain (install via [rustup](https://rustup.rs/))
- Platform-specific Tauri dependencies:
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`)
  - **Windows**: Visual Studio C++ Build Tools, WebView2 (pre-installed on Windows 11)
  - **Linux**: `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`

```bash
git clone https://github.com/Michael-OvO/TokenMonitor.git
cd TokenMonitor
npm ci
npx tauri build
```

Platform-specific bundle output:

| Platform | Output |
|----------|--------|
| macOS | `src-tauri/target/release/bundle/dmg/TokenMonitor_x.y.z_aarch64.dmg` |
| Windows | `src-tauri/target/release/bundle/nsis/TokenMonitor_x.y.z_x64-setup.exe` |
| Linux | `src-tauri/target/release/bundle/deb/token-monitor_x.y.z_amd64.deb` |

> [!NOTE]
> **macOS:** All builds are **unsigned** (no Apple Developer ID certificate), so on
> first launch Gatekeeper may report the app as *"damaged and can't be opened"*.
> The file is fine — clear the quarantine flag once after moving it to
> Applications:
>
> ```bash
> xattr -cr /Applications/TokenMonitor.app
> ```
>
> In-app auto-updates are unaffected: they are verified with the Tauri updater
> (minisign) key, which is independent of Apple code signing.

### Development

```bash
npm ci
npx tauri dev          # full app: hot-reload frontend + debug Rust backend
npm run dev            # frontend only at http://localhost:1420 (no Rust)
```

### Testing

```bash
npm test               # vitest (frontend unit tests)
npm run test:rust      # cargo test (Rust backend tests)
npm run test:all       # both Rust and frontend tests sequentially
```

## Architecture

```mermaid
graph LR
    A["Claude logs<br/><sub>~/.claude/projects/**/*.jsonl</sub>"] --> B["Rust parser + pricing engine"]
    D["Codex logs<br/><sub>~/.codex/sessions/YYYY/MM/DD/*.jsonl</sub>"] --> B
    K["Cursor workspace<br/><sub>state.vscdb</sub>"] --> B
    M["Kimi Code logs<br/><sub>~/.kimi-code/sessions/**/wire.jsonl</sub>"] --> B
    S["SSH remote logs"] --> B
    B --> C["Tauri IPC layer"]
    C --> E["Svelte 5 desktop UI"]
    C --> F["System tray updater"]
    C --> H["FloatBall overlay"]
    C --> U["Auto-updater"]
    B --> G["In-memory query + file caches"]
    B --> AR["Usage archive<br/><sub>persistent hourly aggregates</sub>"]
```

### Project Structure

```text
src/
├── App.svelte                     # Main popover shell and view orchestration
├── float-ball.ts                  # FloatBall entry point
└── lib/
    ├── bootstrap.ts               # Startup wiring and runtime initialization
    ├── stores/
    │   ├── usage.ts               # Usage fetching, in-memory cache, period/provider state
    │   ├── rateLimits.ts          # Rate-limit fetching and persistence
    │   ├── settings.ts            # Theme, tray, currency, and local preferences
    │   └── updater.ts             # Auto-updater state, IPC wiring, event listeners
    ├── providerMetadata.ts        # Central usage/rate-limit provider metadata for the UI
    ├── components/                # Metrics, charts, calendar, footer, settings UI
    │   ├── Chart.svelte           # Bar/line/pie chart visualization
    │   ├── chartBuckets.ts        # Chart bucket computation helpers
    │   ├── Breakdown.svelte       # Per-model cost breakdown
    │   ├── Calendar.svelte        # Heatmap calendar view
    │   ├── DevicesView.svelte     # SSH remote device management
    │   ├── float-ball/            # Overlay component, interactions, and move queue
    │   ├── Footer.svelte          # Active session, burn rate
    │   ├── PermissionDisclosure.svelte # Privacy/permission surface display
    │   ├── PermissionsOnboarding.svelte # First-launch onboarding wizard
    │   ├── settings/              # Settings panel and focused subpanels
    │   ├── UpdateBanner.svelte    # In-app update banner
    │   └── UsageBars.svelte       # Rate limit utilization bars
    ├── permissions/
    │   ├── statusline.ts          # Claude Code statusline install/check
    │   └── surfaces.ts            # Permission surface definitions
    ├── tray/
    │   ├── sync.ts                # Frontend-to-native tray state syncing
    │   └── title.ts               # Tray title formatting
    ├── views/                     # View-model logic (footer, rate limits, devices)
    ├── window/
    │   ├── appearance.ts          # Native theme and visual effects
    │   ├── sizing.ts              # Window size calculations
    │   ├── resizeOrchestrator.ts  # Resize lifecycle
    │   └── uiStability.ts         # Resize stability and diagnostics
    └── utils/
        ├── platform.ts            # OS detection (macOS/Windows/Linux)
        ├── plans.ts               # Plan tier cost lookups
        ├── calendar.ts            # Calendar utilities
        ├── format.ts              # Number/currency formatting
        └── logger.ts              # Frontend logging via Rust file writer

src-tauri/src/
├── lib.rs                         # Tauri app setup, tray wiring, background refresh
├── commands.rs                    # IPC module registry
├── commands/                      # Usage, calendar, config, tray, SSH, overlay, and updater IPC
├── logging.rs                     # tracing + rolling file appender
├── models.rs                      # Shared backend payload types
├── paths.rs                       # Central registry of all filesystem paths read
├── statusline/                    # Claude statusline scripts, installation, and event parsing
├── usage/
│   ├── parser.rs                  # JSONL discovery, parsing, normalization
│   ├── claude_parser.rs           # Claude Code-specific deep parser
│   ├── pricing.rs                 # Model-family-aware token pricing
│   ├── integrations.rs            # Usage integration registry (Claude, Codex, Cursor, Kimi)
│   ├── archive.rs                 # Persistent hourly aggregate storage
│   ├── device_aggregation.rs      # Remote device data aggregation
│   ├── exchange_rates.rs          # Dynamic USD→EUR/GBP/JPY/CNY rates (24h TTL)
│   ├── litellm.rs                 # LiteLLM dynamic pricing (24h TTL)
│   ├── openrouter.rs              # OpenRouter dynamic pricing
│   ├── ssh_remote.rs              # SSH remote log sync
│   └── ssh_config.rs              # SSH host discovery
├── rate_limits/
│   ├── claude.rs                  # OAuth Keychain + API (macOS)
│   ├── codex_cli.rs               # Codex CLI probe fallback
│   ├── codex.rs                   # Session file parsing
│   ├── cursor.rs                  # Cursor API usage + spend limit
│   └── http.rs                    # Shared HTTP client
├── secrets/
│   ├── mod.rs                     # Secret persistence layer (keyring-first strategy)
│   └── cursor.rs                  # Cursor access token management
├── updater/
│   ├── mod.rs                     # Updater module entry
│   ├── state.rs                   # UpdaterState type + banner/notify predicates
│   ├── persistence.rs             # Persist updater state via tauri-plugin-store
│   └── scheduler.rs               # Check scheduler with exponential backoff
├── tray/
│   └── render.rs                  # Native tray icon + utilization bars (RGBA)
├── stats/
│   ├── change.rs                  # Change statistics
│   └── subagent.rs                # Subagent statistics
└── platform/
    ├── mod.rs                     # Cross-platform helpers
    ├── macos/                     # macOS window management
    ├── windows/
    │   └── window.rs              # Taskbar-aligned positioning
    └── linux/                     # Linux window management
```

### Runtime Flow

1. The UI requests a provider, period, and optional historical offset through Tauri IPC.
2. The Rust backend resolves one or more usage integrations, scans their JSONL logs, normalizes integration-specific events, and prices each entry locally.
3. Aggregated payloads are cached in memory for fast repeat requests.
4. The frontend renders metrics, charts, model summaries, calendar views, and footer state.
5. A background loop refreshes the tray title and emits update events on the configured interval.

### Tech Stack

| Layer | Technology |
|---|---|
| Desktop shell | [Tauri v2](https://v2.tauri.app/) |
| Frontend | [Svelte 5](https://svelte.dev/) + TypeScript |
| Backend | Rust |
| Build tool | [Vite 8](https://vite.dev/) |
| State path | Local JSONL parsing + Tauri IPC + Svelte stores |

## For Builders

<details>
<summary>Validation, benchmarks, and versioning</summary>

### Validation

```bash
npx svelte-check                # Svelte type checking
npm test                        # Vitest
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run test:rust               # Rust tests
```

Convenience command:

```bash
npm run test:all
```

### Manual Cache Benchmark

There is an ignored Rust benchmark test for the integrated caching paths:

```bash
cargo test benchmark_real_log_cache_paths --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture
```

### Versioning

Version must stay in sync across `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json`.

```bash
npm run release -- X.Y.Z    # bumps version files, commits, tags, pushes
```

Tag push triggers GitHub Actions release workflow which builds for all three platforms.

</details>

## Contributing

Issues and pull requests are welcome, especially around:

- UI polish and distinctive tray workflows
- Pricing-model accuracy
- Performance on large local histories
- Packaging and distribution
- New provider support
- Cross-platform compatibility

If you use Claude Code, Codex, or Cursor heavily, this repo is intended to be a practical local utility and a solid foundation for usage observability across macOS, Windows, and Linux.

## License

Licensed under the [GNU General Public License v3.0](LICENSE).
