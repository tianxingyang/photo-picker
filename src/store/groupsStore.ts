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

// Per-id single-flight for status persistence. The store always shows the
// latest click (optimistic, instant); the DB write is serialized per photo so
// writes land in click order and a failed write reverts to the last *persisted*
// value — never a stale snapshot that could clobber a newer click or resurrect
// a row a concurrent load() removed. Transient on purpose: kept out of Zustand
// state so it never triggers a re-render.
const desiredStatus = new Map<PhotoId, PhotoStatus>();
const writing = new Set<PhotoId>();

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
  // (state-management.md). Persistence is single-flight per id: a burst of
  // clicks coalesces to the latest intent, writes land in click order. Both the
  // success and failure paths re-assert only the status field — success to the
  // persisted target (so a concurrent load() that overwrote the optimistic write
  // can't leave the UI stale), failure to the last *persisted* value — each
  // guarded so it neither clobbers a newer click nor resurrects a row a
  // concurrent load() removed.
  setStatus: async (id, status) => {
    const cur = get().byId[id];
    if (!cur) return;
    set((s) => ({ byId: { ...s.byId, [id]: { ...cur, status } } })); // optimistic
    desiredStatus.set(id, status);
    if (writing.has(id)) return; // a runner is already draining desiredStatus[id]
    writing.add(id);

    let target = status;
    let baseline = cur.status; // last persisted value when this burst started
    try {
      for (;;) {
        await setPhotoStatus(id, target);
        baseline = target; // target is now persisted
        const latest = desiredStatus.get(id);
        if (latest == null || latest === target) {
          // A concurrent load() may have replaced byId with pre-write DB data
          // while this write was in flight, leaving the UI stale on the success
          // path. Re-assert the persisted value (sync from await to break, so
          // `target` is still the latest intent). Guarded: never resurrect a
          // load()-removed row, and skip a no-op write.
          set((s) => {
            const c = s.byId[id];
            if (!c || c.status === target) return s;
            return { byId: { ...s.byId, [id]: { ...c, status: target } } };
          });
          break;
        }
        target = latest; // a newer click landed mid-write — persist it too
      }
    } catch (e) {
      set((s) => {
        const c = s.byId[id];
        if (!c) return s; // row removed by a concurrent load() — don't resurrect
        return { byId: { ...s.byId, [id]: { ...c, status: baseline } } }; // revert status only
      });
      throw e;
    } finally {
      writing.delete(id);
      desiredStatus.delete(id);
    }
  },

  clear: () => set({ byId: {}, groups: [], ungroupedIds: [], loaded: false }),
}));
