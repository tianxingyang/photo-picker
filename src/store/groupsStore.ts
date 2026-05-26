import { create } from "zustand";

import { listGroups } from "../api/groupsApi";
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
  // (state-management.md). Persisting belongs to the keep-reject-status task;
  // until its `set_status` command lands this only updates local state.
  setStatus: async (id, status) => {
    const prev = get().byId[id];
    if (!prev) return;
    set((s) => ({ byId: { ...s.byId, [id]: { ...prev, status } } }));
    // TODO(keep-reject-status): invoke `set_status` then roll back on failure:
    //   try { await setPhotoStatus(id, status); }
    //   catch (e) { set((s) => ({ byId: { ...s.byId, [id]: prev } })); throw e; }
  },

  clear: () => set({ byId: {}, groups: [], ungroupedIds: [], loaded: false }),
}));
