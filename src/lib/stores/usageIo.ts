import { invoke } from "@tauri-apps/api/core";

// Mirrors src-tauri/src/commands/usage_io.rs (camelCase wire format).
export type ExportResult = {
  path: string;
  sourceCount: number;
  recordCount: number;
};

export type ImportSourceStats = {
  sourceKey: string;
  seen: number;
  newBuckets: number;
  deduped: number;
};

export type ImportResult = {
  sources: ImportSourceStats[];
  totalSeen: number;
  totalNew: number;
  totalDeduped: number;
};

/** Write the usage archive to a JSON file at the caller-chosen `path`. */
export async function exportUsageData(path: string): Promise<ExportResult> {
  return invoke<ExportResult>("export_usage_data", { path });
}

/** Merge a previously exported JSON document into the archive (dedup on import). */
export async function importUsageData(json: string): Promise<ImportResult> {
  return invoke<ImportResult>("import_usage_data", { json });
}

/** File name (without directory) of an export path, for compact UI display. */
export function exportFileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export function formatExportSummary(r: ExportResult): string {
  if (r.recordCount === 0) {
    return "Nothing to export yet";
  }
  const records = r.recordCount.toLocaleString();
  const sources = r.sourceCount === 1 ? "1 source" : `${r.sourceCount} sources`;
  return `Saved ${records} records from ${sources} → ${exportFileName(r.path)}`;
}

export function formatImportSummary(r: ImportResult): string {
  if (r.totalSeen === 0) {
    return "No usage records found in that file";
  }
  const seen = r.totalSeen.toLocaleString();
  return `Imported ${seen} records · ${r.totalNew.toLocaleString()} new · ${r.totalDeduped.toLocaleString()} deduplicated`;
}
