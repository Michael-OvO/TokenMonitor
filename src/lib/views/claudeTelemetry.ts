import type { ClaudeOtelStatus } from "../types/index.js";

export type TelemetryTone = "ok" | "amber" | "warn" | "neutral";

/**
 * Shell lines that point Claude Code's OpenTelemetry export at TokenMonitor.
 * They belong in the shell profile: headless runs started with
 * `--setting-sources ""` ignore the `env` block of Claude settings files.
 */
export function telemetryEnvSnippet(port: number): string {
  return [
    "export CLAUDE_CODE_ENABLE_TELEMETRY=1",
    "export OTEL_LOGS_EXPORTER=otlp",
    "export OTEL_EXPORTER_OTLP_PROTOCOL=http/json",
    `export OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:${port}`,
  ].join("\n");
}

function ago(iso: string, now: Date): string {
  const seconds = Math.max(0, Math.round((now.getTime() - new Date(iso).getTime()) / 1000));
  if (seconds < 60) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

export function telemetryStatusView(
  status: ClaudeOtelStatus | null,
  now: Date = new Date(),
): { label: string; tone: TelemetryTone } {
  if (!status) return { label: "Unavailable", tone: "neutral" };
  if (!status.listening) {
    return { label: status.bindError ? "Port busy" : "Off", tone: "warn" };
  }
  if (status.lastEventAt) {
    return { label: `Last request ${ago(status.lastEventAt, now)}`, tone: "ok" };
  }
  return { label: "Listening", tone: "neutral" };
}
