const DEFAULT_OPTIONS = Object.freeze({
  platform: "current",
  ci: false,
  clean: false,
  verbose: false,
  help: false,
});

export function parseArgs(argv) {
  const options = { ...DEFAULT_OPTIONS };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];

    switch (arg) {
      case "--ci":
        options.ci = true;
        break;
      case "--clean":
        options.clean = true;
        break;
      case "--verbose":
        options.verbose = true;
        break;
      case "--help":
      case "-h":
        options.help = true;
        break;
      case "--platform":
        options.platform = readValue(argv, ++index, "--platform");
        break;
      default:
        if (arg.startsWith("--platform=")) {
          options.platform = arg.slice("--platform=".length);
          break;
        }
        throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return options;
}

function readValue(argv, index, flagName) {
  const value = argv[index];
  if (!value || value.startsWith("--")) {
    throw new Error(`Missing value for ${flagName}`);
  }
  return value;
}

export function formatUsage() {
  return [
    "Usage: node build/index.mjs [options]",
    "",
    "Options:",
    "  --platform <current|macos|windows|linux>  Build for the host platform only",
    "  --ci                                      Pass --ci through to Tauri",
    "  --clean                                   Remove outputs/<platform> before copying artifacts",
    "  --verbose                                 Enable verbose Tauri build logs",
    "  -h, --help                                Show this help",
  ].join("\n");
}
