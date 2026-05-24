import { convertFileSrc, invoke } from "@tauri-apps/api/core";

import type { Photo, PhotoId, PhotoSrc, PhotoStatus } from "../types/photo";

// Raw row shape from Rust `scan_folder` (serde camelCase). Validated at the
// boundary because Rust↔TS type sharing is still OPEN (no codegen yet).
type PhotoRowRaw = {
  id: string;
  path: string;
  status: string;
  createdAt: string;
};

const STATUSES: readonly string[] = ["pending", "keep", "reject"];

function isPhotoRow(v: unknown): boolean {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.id === "string" &&
    typeof o.path === "string" &&
    typeof o.status === "string" &&
    typeof o.createdAt === "string"
  );
}

function basename(path: string): string {
  // why: stored paths are OS-native; split on both separators for the leaf.
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

function toPhoto(row: PhotoRowRaw): Photo {
  const status: PhotoStatus = STATUSES.includes(row.status)
    ? (row.status as PhotoStatus)
    : "pending";
  return {
    id: row.id as PhotoId,
    name: basename(row.path),
    status,
    src: convertFileSrc(row.path) as PhotoSrc,
  };
}

export type ScanResult = { photos: Photo[]; skipped: number };

export async function scanFolder(path: string): Promise<ScanResult> {
  const raw = await invoke<unknown>("scan_folder", { path });
  if (!raw || typeof raw !== "object") {
    throw new Error("scan_folder returned an unexpected shape");
  }
  const o = raw as { photos?: unknown; skipped?: unknown };
  if (!Array.isArray(o.photos) || typeof o.skipped !== "number" || !o.photos.every(isPhotoRow)) {
    throw new Error("scan_folder returned an unexpected shape");
  }
  // validated narrowing above
  return { photos: (o.photos as PhotoRowRaw[]).map(toPhoto), skipped: o.skipped };
}
