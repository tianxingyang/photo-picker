import { create } from "zustand";

import type { Photo, PhotoId } from "../types/photo";

type PhotosState = {
  byId: Record<PhotoId, Photo>;
  order: PhotoId[];
  addPhotos: (photos: Photo[]) => void;
  clear: () => void;
};

export const usePhotosStore = create<PhotosState>((set) => ({
  byId: {},
  order: [],
  addPhotos: (photos) =>
    set((s) => {
      const byId = { ...s.byId };
      const order = [...s.order];
      for (const p of photos) {
        if (!(p.id in byId)) order.push(p.id);
        byId[p.id] = p;
      }
      return { byId, order };
    }),
  clear: () => set({ byId: {}, order: [] }),
}));
