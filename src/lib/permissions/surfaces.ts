import type { Settings } from "../stores/settings.js";

export type PermissionSurfaceId =
  | "usage_logs"
  | "claude_credentials"
  | "keychain_fallback"
  | "login_item"
  | "ssh_config"
  | "updates";

export type PermissionTone = "ok" | "warn" | "neutral";

export type PermissionRequestPolicy =
  | "after_first_run_disclosure"
  | "silent_when_enabled"
  | "explicit_once"
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
  "hasSeenWelcome" | "rateLimitsEnabled" | "keychainAccessRequested" | "launchAtLogin" | "sshHosts"
>;

export function getPermissionSurfaces(
  settings: PermissionSettings,
  platform: PermissionPlatform,
): PermissionSurface[] {
  const surfaces: PermissionSurface[] = [
    {
      id: "usage_logs",
      title: "Session logs",
      status: settings.hasSeenWelcome ? "Enabled" : "After welcome",
      tone: settings.hasSeenWelcome ? "ok" : "neutral",
      why:
        "Reads Claude Code and Codex session logs to calculate tokens, cost, models, and local activity.",
      requestPolicy: "after_first_run_disclosure",
      requestCopy:
        platform.macos
          ? "macOS folder access can appear only if those CLI folders live in Desktop, Documents, Downloads, a network volume, or an external drive."
          : "No OS permission prompt is expected on this platform.",
      paths: [
        "~/.claude/projects",
        "~/.config/claude/projects",
        "~/.codex/sessions",
      ],
    },
    {
      id: "claude_credentials",
      title: "Claude rate-limit credentials",
      status: settings.rateLimitsEnabled ? "Enabled" : "Off",
      tone: settings.rateLimitsEnabled ? "ok" : "neutral",
      why:
        "Reads the Claude Code credentials file to fetch live 5h and weekly rate-limit windows.",
      requestPolicy: "silent_when_enabled",
      requestCopy:
        "This is the primary rate-limit path and does not show a Keychain prompt.",
      paths: ["~/.claude/.credentials.json"],
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
        platform.macos
          ? "macOS may list this under Login Items; TokenMonitor changes it only from the welcome card or Settings toggle."
          : "TokenMonitor changes this only from the welcome card or Settings toggle.",
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

  if (platform.macos) {
    surfaces.splice(2, 0, {
      id: "keychain_fallback",
      title: "Keychain fallback",
      status: settings.keychainAccessRequested ? "Handled" : "Available if needed",
      tone: settings.keychainAccessRequested ? "ok" : "warn",
      why:
        "Optional fallback for Claude live rate limits if the credentials file is unavailable.",
      requestPolicy: "explicit_once",
      requestCopy:
        "TokenMonitor never opens the Keychain prompt automatically. It can appear once only after you click Allow Keychain access.",
      paths: ["Claude Code-credentials item in macOS Keychain"],
    });
  }

  return surfaces;
}

export function permissionSurfaceById(
  surfaces: PermissionSurface[],
  id: PermissionSurfaceId,
): PermissionSurface | undefined {
  return surfaces.find((surface) => surface.id === id);
}
