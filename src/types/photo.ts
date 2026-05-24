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
