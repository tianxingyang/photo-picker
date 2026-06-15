import { invoke } from "@tauri-apps/api/core";

/** Cooperatively cancel an in-progress analyze batch (Rust `cancel_analysis`).
 *  Already-analyzed photos stay `done`; the rest stay `pending` for a re-run. */
export async function cancelAnalysis(): Promise<void> {
  await invoke("cancel_analysis");
}

/** Cooperatively cancel an in-progress thumbnail batch (Rust `cancel_thumbnails`).
 *  Already-generated thumbnails persist; the rest are picked up on the next run. */
export async function cancelThumbnails(): Promise<void> {
  await invoke("cancel_thumbnails");
}
