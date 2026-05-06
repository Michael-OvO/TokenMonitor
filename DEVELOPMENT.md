# TokenMonitor — Development Guide

## Prerequisites

- **Node.js** >= 18 (frontend tooling only — not required to *run* the app)
- **Rust** toolchain via [rustup](https://rustup.rs/) (for Tauri backend)
- Platform-specific Tauri dependencies:
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`)
  - **Windows**: Visual Studio C++ Build Tools, WebView2 (pre-installed on Windows 11)
  - **Linux**: `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`

## Quick Start

```bash
# Install frontend dependencies
npm install

# Run in development mode (hot-reload frontend + debug Rust backend)
npx tauri dev
```

The app appears as a **system tray icon**:
- **macOS**: Menu bar (top-right, near the clock) — no dock icon by default
- **Windows**: System tray (bottom-right)
- **Linux**: System tray area (varies by DE)

Click it to open the popover.

## Project Structure

```
TokenMonitor/
├── src/                          # Svelte 5 frontend (WebView)
│   ├── App.svelte                # Root component — layout orchestration
│   ├── app.css                   # Global styles, CSS variables, keyframes
│   ├── main.ts                   # Svelte mount point
│   ├── float-ball.ts             # FloatBall entry point (separate Vite entry)
│   └── lib/
│       ├── bootstrap.ts          # Startup wiring and runtime initialization
│       ├── components/
│       │   ├── Toggle.svelte          # Provider tab switch
│       │   ├── TimeTabs.svelte        # 5H | Day | Week | Month | Year tabs
│       │   ├── MetricsRow.svelte      # Cost / Tokens / Sessions cards
│       │   ├── Chart.svelte           # Stacked bar / line / pie chart
│       │   ├── chartBuckets.ts        # Chart bucket computation helpers
│       │   ├── Breakdown.svelte       # Per-model cost breakdown
│       │   ├── UsageBars.svelte       # Horizontal progress bars (5H view)
│       │   ├── ModelList.svelte       # Per-model cost/token rows
│       │   ├── SubagentList.svelte    # Agent/subagent cost breakdown
│       │   ├── Footer.svelte          # Live session indicator, timestamps
│       │   ├── DateNav.svelte         # Calendar navigation (day/week/month offset)
│       │   ├── Calendar.svelte        # Date picker / heatmap
│       │   ├── Settings.svelte        # Settings panel (delegates to sub-panels)
│       │   ├── HeaderTabsSettings.svelte  # Tab configuration
│       │   ├── HiddenModelsSettings.svelte # Model filtering
│       │   ├── ThemeSettings.svelte   # Theme configuration
│       │   ├── TrayConfigSettings.svelte  # Tray display options
│       │   ├── SshHostsSettings.svelte    # SSH device management
│       │   ├── SplashScreen.svelte    # Initial loading screen
│       │   ├── SetupScreen.svelte     # Empty-state screen (no data found)
│       │   ├── WelcomeCard.svelte     # First-launch onboarding with opt-in toggles
│       │   ├── UpdateBanner.svelte    # In-app update notification banner
│       │   ├── PermissionDisclosure.svelte # Privacy/permission surface display
│       │   ├── DevicesView.svelte     # SSH remote device list + stats
│       │   ├── SingleDeviceView.svelte # Single device usage detail
│       │   ├── FloatBall.svelte       # Always-on-top overlay component
│       │   ├── floatBallInteraction.ts # FloatBall drag/scale detection
│       │   ├── floatBallUtils.ts      # FloatBall formatting and constants
│       │   ├── SegmentedControl.svelte # Reusable segmented control
│       │   └── ToggleSwitch.svelte    # Reusable toggle
│       ├── stores/
│       │   ├── usage.ts          # Svelte stores + IPC fetch logic
│       │   ├── rateLimits.ts     # Rate limit store + fetch logic
│       │   ├── settings.ts       # Settings store + persistence
│       │   └── updater.ts        # Auto-updater state, IPC, events
│       ├── providerMetadata.ts   # Frontend provider metadata + tab ordering
│       ├── permissions/
│       │   ├── keychain.ts       # macOS Keychain one-time prompt flow
│       │   └── surfaces.ts       # Permission surface definitions
│       ├── tray/
│       │   ├── sync.ts           # Frontend-to-native tray state syncing
│       │   └── title.ts          # Tray title formatting
│       ├── views/
│       │   ├── footer.ts         # Footer view-model logic
│       │   ├── rateLimits.ts     # Rate limit view-model
│       │   ├── rateLimitMonitor.ts # Rate limit monitoring
│       │   └── deviceStats.ts    # Device data aggregation
│       ├── window/
│       │   └── appearance.ts     # Window surface syncing
│       ├── windowSizing.ts       # Window size management
│       ├── resizeOrchestrator.ts # Window resize orchestration
│       ├── uiStability.ts        # UI stability utilities
│       ├── types/
│       │   └── index.ts          # TypeScript interfaces (mirrors Rust structs)
│       └── utils/
│           ├── platform.ts       # OS detection (macOS/Windows/Linux)
│           ├── plans.ts          # Plan tier cost lookups
│           ├── format.ts         # Cost/token/time formatting + model colors
│           ├── calendar.ts       # Calendar utilities
│           └── logger.ts         # Frontend logging via Rust file writer
├── src-tauri/                    # Rust backend (Tauri)
│   ├── Cargo.toml
│   ├── tauri.conf.json           # Window, bundle, updater, and app config
│   ├── Info.plist                # LSUIElement (no dock icon)
│   ├── capabilities/default.json # Permission grants
│   ├── icons/                    # Tray + app icons
│   └── src/
│       ├── main.rs               # Entry point
│       ├── lib.rs                # Tauri setup, tray icon, background polling
│       ├── commands.rs           # IPC dispatch hub
│       ├── commands/
│       │   ├── usage_query.rs    # Data fetching
│       │   ├── calendar.rs       # Heatmap queries
│       │   ├── period.rs         # Time range selection
│       │   ├── config.rs         # Settings sync
│       │   ├── tray.rs           # Title/utilization rendering
│       │   ├── ssh.rs            # Remote device management
│       │   ├── float_ball/       # Overlay state + layout engine
│       │   │   ├── mod.rs
│       │   │   └── layout.rs
│       │   ├── updater.rs        # Auto-update IPC commands
│       │   └── logging.rs        # Log-level control
│       ├── logging.rs            # tracing + rolling file appender
│       ├── models.rs             # Serde structs for frontend payload
│       ├── paths.rs              # Central filesystem path registry
│       ├── usage/
│       │   ├── mod.rs
│       │   ├── parser.rs         # JSONL discovery, parsing, normalization
│       │   ├── claude_parser.rs  # Claude Code-specific deep parser
│       │   ├── pricing.rs        # Model-family-aware token pricing
│       │   ├── integrations.rs   # Integration registry (Claude, Codex, Cursor)
│       │   ├── archive.rs        # Persistent hourly aggregate storage
│       │   ├── device_aggregation.rs # Remote device data aggregation
│       │   ├── exchange_rates.rs # Dynamic USD→multi-currency rates (24h TTL)
│       │   ├── litellm.rs        # LiteLLM dynamic pricing (24h TTL)
│       │   ├── openrouter.rs     # OpenRouter dynamic pricing
│       │   ├── ssh_remote.rs     # SSH remote log sync
│       │   └── ssh_config.rs     # SSH host discovery
│       ├── rate_limits/
│       │   ├── mod.rs
│       │   ├── claude.rs         # OAuth Keychain + API (macOS)
│       │   ├── claude_cli.rs     # CLI probe fallback (all platforms)
│       │   ├── codex.rs          # Session file parsing
│       │   ├── cursor.rs         # Cursor API plan usage + spend limit
│       │   └── http.rs           # Shared HTTP client
│       ├── secrets/
│       │   ├── mod.rs            # Secret persistence (keyring-first strategy)
│       │   └── cursor.rs         # Cursor access token management
│       ├── updater/
│       │   ├── mod.rs
│       │   ├── state.rs          # UpdaterState type + predicates
│       │   ├── persistence.rs    # Persist via tauri-plugin-store
│       │   └── scheduler.rs      # Check scheduler with exponential backoff
│       ├── tray/
│       │   ├── mod.rs
│       │   └── render.rs         # Native tray icon + utilization bars (RGBA)
│       ├── stats/
│       │   ├── mod.rs
│       │   ├── change.rs         # Change statistics
│       │   └── subagent.rs       # Subagent statistics
│       └── platform/
│           ├── mod.rs            # Cross-platform helpers
│           ├── macos/            # macOS window management
│           ├── windows/
│           │   ├── taskbar.rs    # GDI taskbar panel
│           │   └── window.rs     # Taskbar-aligned positioning
│           └── linux/            # Linux window management
├── float-ball.html               # FloatBall HTML entry (separate Vite entry)
├── build/                        # Modular build system
│   ├── index.mjs                 # Build entry point
│   ├── lib/                      # CLI, platform, workflow helpers
│   └── config/                   # Platform-specific Tauri configs
├── scripts/                      # Release and setup scripts
├── docs/                         # Design notes, tutorial, test matrices
├── package.json
├── vite.config.ts
├── vitest.config.ts
└── svelte.config.js
```

## Development Workflow

### Running

```bash
# Full app (frontend + backend)
npx tauri dev

# Frontend only (for CSS/layout iteration without Rust rebuild)
npm run dev
# Then open http://localhost:1420 in a browser
```

### Rebuilding

Tauri dev mode hot-reloads the frontend automatically. Rust changes require a recompile (~2-4s incremental).

If the port is already in use from a previous run:

```bash
# Kill stale processes
pkill -f "token-monitor"; lsof -ti:1420 | xargs kill -9
```

### Checking Rust compilation without running

```bash
cd src-tauri && cargo check
```

### Building for production

```bash
npx tauri build
```

Platform-specific outputs:
- **macOS**: `src-tauri/target/release/bundle/dmg/TokenMonitor_x.y.z_aarch64.dmg`
- **Windows**: `src-tauri/target/release/bundle/nsis/TokenMonitor_x.y.z_x64-setup.exe`
- **Linux**: `src-tauri/target/release/bundle/deb/token-monitor_x.y.z_amd64.deb` + `.AppImage`

### Testing

```bash
npm test               # vitest (frontend unit tests)
npm run test:watch     # vitest in watch mode
npm run test:rust      # cargo test (Rust backend tests)
npm run test:all       # both Rust and frontend tests sequentially
```

Run a single frontend test file:
```bash
npx vitest run src/lib/stores/usage.test.ts
```

Run a single Rust test:
```bash
cd src-tauri && cargo test test_name
```

### CI checks

```bash
npx svelte-check                # Svelte type checking
npm test                        # Vitest
cd src-tauri && cargo fmt --check       # Rust format
cd src-tauri && cargo clippy -- -D warnings  # Rust lints
cd src-tauri && cargo test      # Rust tests
```

A pre-commit hook runs all CI checks before each commit.

## Data Flow

```
~/.claude/projects/**/*.jsonl        (Claude Code integration)
~/.codex/sessions/YYYY/MM/DD/*.jsonl (Codex CLI integration)
Cursor workspace state.vscdb         (Cursor IDE integration)
    ↓ native Rust file I/O
integrations.rs + parser.rs + claude_parser.rs + pricing.rs
    (integration selection, JSONL parsing, token aggregation, cost calculation)
    ↓ archive.rs (persist completed hours)
    ↓ IPC invoke
Svelte frontend (stores/usage.ts → components)
```

No external processes, no Node.js at runtime. Network calls are limited to:
- Dynamic pricing (LiteLLM, OpenRouter) — optional, 24h cached
- Exchange rates (Frankfurter API) — optional, 24h cached
- Cursor rate limits (Cursor API) — when enabled
- Auto-updater (GitHub releases) — configurable

### In-Memory Cache

`Arc<RwLock<HashMap<String, (UsagePayload, Instant)>>>` with a 2-minute TTL.
Reading local JSONL files takes milliseconds, so no disk cache layer is needed for live data.
Background polling refreshes every 120 seconds and emits a `data-updated` event.

### Usage Archive

Completed hours are archived to persistent per-month JSONL files under `{app_data_dir}/usage-archive/`. Uses time-boundary partitioning: archive covers `[0..frontier]`, live source covers `(frontier..now]`. This prevents data loss when source JSONL files are deleted.

### Parser: Period → Method Dispatch

| Frontend period | Parser method | `since` value |
|----------------|---------------|---------------|
| `5h` | `get_blocks` | Today's date |
| `day` | `get_hourly` | Today's date |
| `week` | `get_daily` | Monday of current week |
| `month` | `get_daily` | 1st of current month |
| `year` | `get_monthly` | January 1st of current year |

### Pricing

`pricing.rs` contains a hardcoded pricing table for Anthropic, OpenAI, and Cursor-family models,
matched by pattern (most-specific first). Known families fall back within-family; unsupported
families can be resolved via dynamic pricing from LiteLLM (`litellm.rs`) and OpenRouter
(`openrouter.rs`) APIs with 24h TTL caching. Pricing version is stamped as `PRICING_VERSION`
for debugging.

### Data Sources

| Provider | Log location | Key field |
|----------|-------------|-----------|
| Claude | `~/.claude/projects/**/*.jsonl` | `type == "assistant"` entries |
| Codex | `~/.codex/sessions/YYYY/MM/DD/*.jsonl` | Final `token_count` event per session file |
| Cursor | Workspace storage `state.vscdb` | Usage records from Cursor IDE |

## Troubleshooting

**Two tray icons appearing** — Kill all processes and restart: `pkill -f "token-monitor"`

**Blank popover / no data** — Check that Claude Code, Codex CLI, or Cursor IDE have been used at least once:
```bash
ls ~/.claude/projects/
ls ~/.codex/sessions/
```

**Stale data** — The in-memory cache expires automatically every 120s. To force a refresh,
use the refresh button in the app or restart it.

**Rust compile errors** — Ensure your Rust toolchain is up to date:
```bash
rustup update
```
