import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
if (args[0] === "dev" && !args.includes("--help") && !args.includes("-h") && process.platform === "win32") {
  const cleanup = spawnSync("powershell.exe", [
    "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
    "-File", join(root, "scripts", "clean-idle-rust-cache.ps1"),
  ], { cwd: root, stdio: "inherit", windowsHide: true });
  if (cleanup.error || cleanup.status !== 0) {
    console.warn("[rust-cache] Cleanup skipped; continuing development.");
  }
}
const result = spawnSync(process.execPath, [
  join(root, "node_modules", "@tauri-apps", "cli", "tauri.js"), ...args,
], { cwd: root, stdio: "inherit" });
if (result.error) console.error(result.error.message);
process.exit(result.status ?? 1);
