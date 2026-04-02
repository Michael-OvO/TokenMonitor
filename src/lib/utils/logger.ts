import { invoke } from "@tauri-apps/api/core";

export type LogLevel = "error" | "warn" | "info" | "debug";

const LEVEL_PRIORITY: Record<LogLevel, number> = {
  error: 0,
  warn: 1,
  info: 2,
  debug: 3,
};

let currentLevel: LogLevel = "info";
let ipcReady = false;

function shouldLog(level: LogLevel): boolean {
  return LEVEL_PRIORITY[level] <= LEVEL_PRIORITY[currentLevel];
}

function sendLog(level: LogLevel, category: string, message: string): void {
  if (!shouldLog(level)) return;

  const timestamped = `${new Date().toISOString()} ${message}`;

  // Forward to Rust via IPC for file persistence
  if (ipcReady) {
    invoke("log_frontend_message", { level, category, message: timestamped }).catch(() => {
      // Silently ignore IPC failures to prevent error loops
    });
  }
}

export const logger = {
  error(category: string, message: string): void {
    sendLog("error", category, message);
  },
  warn(category: string, message: string): void {
    sendLog("warn", category, message);
  },
  info(category: string, message: string): void {
    sendLog("info", category, message);
  },
  debug(category: string, message: string): void {
    sendLog("debug", category, message);
  },
  setLevel(level: LogLevel): void {
    currentLevel = level;
  },
  setIpcReady(): void {
    ipcReady = true;
  },
  getLevel(): LogLevel {
    return currentLevel;
  },
};
