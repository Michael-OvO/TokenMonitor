import type { Settings } from "../stores/settings.js";

export type PermissionSurfaceId =
  | "usage_logs"
  | "claude_statusline"
  | "login_item"
  | "ssh_config"
  | "updates";

export type PermissionTone = "ok" | "warn" | "neutral";

export type PermissionRequestPolicy =
  | "after_first_run_disclosure"
  | "explicit_install"
  | "explicit_toggle"
  | "configured_only"
  | "never_requests_os_prompt";

export interface PermissionSurface {
  id: PermissionSurfaceId;
  title: string;
  status: string;
  tone: PermissionTone;
  why: string;
  requestPolicy: PermissionRequestPolicy;
  requestCopy: string;
  paths: string[];
}

export interface PermissionPlatform {
  macos: boolean;
}

type PermissionSettings = Pick<
  Settings,
  "hasSeenWelcome" | "rateLimitsEnabled" | "statuslineInstalled" | "launchAtLogin" | "sshHosts"
>;

export function getPermissionSurfaces(
  settings: PermissionSettings,
  _platform: PermissionPlatform,
): PermissionSurface[] {
  return [
    {
      id: "usage_logs",
      title: "Session logs",
      status: settings.hasSeenWelcome ? "Enabled" : "After welcome",
      tone: settings.hasSeenWelcome ? "ok" : "neutral",
      why:
        "Reads Claude Code and Codex session logs to calculate tokens, cost, models, and local activity.",
      requestPolicy: "after_first_run_disclosure",
      requestCopy:
        "Reads files inside your home directory only — TokenMonitor never opens an OS-level permission prompt for these paths.",
      paths: [
        "~/.claude/projects",
        "~/.config/claude/projects",
        "~/.codex/sessions",
      ],
    },
    {
      id: "claude_statusline",
      title: "Claude statusline",
      status: settings.statuslineInstalled
        ? "Installed"
        : settings.rateLimitsEnabled
        ? "Pending install"
        : "Off",
      tone: settings.statuslineInstalled
        ? "ok"
        : settings.rateLimitsEnabled
        ? "warn"
        : "neutral",
      why:
        "Lets TokenMonitor see which Claude Code session is active so it can show live 5h and weekly utilization. The script is plain shell, runs as you, and writes a single JSON line per prompt.",
      requestPolicy: "explicit_install",
      requestCopy:
        "Click \"Install\" to write a small script under TokenMonitor's data directory and add a `statusLine` entry to ~/.claude/settings.json. No Keychain prompt, no network request.",
      paths: [
        "~/.claude/settings.json (statusLine entry)",
        "<app-data>/statusline/events.jsonl",
      ],
    },
    {
      id: "login_item",
      title: "Launch at login",
      status: settings.launchAtLogin ? "Enabled" : "Off",
      tone: settings.launchAtLogin ? "ok" : "neutral",
      why:
        "Registers TokenMonitor as a login item only when you enable Launch at Login.",
      requestPolicy: "explicit_toggle",
      requestCopy:
        "TokenMonitor changes this only from the welcome card or Settings toggle.",
      paths: [],
    },
    {
      id: "ssh_config",
      title: "SSH remote devices",
      status: settings.sshHosts.some((host) => host.enabled) ? "Configured" : "Off",
      tone: settings.sshHosts.some((host) => host.enabled) ? "ok" : "neutral",
      why:
        "Reads SSH host config only when remote devices are configured, so remote usage can be shown separately.",
      requestPolicy: "configured_only",
      requestCopy:
        "No SSH config is read for remote devices until hosts are configured.",
      paths: ["~/.ssh/config", "Include files referenced by ~/.ssh/config"],
    },
    {
      id: "updates",
      title: "Updates",
      status: "Banner only",
      tone: "ok",
      why:
        "Checks for releases and shows update availability inside the app.",
      requestPolicy: "never_requests_os_prompt",
      requestCopy:
        "The app does not request notification permission for update checks.",
      paths: [],
    },
  ];
}

export function permissionSurfaceById(
  surfaces: PermissionSurface[],
  id: PermissionSurfaceId,
): PermissionSurface | undefined {
  return surfaces.find((surface) => surface.id === id);
}
