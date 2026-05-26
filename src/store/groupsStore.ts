import { create } from "zustand";

import { listGroups } from "../api/groupsApi";
import { setPhotoStatus } from "../api/photosApi";
import type { BrowseGroup } from "../types/group";
import type { BrowsePhoto, PhotoId, PhotoStatus } from "../types/photo";

type GroupsState = {
  byId: Record<PhotoId, BrowsePhoto>;
  groups: BrowseGroup[];
  ungroupedIds: PhotoId[];
  loaded: boolean;
  load: () => Promise<void>;
  setStatus: (id: PhotoId, status: PhotoStatus) => Promise<void>;
  clear: () => void;
};

export const useGroupsStore = create<GroupsState>((set, get) => ({
  byId: {},
  groups: [],
  ungroupedIds: [],
  loaded: false,

  load: async () => {
    const { byId, groups, ungroupedIds } = await listGroups();
    set({ byId, groups, ungroupedIds, loaded: true });
  },

  // Optimistic: write the store first so the status pill flips instantly
  // (state-management.md), then persist; roll back to `prev` and rethrow on
  // failure so the caller's `.catch` can react.
  setStatus: async (id, status) => {
    const prev = get().byId[id];
    if (!prev) return;
    set((s) => ({ byId: { ...s.byId, [id]: { ...prev, status } } })); // optimistic
    try {
      await setPhotoStatus(id, status);
    } catch (e) {
      set((s) => ({ byId: { ...s.byId, [id]: prev } })); // rollback
      throw e;
    }
  },

  clear: () => set({ byId: {}, groups: [], ungroupedIds: [], loaded: false }),
}));
