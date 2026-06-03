import { describe, it, expect } from "vitest";
import {
  exportFileName,
  formatExportSummary,
  formatImportSummary,
  type ExportResult,
  type ImportResult,
} from "./usageIo.js";

describe("exportFileName", () => {
  it("extracts the file name from a Windows path", () => {
    expect(exportFileName("C:\\Users\\me\\Downloads\\TokenMonitor-Usage-20260602.json")).toBe(
      "TokenMonitor-Usage-20260602.json",
    );
  });

  it("extracts the file name from a POSIX path", () => {
    expect(exportFileName("/home/me/Downloads/TokenMonitor-Usage.json")).toBe(
      "TokenMonitor-Usage.json",
    );
  });
});

describe("formatExportSummary", () => {
  it("summarizes a multi-source export", () => {
    const r: ExportResult = { path: "/tmp/a.json", sourceCount: 2, recordCount: 1240 };
    expect(formatExportSummary(r)).toBe("Saved 1,240 records from 2 sources → a.json");
  });

  it("uses singular for one source", () => {
    const r: ExportResult = { path: "/tmp/a.json", sourceCount: 1, recordCount: 5 };
    expect(formatExportSummary(r)).toContain("from 1 source");
  });

  it("handles an empty archive", () => {
    const r: ExportResult = { path: "/tmp/a.json", sourceCount: 0, recordCount: 0 };
    expect(formatExportSummary(r)).toBe("Nothing to export yet");
  });
});

describe("formatImportSummary", () => {
  it("reports new vs deduplicated counts", () => {
    const r: ImportResult = {
      sources: [],
      totalSeen: 1240,
      totalNew: 30,
      totalDeduped: 1210,
    };
    expect(formatImportSummary(r)).toBe("Imported 1,240 records · 30 new · 1,210 deduplicated");
  });

  it("handles an empty/foreign file", () => {
    const r: ImportResult = { sources: [], totalSeen: 0, totalNew: 0, totalDeduped: 0 };
    expect(formatImportSummary(r)).toBe("No usage records found in that file");
  });
});
