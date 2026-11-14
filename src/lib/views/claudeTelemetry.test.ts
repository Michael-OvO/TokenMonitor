import { describe, expect, it } from "vitest";
import type { ClaudeOtelStatus } from "../types/index.js";
import { telemetryAction, telemetryConflictMessage, telemetryEnvSnippet, telemetryStatusView } from "./claudeTelemetry.js";

const status = (over: Partial<ClaudeOtelStatus> = {}): ClaudeOtelStatus => ({
  port: 53218,
  listening: true,
  bindError: null,
  eventsReceived: 0,
  lastEventAt: null,
  ledgerDir: "/tmp/claude-otel",
  ...over,
});

describe("telemetryEnvSnippet", () => {
  it("exports the OTLP/HTTP JSON settings for the given port", () => {
    const lines = telemetryEnvSnippet(53218).split("\n");
    expect(lines).toEqual([
      "export CLAUDE_CODE_ENABLE_TELEMETRY=1",
      "export OTEL_LOGS_EXPORTER=otlp",
      "export OTEL_EXPORTER_OTLP_PROTOCOL=http/json",
      "export OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:53218",
    ]);
  });
});

describe("telemetryStatusView", () => {
  const now = new Date("2026-11-14T08:00:00Z");
  const on = { state: "on" } as const;
  const off = { state: "off" } as const;

  it("handles missing data, a busy port, conflicts and off", () => {
    expect(telemetryStatusView(null, status(), now)).toEqual({ label: "Unavailable", tone: "neutral" });
    expect(telemetryStatusView(on, status({ listening: false, bindError: "in use" }), now)).toEqual({ label: "Port busy", tone: "warn" });
    expect(telemetryStatusView({ state: "conflict", key: "K", value: "V" }, status(), now)).toEqual({ label: "Conflict", tone: "amber" });
    expect(telemetryStatusView(off, status(), now)).toEqual({ label: "Off", tone: "neutral" });
  });

  it("shows the newest request when on", () => {
    expect(telemetryStatusView(on, status(), now)).toEqual({ label: "On", tone: "ok" });
    expect(telemetryStatusView(on, status({ lastEventAt: "2026-11-14T07:55:00Z" }), now).label).toBe("On · last request 5m ago");
  });
});

describe("telemetryAction / telemetryConflictMessage", () => {
  it("offers the opposite action and explains conflicts", () => {
    expect(telemetryAction({ state: "on" })).toBe("Turn off");
    expect(telemetryAction({ state: "off" })).toBe("Turn on");
    expect(telemetryAction({ state: "conflict", key: "K", value: "V" })).toBeNull();
    expect(telemetryAction(null)).toBeNull();
    expect(telemetryConflictMessage({ state: "conflict", key: "OTEL_LOGS_EXPORTER", value: "none" })).toBe(
      "OTEL_LOGS_EXPORTER is set to \"none\" in Claude's settings.json, so TokenMonitor left it alone. Remove it to record headless runs.",
    );
    expect(telemetryConflictMessage({ state: "on" })).toBeNull();
  });
});
