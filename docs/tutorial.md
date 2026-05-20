# TokenMonitor Tutorial

A step-by-step guide to installing, configuring, and using TokenMonitor — a local-first cross-platform system tray app that monitors your Claude Code, Codex CLI, and Cursor IDE token spending.

---

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Installation](#2-installation)
3. [First Launch](#3-first-launch)
4. [Understanding the Interface](#4-understanding-the-interface)
5. [Provider Tabs](#5-provider-tabs)
6. [Period Views](#6-period-views)
7. [Charts & Model Breakdown](#7-charts--model-breakdown)
8. [Active Session & Burn Rate](#8-active-session--burn-rate)
9. [Rate Limits](#9-rate-limits)
10. [Calendar View](#10-calendar-view)
11. [Tray Display](#11-tray-display)
12. [FloatBall Overlay](#12-floatball-overlay)
13. [SSH Remote Devices](#13-ssh-remote-devices)
14. [Auto-Updater](#14-auto-updater)
15. [Settings](#15-settings)
16. [Troubleshooting](#16-troubleshooting)

---

## 1. Prerequisites

- **macOS 13+**, **Windows 10/11**, or **Linux** (with WebKitGTK support)
- **Claude Code**, **Codex CLI**, and/or **Cursor IDE** installed and used at least once (TokenMonitor reads the session logs these tools generate on disk)

No API keys, accounts, or cloud services are required. TokenMonitor is fully local.

## 2. Installation

### Option A: Download a pre-built installer (recommended)

Go to the [latest release page](https://github.com/Michael-OvO/TokenMonitor/releases/latest) and download the installer for your platform:

| Platform | File | How to install |
|----------|------|----------------|
| **macOS** | `.dmg` | Open the DMG, drag **TokenMonitor** into `Applications` |
| **Windows** | `.exe` (NSIS) | Run the installer, follow the prompts |
| **Linux** | `.deb` | `sudo dpkg -i token-monitor_*.deb` |

> **macOS first launch:** macOS may show a security prompt. Go to **System Settings > Privacy & Security** and click **Open Anyway**.

> **Windows first launch:** Windows SmartScreen may show a warning for unsigned builds. Click **More info > Run anyway**.

### Option B: Build from source

Make sure you have **Node.js >= 18**, **npm**, and a **Rust toolchain** ([rustup](https://rustup.rs/)).

Platform-specific dependencies:
- **macOS**: `xcode-select --install`
- **Windows**: Visual Studio C++ Build Tools, WebView2 (pre-installed on Windows 11)
- **Linux**: `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`

```bash
git clone https://github.com/Michael-OvO/TokenMonitor.git
cd TokenMonitor
npm install
npx tauri build
```

The built app will be in `src-tauri/target/release/bundle/`.

## 3. First Launch

When TokenMonitor starts, it appears as a small icon in your **system tray**:

- **macOS**: Menu bar (top-right, near the clock)
- **Windows**: System tray (bottom-right, near the clock)
- **Linux**: System tray area (varies by desktop environment)

There is no Dock/Taskbar window by default — TokenMonitor is a tray utility.

**Click the tray icon** to open the popover window.

If you have existing Claude Code or Codex session logs on your machine, TokenMonitor will immediately show your usage data. If not, the app stays idle until you start a coding session.

### Where does the data come from?

| Provider | Log location |
|----------|-------------|
| Claude Code | `~/.claude/projects/**/*.jsonl` |
| Codex CLI | `~/.codex/sessions/YYYY/MM/DD/*.jsonl` |
| Cursor IDE | Cursor workspace storage `state.vscdb` |

TokenMonitor reads these files directly — it never modifies or deletes them.

## 4. Understanding the Interface

The popover window has several layers, from top to bottom:

```
+----------------------------------+
|  [All] [Claude] [Codex] [Cursor]|  <- Provider tabs
+----------------------------------+
|  $12.45 total   42,391 tokens   |  <- Metrics row
+----------------------------------+
|  5h  day  week  month  year     |  <- Period selector
+----------------------------------+
|  ████████████████████████        |  <- Cost chart (bar or line)
|  ██████████████                  |
|  ████████                        |
+----------------------------------+
|  Sonnet 4.6    $8.23  (66%)     |  <- Model breakdown
|  Haiku 4.5     $3.12  (25%)     |
|  Opus 4.6      $1.10  ( 9%)     |
+----------------------------------+
|  Active: $2.31/hr · ~$4.80 proj |  <- Footer: active session info
+----------------------------------+
```

## 5. Provider Tabs

The top row shows your data sources:

- **All** — Combined view across all providers
- **Claude** — Claude Code usage only
- **Codex** — Codex CLI usage only
- **Cursor** — Cursor IDE usage only

Click a tab to switch. The chart, breakdown, and metrics all update to reflect the selected provider.

You can customize which tabs appear (show/hide, rename) in **Settings > Header Tabs**.

## 6. Period Views

Below the metrics row, select a time period:

| Period | What it shows |
|--------|--------------|
| **5h** | Current 5-hour billing window — cost, burn rate, and projected spend |
| **day** | Today's usage. Use < > arrows to browse previous days |
| **week** | This week's daily breakdown. Navigate to past weeks |
| **month** | This month's daily breakdown. Navigate to past months |
| **year** | Monthly breakdown across the current year |

The **< > arrows** next to the period let you navigate backwards and forwards through history. For example, in "day" view, clicking < shows yesterday's data.

## 7. Charts & Model Breakdown

### Chart

The chart shows cost distribution over the selected period. Each bar is color-coded by model.

- **Bar mode**: Stacked bars per time bucket (default)
- **Line mode**: Cost trend over time
- **Pie mode**: Model-share breakdown as a donut chart

Toggle between modes by clicking the chart mode icon.

**Hover a bar** to see a detailed tooltip with per-model costs for that time bucket.

### Model Breakdown

Below the chart, each model you've used is listed with:

- **Display name** (e.g., "Sonnet 4.6", "Haiku 4.5")
- **Total cost** for the selected period
- **Token count**

Models are sorted by cost (highest first). You can hide specific models from the chart in **Settings > Hidden Models**.

## 8. Active Session & Burn Rate

The **footer** at the bottom of the popover shows live session information:

- **Active block cost** — How much you've spent in the current 5-hour billing window
- **Burn rate** — Estimated cost per hour based on recent activity
- **Projected cost** — Where you'll end up if the current pace continues to the end of the 5-hour window

This updates automatically based on your configured refresh interval (default: every 30 seconds).

## 9. Rate Limits

When rate limit data is available, TokenMonitor shows utilization bars for each provider:

- **Claude**:
  - **macOS**: Reads the OAuth authentication state already on your machine via the Anthropic API
  - **Windows / Linux**: Uses CLI probe to query rate limit status
- **Codex**: Reads rate limit metadata from local Codex session files (all platforms)
- **Cursor**: Fetches plan usage and spend limit data from the Cursor API. The access token is auto-detected from Cursor IDE's local storage (zero-config on macOS/Windows) or can be manually provided in Settings.

Rate limit panels show:
- Current utilization percentage
- Time until the rate limit window resets
- Cooldown state if you've been throttled
- Cursor plan usage (auto-mode %) and spend limit tracking

> Rate limits are optional — if the data isn't available, the panels simply don't appear. You can opt in/out of rate limit tracking from the first-launch welcome card or from Settings.

## 10. Calendar View

In **month** view, click the **calendar icon** to switch to a heatmap calendar:

- Each day is colored by spend intensity
- Darker = more expensive day
- Click any day to jump to that day's detailed view

This gives you a quick visual overview of your spending patterns across the month.

## 11. Tray Display

The tray icon can optionally show spend information:

- **macOS**: A cost label appears next to the menu bar icon (e.g., `$12.45`)
- **Windows / Linux**: The cost is shown in the tooltip on hover

This gives you a glanceable spend number without opening the popover.

Configure what the tray displays in **Settings > Tray Config**:

| Setting | Options |
|---------|---------|
| **Show cost** | On/Off — display the dollar amount |
| **Cost precision** | `full` ($12.45) or `whole` ($12) |
| **Rate limit bars** | `off`, `single` (one provider), or `both` |
| **Percentages** | Show/hide utilization percentages |

**Right-click** the tray icon to access the **Quit** option.

## 12. FloatBall Overlay

TokenMonitor includes an optional **FloatBall** — a small, always-on-top draggable overlay that shows your live spend without needing to open the main popover.

- **Enable/disable** from Settings
- **Drag** it anywhere on your screen
- Works independently from the main window (separate Vite entry point)

This is useful when you want constant visibility of your spend while working in other applications.

## 13. SSH Remote Devices

TokenMonitor can fetch usage logs from **remote machines via SSH**, giving you a unified view of usage across multiple devices.

### Setup

1. Open the **Devices** view from the main interface
2. TokenMonitor auto-discovers hosts from your `~/.ssh/config` file
3. Select a host and TokenMonitor will sync its Claude Code / Codex logs over SSH
4. Remote usage data appears alongside your local data

### How it works

- `ssh_config.rs` discovers available SSH hosts
- `ssh_remote.rs` manages per-host sync state and caches fetched logs
- Remote logs are merged into the same parsing and pricing pipeline as local logs
- Each host's sync state is persisted in settings so you don't re-sync unnecessarily

> You need SSH key-based authentication configured for the remote hosts. Password-based auth is not supported in the background sync flow.

## 14. Auto-Updater

TokenMonitor includes a built-in auto-updater that checks for new versions and offers in-app updates.

### How it works

- After launch, the app checks for updates (initial check after ~10 seconds, then every 6 hours)
- When a new version is available, an **update banner** appears at the top of the popover
- The **tray icon** shows a small **red badge dot** in the top-right corner
- An **OS notification** fires once per new version (deduped across checks)

### Update actions

| Action | Behavior |
|--------|----------|
| **Update Now** | Downloads and installs the update. On macOS/Linux AppImage, the app relaunches automatically. On Windows, the NSIS installer runs in passive mode. |
| **Skip** | Hides the banner for this specific version. The next release will re-trigger. |
| **Later** | Dismisses the banner for the current session only. |

### Platform behavior

- **macOS**: `.app.tar.gz` bundle is downloaded, verified against the signing key, and replaced in-place
- **Windows**: `.nsis.zip` bundle triggers the NSIS installer in passive mode
- **Linux (AppImage)**: `.AppImage` is downloaded, verified, and replaced
- **Linux (.deb)**: The banner shows a "Download" link that opens the GitHub release page in your browser (apt owns `.deb` installations)

### Settings

You can manage auto-update behavior in **Settings > Updates**:
- Enable/disable automatic checks
- View last check time and any errors
- Manage skipped versions

## 15. Settings

Click the **gear icon** in the popover to open the settings panel:

### Appearance
- **Theme** — Light, Dark, or System (follows OS appearance)
- **Glass effect** — Enable/disable the macOS vibrancy blur behind the popover (macOS only; hidden on Windows/Linux)
- **Brand theming** — Color the header and accents based on the selected provider

### Behavior
- **Default provider** — Which tab opens first (Claude, Codex, Cursor, or All)
- **Default period** — Starting time period (5h, day, week, month)
- **Refresh interval** — How often data auto-refreshes: 30s, 60s, 5min, or off
- **Launch at login** — Automatically start TokenMonitor when you log in
- **Show Dock icon** — Show/hide the Dock icon (macOS only, hidden by default)
- **Rate limits** — Enable/disable rate limit tracking (opt-in from welcome card or here)

### Data
- **Currency** — Display costs in USD, EUR, GBP, JPY, or CNY

### Visibility
Grouped into a single card, each row is collapsible:
- **Provider** — Show/hide and rename the provider header tabs (Claude / Codex / Cursor / All)
- **Model Visibility** — Exclude individual models from charts and breakdowns
- **SSH Hosts** — Enable/disable remote devices and trigger a one-click sync of all enabled hosts (see [SSH Remote Devices](#13-ssh-remote-devices))

### Tray Config
- Cost display, precision, rate limit bars, and percentage format (see [Tray Display](#11-tray-display))

### Updates
- Enable/disable automatic update checks
- View current version and last check time
- Manage skipped versions (see [Auto-Updater](#14-auto-updater))

### Privacy & Permissions
- View all filesystem paths and credential surfaces the app accesses
- Each surface shows what data is read, why, and the access policy

## 16. Troubleshooting

### "No data" after installing

TokenMonitor reads session logs that Claude Code, Codex, and Cursor IDE write to disk. If you see no data:

1. **Verify logs exist**:
   - Claude Code: Check that `~/.claude/projects/` contains `.jsonl` files
   - Codex: Check `~/.codex/sessions/`
   - Cursor: Check that Cursor IDE has been used (workspace storage is auto-detected)
   - On Windows, `~` maps to `C:\Users\<username>`
2. **Run a session**: Open Claude Code, Codex, or Cursor and have at least one conversation. Logs are written during usage.
3. **Check the date**: Make sure you're looking at the correct period. Use the < arrow to browse previous days.

### Popover doesn't appear

- **macOS**: Click the TokenMonitor icon in the menu bar (top-right, near the clock). If the icon isn't visible, check **System Settings > Control Center > Menu Bar Only** to ensure it's not hidden.
- **Windows**: Look for the icon in the system tray (bottom-right). It may be in the overflow area — click the `^` arrow to expand.
- **Linux**: Ensure your desktop environment supports the system tray (AppIndicator). Some minimal window managers may not show tray icons by default.
- Try quitting and relaunching the app.

### Cost numbers look wrong

TokenMonitor uses built-in pricing tables. If you notice discrepancies:

- Check which models you're using — pricing varies significantly between models
- Claude cache-write pricing has two tiers (5-minute and 1-hour) that affect totals
- Use the `day` view to isolate and verify individual day costs

### High memory or CPU usage

- Reduce the refresh interval in Settings (e.g., from 30s to 5min)
- The first load after a long idle period may take a moment while the parser scans recent logs. Subsequent loads use cached results.

### Rate limits not showing

Rate limit data requires:
- **Claude on macOS**: An active Claude authentication on your machine (the same one Claude Code uses)
- **Claude on Windows/Linux**: Claude CLI must be available for probe-based rate limit checking
- **Codex (all platforms)**: Recent Codex session files with rate limit metadata
- **Cursor (all platforms)**: An access token — auto-detected from Cursor IDE on macOS/Windows, or manually entered in Settings

If none are available, the rate limit panels are simply hidden. Rate limits must be enabled (opt-in from the first-launch welcome card or Settings).

### Windows-specific issues

- **WebView2 missing**: Windows 10 may not have WebView2 pre-installed. Download it from [Microsoft](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).
- **Tray icon not visible**: Check the system tray overflow area. You can pin the icon by dragging it out of the overflow.

### Linux-specific issues

- **Tray icon not showing**: Install `libappindicator3-1` if not already present. Some DEs (like GNOME) need an extension like [AppIndicator Support](https://extensions.gnome.org/extension/615/appindicator-support/).
- **Blank window**: Ensure WebKitGTK 4.1 is installed: `sudo apt install libwebkit2gtk-4.1-0`.

---

## Tips

- **Quick glance**: Keep the tray cost display enabled for at-a-glance spend monitoring
- **Navigate history**: Use < > arrows heavily — understanding past patterns helps budget future usage
- **Model awareness**: Check the model breakdown regularly. Switching to a cheaper model for simple tasks can significantly reduce costs
- **5h window**: The 5-hour view maps to Claude's billing windows. Use it to pace yourself within rate limit cycles
- **FloatBall**: Enable FloatBall when you want persistent cost visibility while coding in other windows
- **Remote devices**: If you use Claude Code on multiple machines, SSH sync lets you see total spend in one place
- **Cursor spend limits**: If you use Cursor with a spend limit, TokenMonitor shows both plan usage and spend limit utilization
- **Auto-update**: Keep auto-update enabled to get the latest pricing tables and bug fixes automatically
- **Pie chart**: Use pie chart mode for a quick visual breakdown of model share within a period
