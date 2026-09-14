import type { RemoteDeviceIncludeConfig } from "../types/index.js";

export function setRemoteDeviceIncludeFlag(
  devices: RemoteDeviceIncludeConfig[],
  alias: string,
  includeInStats: boolean,
): RemoteDeviceIncludeConfig[] {
  const next = {
    alias,
    include_in_stats: includeInStats,
  };

  if (!devices.some((device) => device.alias === alias)) {
    return [...devices, next];
  }

  return devices.map((device) => (device.alias === alias ? next : device));
}
