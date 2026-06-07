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

// Tauri v2: JS camelCase `photoId` ↔ Rust snake_case `photo_id`. Shared by
// group-browse-ui and (future) ab-compare — both call through here, never
// `invoke` directly (`@tauri-apps/api` only appears in `api/`).
export async function setPhotoStatus(id: PhotoId, status: PhotoStatus): Promise<void> {
  await invoke("set_status", { photoId: id, status });
}

export type ExportFailure = { source: string; reason: string };
export type ExportSummary = { exported: number; renamed: number; failed: ExportFailure[] };

function isExportFailure(v: unknown): boolean {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return typeof o.source === "string" && typeof o.reason === "string";
}

// Tauri v2: JS camelCase `destDir` ↔ Rust snake_case `dest_dir`. Validated at the
// boundary like scanFolder — including each `failed[]` element — because Rust↔TS
// type sharing is still OPEN (no codegen yet).
export async function exportKeep(destDir: string): Promise<ExportSummary> {
  const raw = await invoke<unknown>("export_keep", { destDir });
  if (!raw || typeof raw !== "object") {
    throw new Error("export_keep returned an unexpected shape");
  }
  const o = raw as { exported?: unknown; renamed?: unknown; failed?: unknown };
  if (
    typeof o.exported !== "number" ||
    typeof o.renamed !== "number" ||
    !Array.isArray(o.failed) ||
    !o.failed.every(isExportFailure)
  ) {
    throw new Error("export_keep returned an unexpected shape");
  }
  // validated narrowing above
  return { exported: o.exported, renamed: o.renamed, failed: o.failed as ExportFailure[] };
}
