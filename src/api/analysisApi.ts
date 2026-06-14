import { invoke } from "@tauri-apps/api/core";

/** Outcome of an analyze batch (mirrors Rust `AnalyzeSummary`, serde camelCase). */
export type AnalyzeSummary = {
  analyzed: number;
  failed: number;
  /** true when the user cancelled mid-batch (remaining photos stay pending). */
  cancelled: boolean;
};

/** Analyze every pending photo (EXIF/blur/exposure/pHash) — Rust `analyze_pending`. */
export async function analyzePending(): Promise<AnalyzeSummary> {
  return await invoke<AnalyzeSummary>("analyze_pending");
}
