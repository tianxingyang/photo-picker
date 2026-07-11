import { invoke } from "@tauri-apps/api/core";

/** Outcome of a thumbnail batch (mirrors Rust `ThumbnailSummary`, serde camelCase). */
export type ThumbnailSummary = {
  generated: number;
  /** Up-to-date cache hits + vanished sources + photos skipped after a cancel. */
  skipped: number;
  failed: number;
  /** true when the user cancelled mid-batch (remaining photos stay pending). */
  cancelled: boolean;
};

/** Generate 512px WebP thumbnails for every photo that needs one in the open
 *  project (Rust `generate_thumbnails`). Idempotent + self-healing: up-to-date
 *  thumbnails are skipped, changed/missing ones are regenerated. */
export async function generateThumbnails(): Promise<ThumbnailSummary> {
  return await invoke<ThumbnailSummary>("generate_thumbnails");
}
