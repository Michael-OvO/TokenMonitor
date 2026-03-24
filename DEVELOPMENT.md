# TokenMonitor — Development Guide

## Prerequisites

- **Node.js** ≥ 18 (frontend tooling only — not required to *run* the app)
- **Rust** toolchain via [rustup](https://rustup.rs/) (for Tauri backend)
- **Xcode Command Line Tools** — `xcode-select --install`

## Quick Start

```bash
# Install frontend dependencies
npm install

# Run in development mode (hot-reload frontend + debug Rust backend)
npx tauri dev
```

The app appears as a **menu bar icon** (no dock icon). Click it to open the popover.

## Project Structure

```
TokenMonitor/
├── src/                          # Svelte frontend (WebView)
│   ├── App.svelte                # Root component — layout orchestration
│   ├── app.css                   # Global styles, CSS variables, keyframes
│   ├── main.ts                   # Svelte mount point
│   └── lib/
│       ├── components/
│       │   ├── Toggle.svelte          # Claude/Codex provider switch
│       │   ├── TimeTabs.svelte        # 5H | Day | Week | Month tabs
│       │   ├── MetricsRow.svelte      # Cost / Tokens / Sessions cards
│       │   ├── Chart.svelte           # Stacked bar chart + inline detail panel
│       │   ├── UsageBars.svelte       # Horizontal progress bars (5H view)
│       │   ├── ModelList.svelte       # Per-model cost/token rows
│       │   ├── Footer.svelte          # Live session indicator, timestamps
│       │   ├── DateNav.svelte         # Calendar navigation (day/week/month offset)
│       │   ├── Calendar.svelte        # Date picker
│       │   ├── Settings.svelte        # Settings panel
│       │   ├── SplashScreen.svelte    # Initial loading screen
│       │   ├── SetupScreen.svelte     # Empty-state screen (no data found)
│       │   ├── SegmentedControl.svelte# Reusable segmented control
│       │   ├── ToggleSwitch.svelte    # Reusable toggle
│       │   └── ResizeDebugOverlay.svelte # Resize debug overlay (dev only)
│       ├── stores/
│       │   ├── usage.ts          # Svelte stores + IPC fetch logic
│       │   ├── rateLimits.ts     # Rate limit store + fetch logic
│       │   └── settings.ts       # Settings store + persistence
│       ├── providerMetadata.ts   # Frontend provider metadata + tab ordering
│       ├── types/
│       │   └── index.ts          # TypeScript interfaces (mirrors Rust structs)
│       └── utils/
│           └── format.ts         # Cost/token/time formatting + model colors
├── src-tauri/                    # Rust backend (Tauri)
│   ├── Cargo.toml
│   ├── tauri.conf.json           # Window, bundle, and app config
│   ├── Info.plist                # LSUIElement (no dock icon)
│   ├── capabilities/default.json # Permission grants
│   ├── icons/                    # Tray + app icons
│   └── src/
│       ├── main.rs               # Entry point
│       ├── lib.rs                # Tauri setup, tray icon, background polling
│       ├── integrations.rs       # Usage integration IDs, selection, and root discovery
│       ├── parser.rs             # Integration-driven JSONL reader + aggregations
│       ├── pricing.rs            # Pricing table + model-family-aware fallback logic
│       ├── commands.rs           # IPC handlers + data transformation
│       ├── rate_limits.rs        # Rate limit fetching + caching
│       └── models.rs             # Serde structs for frontend payload
├── package.json
├── vite.config.ts
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

Output: `src-tauri/target/release/bundle/dmg/TokenMonitor_0.1.0_aarch64.dmg`

## Data Flow

```
~/.claude/projects/**/*.jsonl        (Claude Code integration)
~/.codex/sessions/YYYY/MM/DD/*.jsonl (Codex CLI integration)
    ↓ native Rust file I/O
integrations.rs + parser.rs + pricing.rs
    (integration selection, JSONL parsing, token aggregation, cost calculation)
    ↓ IPC invoke
Svelte frontend (stores/usage.ts → components)
```

No external processes, no Node.js at runtime, no network calls for usage data.

### In-Memory Cache

A single `Mutex<HashMap<String, (UsagePayload, Instant)>>` with a 2-minute TTL.
Reading local JSONL files takes milliseconds, so no disk cache layer is needed.
Background polling refreshes every 120 seconds and emits a `data-updated` event.

### Parser: Period → Method Dispatch

| Frontend period | Parser method | `since` value |
|----------------|---------------|---------------|
| `5h` | `get_blocks` | Today's date |
| `day` | `get_hourly` | Today's date |
| `week` | `get_daily` | Monday of current week |
| `month` | `get_daily` | 1st of current month |

### Pricing

`pricing.rs` contains a hardcoded pricing table for Anthropic and OpenAI-family models,
matched by pattern (most-specific first). Known families fall back within-family; unsupported
families currently price at zero until explicit rates are added. Pricing version is stamped as
`PRICING_VERSION` for debugging.

### Data Sources

| Provider | Log location | Key field |
|----------|-------------|-----------|
| Claude | `~/.claude/projects/**/*.jsonl` | `type == "assistant"` entries |
| Codex | `~/.codex/sessions/YYYY/MM/DD/*.jsonl` | Final `token_count` event per session file |

## Troubleshooting

**Two tray icons appearing** — Kill all processes and restart: `pkill -f "token-monitor"`

**Blank popover / no data** — Check that Claude Code or Codex CLI have been used at least once:
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
