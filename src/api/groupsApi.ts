import { convertFileSrc, invoke } from "@tauri-apps/api/core";

import type { BrowseGroup } from "../types/group";
import type {
  AnalysisState,
  BrowsePhoto,
  ExposureFlag,
  PhotoId,
  PhotoSrc,
  PhotoStatus,
} from "../types/photo";

// Raw row from Rust `list_groups` (serde camelCase). Validated at the boundary
// because Rust↔TS type sharing is still OPEN (no codegen yet).
type BrowsePhotoRaw = {
  id: string;
  path: string;
  status: string;
  shotAt: string | null;
  isBlurry: boolean | null;
  blurScore: number | null;
  exposureFlag: string | null;
  analysisState: string;
};

const STATUSES: readonly string[] = ["pending", "keep", "reject"];
const EXPOSURES: readonly string[] = ["normal", "over", "under"];
const ANALYSIS_STATES: readonly string[] = ["pending", "done", "failed"];

function isStringOrNull(v: unknown): v is string | null {
  return v === null || typeof v === "string";
}

function isBrowsePhotoRaw(v: unknown): v is BrowsePhotoRaw {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.id === "string" &&
    typeof o.path === "string" &&
    typeof o.status === "string" &&
    isStringOrNull(o.shotAt) &&
    (o.isBlurry === null || typeof o.isBlurry === "boolean") &&
    (o.blurScore === null || typeof o.blurScore === "number") &&
    isStringOrNull(o.exposureFlag) &&
    typeof o.analysisState === "string"
  );
}

function basename(path: string): string {
  // why: stored paths are OS-native; split on both separators for the leaf.
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

function toBrowsePhoto(raw: BrowsePhotoRaw): BrowsePhoto {
  const name = basename(raw.path);
  const status: PhotoStatus = STATUSES.includes(raw.status)
    ? (raw.status as PhotoStatus)
    : "pending";
  const exposureFlag: ExposureFlag | null =
    raw.exposureFlag !== null && EXPOSURES.includes(raw.exposureFlag)
      ? (raw.exposureFlag as ExposureFlag)
      : null;
  const analysisState: AnalysisState = ANALYSIS_STATES.includes(raw.analysisState)
    ? (raw.analysisState as AnalysisState)
    : "pending";
  return {
    id: raw.id as PhotoId,
    name,
    src: convertFileSrc(raw.path) as PhotoSrc,
    isHeic: /\.hei[cf]$/i.test(name),
    status,
    shotAt: raw.shotAt,
    blurScore: raw.blurScore,
    isBlurry: raw.isBlurry,
    exposureFlag,
    analysisState,
  };
}

export type BrowseModelView = {
  byId: Record<PhotoId, BrowsePhoto>;
  groups: BrowseGroup[];
  ungroupedIds: PhotoId[];
};

function collect(rawPhotos: unknown, byId: Record<PhotoId, BrowsePhoto>): PhotoId[] {
  if (!Array.isArray(rawPhotos) || !rawPhotos.every(isBrowsePhotoRaw)) {
    throw new Error("list_groups returned an unexpected shape");
  }
  const ids: PhotoId[] = [];
  for (const raw of rawPhotos) {
    const photo = toBrowsePhoto(raw);
    byId[photo.id] = photo;
    ids.push(photo.id);
  }
  return ids;
}

export async function listGroups(): Promise<BrowseModelView> {
  const raw = await invoke<unknown>("list_groups");
  if (!raw || typeof raw !== "object") {
    throw new Error("list_groups returned an unexpected shape");
  }
  const o = raw as { groups?: unknown; ungrouped?: unknown };
  if (!Array.isArray(o.groups)) {
    throw new Error("list_groups returned an unexpected shape");
  }

  const byId: Record<PhotoId, BrowsePhoto> = {};
  const groups: BrowseGroup[] = [];
  for (const g of o.groups) {
    if (!g || typeof g !== "object") {
      throw new Error("list_groups returned an unexpected shape");
    }
    const group = g as { id?: unknown; photos?: unknown };
    if (typeof group.id !== "string") {
      throw new Error("list_groups returned an unexpected shape");
    }
    groups.push({ id: group.id, photoIds: collect(group.photos, byId) });
  }
  const ungroupedIds = collect(o.ungrouped, byId);

  return { byId, groups, ungroupedIds };
}

/** Re-cluster analysed photos into similar groups (Rust `group_photos`). */
export async function groupPhotos(): Promise<void> {
  await invoke("group_photos");
}
