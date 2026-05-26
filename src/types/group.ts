import type { PhotoId } from "./photo";

// A similar-group as the browse UI consumes it: an id plus the ordered member
// ids (photo data itself lives in the store's `byId`, keyed by PhotoId).
export type BrowseGroup = {
  id: string;
  photoIds: PhotoId[];
};
