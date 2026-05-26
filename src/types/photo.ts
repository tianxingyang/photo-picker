type Brand<T, B> = T & { __brand: B };

export type PhotoId = Brand<string, "PhotoId">;
export type PhotoSrc = Brand<string, "PhotoSrc">; // returned by convertFileSrc

export type PhotoStatus = "pending" | "keep" | "reject";

// why: component/store view — raw OS path intentionally not exposed (see
// frontend/directory-structure.md). `name` + `src` are derived in api/.
export type Photo = {
  id: PhotoId;
  name: string;
  status: PhotoStatus;
  src: PhotoSrc;
};

export type ExposureFlag = "normal" | "over" | "under";
export type AnalysisState = "pending" | "done" | "failed";

// Photo enriched with analysis fields for the group-browse view. Mirrors the
// Rust `BrowsePhoto` DTO (commands/grouping.rs); validated/derived in api/.
export type BrowsePhoto = {
  id: PhotoId;
  name: string;
  src: PhotoSrc;
  isHeic: boolean; // browsers can't decode HEIC -> show a placeholder tile
  status: PhotoStatus;
  shotAt: string | null;
  blurScore: number | null;
  isBlurry: boolean | null;
  exposureFlag: ExposureFlag | null;
  analysisState: AnalysisState;
};
