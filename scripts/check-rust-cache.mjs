import { readdir, lstat } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const gib = 1024 ** 3;
const threshold = 15 * gib;

// Ask Cargo so CARGO_TARGET_DIR and Cargo configuration are respected.
// This reads metadata only: it neither downloads nor builds dependencies.
try {
  const metadata = spawnSync("cargo", [
    "metadata", "--no-deps", "--format-version", "1", "--offline", "--locked",
  ], { cwd: join(root, "src-tauri"), encoding: "utf8", timeout: 10000 });
  if (metadata.status !== 0) throw new Error("Cargo metadata unavailable");
  const target = JSON.parse(metadata.stdout).target_directory;
  const pending = [target];
  const seen = new Set();
  let bytes = 0;
  while (pending.length) {
    const path = pending.pop();
    let stat;
    try {
      stat = await lstat(path, { bigint: true });
    } catch (error) {
      if (error.code === "ENOENT") continue; // No builds yet, or a concurrent clean.
      throw error;
    }
    if (stat.isSymbolicLink()) continue;
    if (stat.isDirectory()) {
      for (const name of await readdir(path)) pending.push(join(path, name));
    } else if (stat.isFile()) {
      const key = `${stat.dev}:${stat.ino}`;
      if (!seen.has(key)) {
        seen.add(key);
        bytes += Number(stat.size);
      }
    }
    // Startup only needs to detect a large cache; avoid scanning the rest.
    if (bytes >= threshold && !process.argv.includes("--status")) break;
  }
  if (bytes >= threshold) {
    console.warn(`[rust-cache] ${target} uses ${process.argv.includes("--status") ? "about" : "at least"} ${(bytes / gib).toFixed(1)} GiB (15 GiB advisory threshold).`);
    console.warn("[rust-cache] After stopping development/builds, run npm run cache:clean:dev if needed. Nothing is deleted automatically.");
  } else if (process.argv.includes("--status")) {
    console.log(`[rust-cache] ${target}: ${(bytes / gib).toFixed(2)} GiB / 15 GiB advisory threshold.`);
  }
} catch (error) {
  // An optional disk-space check must not prevent frontend or Tauri development.
  console.warn(`[rust-cache] Could not check cache size: ${error.message}. Continuing.`);
}
