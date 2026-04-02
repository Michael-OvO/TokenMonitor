import type { SshHostConfig, UsagePayload } from "../types/index.js";

export function setDeviceIncludeFlag(
  payload: UsagePayload | null,
  device: string,
  includeInStats: boolean,
): UsagePayload | null {
  if (!payload?.device_breakdown) {
    return payload;
  }

  return {
    ...payload,
    device_breakdown: payload.device_breakdown.map((entry) =>
      entry.device === device ? { ...entry, include_in_stats: includeInStats } : entry,
    ),
  };
}

export function setSshHostIncludeFlag(
  hosts: SshHostConfig[],
  alias: string,
  includeInStats: boolean,
): SshHostConfig[] {
  return hosts.map((host) =>
    host.alias === alias ? { ...host, include_in_stats: includeInStats } : host,
  );
}
