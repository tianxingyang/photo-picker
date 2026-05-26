import { invoke } from "@tauri-apps/api/core";

/** Analyze every pending photo (EXIF/blur/exposure/pHash) — Rust `analyze_pending`. */
export async function analyzePending(): Promise<void> {
  await invoke("analyze_pending");
}
