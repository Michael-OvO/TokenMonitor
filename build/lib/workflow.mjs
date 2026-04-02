import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { access, copyFile, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

import { detectHostArch, detectHostPlatformId, resolveRequestedPlatform } from "./platform.mjs";

const BUILD_DIR = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const REPO_ROOT = path.resolve(BUILD_DIR, "..");
const OUTPUT_ROOT = path.join(REPO_ROOT, "outputs");
const SRC_TAURI_DIR = path.join(REPO_ROOT, "src-tauri");
const TAURI_BUNDLE_ROOT = path.join(SRC_TAURI_DIR, "target", "release", "bundle");
const PACKAGE_JSON_PATH = path.join(REPO_ROOT, "package.json");
const TAURI_CONFIG_PATH = path.join(SRC_TAURI_DIR, "tauri.conf.json");

export async function runBuild(options) {
  const hostPlatformId = detectHostPlatformId();
  const platform = resolveRequestedPlatform(options.platform, hostPlatformId);
  const context = {
    arch: detectHostArch(),
    ci: options.ci,
    outputDir: path.join(OUTPUT_ROOT, platform.id),
    packageMeta: await readJson(PACKAGE_JSON_PATH),
    platform,
    verbose: options.verbose,
  };

  await verifyTauriConfig();
  await preflight(context);

  if (options.clean) {
    await rm(context.outputDir, { recursive: true, force: true });
  }
  await mkdir(context.outputDir, { recursive: true });

  console.log(`[build] TokenMonitor ${context.packageMeta.version}`);
  console.log(`[build] host: ${platform.displayName} (${context.arch})`);
  console.log(`[build] output: ${context.outputDir}`);

  await runTauriBuild(context);
  const copiedArtifacts = await collectArtifacts(context);
  const checksumFile = await writeChecksums(copiedArtifacts, context.outputDir);

  return {
    artifacts: copiedArtifacts.map((artifactPath) => path.basename(artifactPath)),
    checksumFile,
    outputDir: context.outputDir,
    platform,
  };
}

async function verifyTauriConfig() {
  const tauriConfig = await readJson(TAURI_CONFIG_PATH);
  if (!tauriConfig?.bundle?.active) {
    throw new Error("Tauri bundling is disabled in src-tauri/tauri.conf.json.");
  }
}

async function preflight(context) {
  await ensureCommand("cargo", ["--version"], "Rust toolchain");
  await ensureCommand(npxCommand(), ["tauri", "--version"], "Tauri CLI");

  if (context.platform.id === "linux") {
    await ensureLinuxDependency("pkg-config", ["--version"], "pkg-config");
    await ensurePkgConfigOneOf(["webkit2gtk-4.1"], "libwebkit2gtk-4.1-dev");
    await ensurePkgConfigOneOf(
      ["ayatana-appindicator3-0.1", "appindicator3-0.1"],
      "libayatana-appindicator3-dev or libappindicator3-dev",
    );
    await ensureLinuxDependency("patchelf", ["--version"], "patchelf");
  }
}

async function ensurePkgConfigOneOf(names, installHint) {
  for (const name of names) {
    if (await commandSucceeds("pkg-config", ["--exists", name])) {
      return;
    }
  }

  throw new Error(
    `Missing Linux system dependency. Install ${installHint} before running the installer build.`,
  );
}

async function ensureLinuxDependency(command, args, label) {
  if (!(await commandSucceeds(command, args))) {
    throw new Error(`Missing ${label}. Install the Linux build prerequisites and retry.`);
  }
}

async function runTauriBuild(context) {
  const configPath = path.join(REPO_ROOT, "build", "config", `tauri.${context.platform.id}.json`);
  const args = ["tauri", "build", "--config", configPath];

  if (context.ci) {
    args.push("--ci");
  }
  if (context.verbose) {
    args.push("--verbose");
  }
  if (shouldDisableSigning(context.platform.id)) {
    args.push("--no-sign");
  }

  await runCommand(npxCommand(), args, {
    cwd: REPO_ROOT,
    env: process.env,
    stdio: "inherit",
  });
}

function shouldDisableSigning(platformId) {
  if (platformId !== "macos") {
    return false;
  }

  return !(
    process.env.APPLE_SIGNING_IDENTITY
    || process.env.APPLE_API_KEY
    || process.env.APPLE_API_KEY_PATH
  );
}

async function collectArtifacts(context) {
  const sourceDir = path.join(TAURI_BUNDLE_ROOT, context.platform.bundleDir);
  await assertExists(sourceDir, `Expected bundle directory was not created: ${sourceDir}`);

  const entries = await readdir(sourceDir, { withFileTypes: true });
  const sourceArtifacts = entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(context.platform.artifactExtension))
    .map((entry) => path.join(sourceDir, entry.name))
    .sort();

  if (sourceArtifacts.length === 0) {
    throw new Error(
      `No ${context.platform.artifactExtension} installers were found in ${sourceDir}.`,
    );
  }

  const copiedArtifacts = [];
  for (const sourceArtifact of sourceArtifacts) {
    const destination = path.join(context.outputDir, path.basename(sourceArtifact));
    await copyFile(sourceArtifact, destination);
    copiedArtifacts.push(destination);
  }

  return copiedArtifacts;
}

