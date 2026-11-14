import { describe, expect, it } from "vitest";
import type { ClaudeOtelStatus } from "../types/index.js";
import { telemetryEnvSnippet, telemetryStatusView } from "./claudeTelemetry.js";

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

  it("reports missing status, bind failure and idle listening", () => {
    expect(telemetryStatusView(null, now)).toEqual({ label: "Unavailable", tone: "neutral" });
    expect(telemetryStatusView(status({ listening: false, bindError: "in use" }), now)).toEqual({
      label: "Port busy",
      tone: "warn",
    });
    expect(telemetryStatusView(status({ listening: false }), now).label).toBe("Off");
    expect(telemetryStatusView(status(), now)).toEqual({ label: "Listening", tone: "neutral" });
  });

  it("shows how long ago the newest request arrived", () => {
    const at = (iso: string) => telemetryStatusView(status({ lastEventAt: iso }), now).label;
    expect(at("2026-11-14T07:59:40Z")).toBe("Last request just now");
    expect(at("2026-11-14T07:55:00Z")).toBe("Last request 5m ago");
    expect(at("2026-11-14T05:00:00Z")).toBe("Last request 3h ago");
    expect(at("2026-11-10T08:00:00Z")).toBe("Last request 4d ago");
    expect(telemetryStatusView(status({ lastEventAt: "2026-11-14T07:59:40Z" }), now).tone).toBe("ok");
  });
});
