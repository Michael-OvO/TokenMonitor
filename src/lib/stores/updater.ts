import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type UpdateInfo = {
  version: string;
  currentVersion: string;
  notes: string | null;
  pubDate: string | null;
};

export type DownloadProgress = {
  downloaded: number;
  total: number | null;
  percent: number | null;
};

export type InstallMode = "auto" | "manual";

export type UpdaterSnapshot = {
  available: UpdateInfo | null;
  lastCheck: string | null;
  lastCheckError: string | null;
  skippedVersions: string[];
  autoCheckEnabled: boolean;
  dismissedForSession: boolean;
  progress: DownloadProgress | null;
  currentVersion: string;
  installMode: InstallMode;
};

const INITIAL: UpdaterSnapshot = {
  available: null,
  lastCheck: null,
  lastCheckError: null,
  skippedVersions: [],
  autoCheckEnabled: true,
  dismissedForSession: false,
  progress: null,
  currentVersion: "0.0.0",
  installMode: "auto",
};

export const updaterStore = writable<UpdaterSnapshot>(INITIAL);

type RustUpdaterState = {
  available: UpdateInfo | null;
  lastCheck: string | null;
  lastCheckError: string | null;
  skippedVersions: string[];
  lastNotifiedVersion: string | null;
  autoCheckEnabled: boolean;
  progress: DownloadProgress | null;
  dismissedForSession: boolean;
};

type StatusPayload = {
  state: RustUpdaterState;
  currentVersion: string;
  installMode: InstallMode;
};

function project(payload: StatusPayload): UpdaterSnapshot {
  return {
    available: payload.state.available,
    lastCheck: payload.state.lastCheck,
    lastCheckError: payload.state.lastCheckError,
    skippedVersions: payload.state.skippedVersions,
    autoCheckEnabled: payload.state.autoCheckEnabled,
    dismissedForSession: payload.state.dismissedForSession,
    progress: payload.state.progress,
    currentVersion: payload.currentVersion,
    installMode: payload.installMode,
  };
}

export async function hydrateUpdater(): Promise<void> {
  try {
    const payload = await invoke<StatusPayload>("updater_status");
    updaterStore.set(project(payload));
  } catch {
    // leave defaults; the Rust side will emit status-changed later.
  }
}

let listenersInstalled = false;
export async function installUpdaterListeners(): Promise<void> {
  if (listenersInstalled) return;
  listenersInstalled = true;
  await listen("updater://status-changed", () => {
    hydrateUpdater();
  });
  await listen<DownloadProgress>("updater://progress", (e) => {
    updaterStore.update((s) => ({ ...s, progress: e.payload }));
  });
}

export async function checkNow(): Promise<void> {
  await invoke("updater_check_now");
}

export async function installUpdate(): Promise<void> {
  await invoke("updater_install");
}

export async function skipVersion(version: string): Promise<void> {
  await invoke("updater_skip_version", { version });
}

export async function dismissBanner(): Promise<void> {
  await invoke("updater_dismiss");
}

export async function setAutoCheck(enabled: boolean): Promise<void> {
  await invoke("updater_set_auto_check", { enabled });
}
