#!/usr/bin/env node

import { formatUsage, parseArgs } from "./lib/cli.mjs";
import { runBuild } from "./lib/workflow.mjs";

async function main() {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.help) {
      console.log(formatUsage());
      return;
    }

    const result = await runBuild(options);
    const artifactList = result.artifacts.map((artifact) => `  - ${artifact}`).join("\n");

    console.log("");
    console.log(`[build] ${result.platform.displayName} installers ready`);
    console.log(`[build] output: ${result.outputDir}`);
    console.log(`[build] checksums: ${result.checksumFile}`);
    console.log("[build] artifacts:");
    console.log(artifactList);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`[build] ${message}`);
    process.exitCode = 1;
  }
}

void main();
