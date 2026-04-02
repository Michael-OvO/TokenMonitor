export const PLATFORM_SPECS = Object.freeze({
  macos: Object.freeze({
    id: "macos",
    hostPlatform: "darwin",
    displayName: "macOS",
    bundleDir: "dmg",
    artifactExtension: ".dmg",
    bundleTarget: "dmg",
  }),
  windows: Object.freeze({
    id: "windows",
    hostPlatform: "win32",
    displayName: "Windows",
    bundleDir: "nsis",
    artifactExtension: ".exe",
    bundleTarget: "nsis",
  }),
  linux: Object.freeze({
    id: "linux",
    hostPlatform: "linux",
    displayName: "Linux",
    bundleDir: "deb",
    artifactExtension: ".deb",
    bundleTarget: "deb",
  }),
});

const HOST_PLATFORM_TO_ID = Object.freeze({
  darwin: "macos",
  win32: "windows",
  linux: "linux",
});

export function detectHostPlatformId(nodePlatform = process.platform) {
  const platformId = HOST_PLATFORM_TO_ID[nodePlatform];
  if (!platformId) {
    throw new Error(
      `Unsupported host platform "${nodePlatform}". Supported hosts: macOS, Windows, Linux.`,
    );
  }
  return platformId;
}

export function detectHostArch(nodeArch = process.arch) {
  switch (nodeArch) {
    case "x64":
      return "x64";
    case "arm64":
      return "arm64";
    default:
      return nodeArch;
  }
}

export function getPlatformSpec(platformId) {
  const spec = PLATFORM_SPECS[platformId];
  if (!spec) {
    throw new Error(
      `Unsupported platform "${platformId}". Supported values: current, macos, windows, linux.`,
    );
  }
  return spec;
}

export function resolveRequestedPlatform(requestedPlatform, hostPlatformId = detectHostPlatformId()) {
  if (!requestedPlatform || requestedPlatform === "current") {
    return getPlatformSpec(hostPlatformId);
  }

  const requestedSpec = getPlatformSpec(requestedPlatform);
  if (requestedSpec.id !== hostPlatformId) {
    const host = getPlatformSpec(hostPlatformId);
    throw new Error(
      `Cross-platform builds are not supported from this host. Requested ${requestedSpec.displayName}, running on ${host.displayName}.`,
    );
  }

  return requestedSpec;
}