async function writeChecksums(artifactPaths, outputDir) {
  const checksumPath = path.join(outputDir, "SHA256SUMS.txt");
  const lines = [];

  for (const artifactPath of artifactPaths) {
    const digest = await sha256File(artifactPath);
    lines.push(`${digest}  ${path.basename(artifactPath)}`);
  }

  await writeFile(checksumPath, `${lines.join("\n")}\n`, "utf8");
  return checksumPath;
}

async function sha256File(filePath) {
  const hash = createHash("sha256");
  await new Promise((resolve, reject) => {
    const stream = createReadStream(filePath);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("end", resolve);
    stream.on("error", reject);
  });
  return hash.digest("hex");
}

async function ensureCommand(command, args, label) {
  const result = await runCommand(command, args, {
    cwd: REPO_ROOT,
    env: process.env,
    stdio: ["ignore", "pipe", "pipe"],
  });

  if (result.code !== 0) {
    throw new Error(`${label} is not available. Command failed: ${command} ${args.join(" ")}`);
  }
}

async function commandSucceeds(command, args) {
  try {
    const result = await runCommand(command, args, {
      cwd: REPO_ROOT,
      env: process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    return result.code === 0;
  } catch {
    return false;
  }
}

async function runCommand(command, args, options) {
  return new Promise((resolve, reject) => {
    const spawnSpec = resolveSpawnSpec(command, args);
    const child = spawn(spawnSpec.command, spawnSpec.args, options);
    let stdout = "";
    let stderr = "";

    if (child.stdout) {
      child.stdout.on("data", (chunk) => {
        stdout += chunk.toString();
      });
    }
    if (child.stderr) {
      child.stderr.on("data", (chunk) => {
        stderr += chunk.toString();
      });
    }

    child.on("error", (error) => {
      reject(new Error(`Failed to start command "${command}": ${error.message}`));
    });

    child.on("close", (code) => {
      resolve({ code: code ?? 1, stdout, stderr });
    });
  });
}

function resolveSpawnSpec(command, args) {
  if (process.platform === "win32" && (command.endsWith(".cmd") || command.endsWith(".bat"))) {
    const comspec = process.env.ComSpec ?? "cmd.exe";
    const commandToken = /[ \t"]/u.test(command) ? quoteWindowsArg(command) : command;
    const commandLine = [commandToken, ...args.map(quoteWindowsArg)].join(" ");
    return {
      command: comspec,
      args: ["/d", "/s", "/c", commandLine],
    };
  }

  return { command, args };
}

function quoteWindowsArg(arg) {
  if (!arg.length) {
    return '""';
  }

  if (!/[ \t"]/u.test(arg)) {
    return arg;
  }

  let result = '"';
  let backslashes = 0;

  for (const char of arg) {
    if (char === "\\") {
      backslashes += 1;
      continue;
    }

    if (char === '"') {
      result += `${"\\".repeat(backslashes * 2 + 1)}"`;
      backslashes = 0;
      continue;
    }

    if (backslashes > 0) {
      result += "\\".repeat(backslashes);
      backslashes = 0;
    }
    result += char;
  }

  if (backslashes > 0) {
    result += "\\".repeat(backslashes * 2);
  }

  return `${result}"`;
}

function npxCommand() {
  return process.platform === "win32" ? "npx.cmd" : "npx";
}

async function assertExists(targetPath, errorMessage) {
  try {
    await access(targetPath);
  } catch {
    throw new Error(errorMessage);
  }
}

async function readJson(filePath) {
  const content = await readFile(filePath, "utf8");
  return JSON.parse(content);
}
