import { convertFileSrc, invoke } from "@tauri-apps/api/core";

import type { PhotoId, PhotoSrc } from "../types/photo";

/**
 * Transcode a photo (any format, incl. HEIC) to a display-ready JPEG via the
 * Rust sidecar, then convert the temp-file path to an asset URL the webview
 * can load. Components NEVER see raw OS paths — this is the single point where
 * `convertFileSrc` wraps the Rust-provided temp path into a `PhotoSrc`.
 */
export async function transcodeForDisplay(id: PhotoId): Promise<PhotoSrc> {
  const dest = await invoke<unknown>("transcode_for_display", { photoId: id });
  if (typeof dest !== "string" || dest.length === 0) {
    throw new Error("transcode_for_display: unexpected response shape");
  }
  // convertFileSrc converts the OS temp path to an asset:// URL that the Tauri
  // webview is allowed to load (assetProtocol scope ["**"] already includes temp).
  return convertFileSrc(dest) as PhotoSrc;
}
