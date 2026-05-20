#!/usr/bin/env node
import { readFileSync, existsSync } from "fs";
import { execSync } from "child_process";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const cargoLock = join(root, "src-tauri", "Cargo.lock");
if (!existsSync(cargoLock)) process.exit(0);

const lock = readFileSync(cargoLock, "utf8");
const m = lock.match(/name = "tauri"\s+version = "(\d+)\.(\d+)\.\d+"/);
if (!m) process.exit(0);

const [, rustMajor, rustMinor] = m;

const apiPkg = join(root, "node_modules", "@tauri-apps", "api", "package.json");
if (!existsSync(apiPkg)) process.exit(0);

const apiVer = JSON.parse(readFileSync(apiPkg, "utf8")).version;
const [apiMajor, apiMinor] = apiVer.split(".");

if (rustMajor === apiMajor && rustMinor === apiMinor) process.exit(0);

console.log(
  `[sync-tauri] tauri crate ${m[0].match(/\d+\.\d+\.\d+/)[0]} vs @tauri-apps/api ${apiVer} — aligning npm package`
);
execSync(`npm install @tauri-apps/api@~${rustMajor}.${rustMinor}.0`, {
  cwd: root,
  stdio: "inherit",
});
