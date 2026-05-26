import { create } from "zustand";

import { useGroupsStore } from "./groupsStore";
import type { PhotoId } from "../types/photo";

type CompareState = {
  open: boolean;
  /** Ordered member ids for the filmstrip (current group, or [id] if ungrouped). */
  memberIds: PhotoId[];
  /** Left pane — the photo the user clicked to open. */
  aId: PhotoId | null;
  /** Right pane — next group member; null if the photo has no group peers. */
  bId: PhotoId | null;

  /**
   * Open the viewer for the given photo id. Reads groupsStore.getState() to
   * locate the owning group: A = clicked photo, B = the next member in the
   * group; ungrouped photos open with B = null (no peers to compare).
   */
  openFor: (photoId: PhotoId) => void;

  /** Set the right-pane (B) photo by id. Resets view transform (handled by viewer). */
  setB: (id: PhotoId) => void;

  /** Swap A and B. */
  swap: () => void;

  /**
   * Step the B pane forward (+1) or backward (−1) through memberIds, skipping
   * the id currently in A so A is never compared to itself.
   */
  stepB: (dir: 1 | -1) => void;

  /** Close the overlay. */
  close: () => void;
};

export const useCompareStore = create<CompareState>((set, get) => ({
  open: false,
  memberIds: [],
  aId: null,
  bId: null,

  openFor: (photoId) => {
    // Cross-store read: locate the group that contains photoId. Zustand's
    // getState() is the approved cross-store access pattern (no hook, no
    // subscription — just a direct snapshot read at action time).
    const { groups, ungroupedIds } = useGroupsStore.getState();

    let memberIds: PhotoId[] = [];
    const group = groups.find((g) => g.photoIds.includes(photoId));

    if (group) {
      memberIds = group.photoIds;
    } else if (ungroupedIds.includes(photoId)) {
      memberIds = [photoId];
    } else {
      // Photo not yet in the store (race with load) — open with just this photo.
      memberIds = [photoId];
    }

    // B = the next member after A (wraps if A is last). If the group has only
    // one member, B = null (no peers).
    const aIdx = memberIds.indexOf(photoId);
    const bId = memberIds.length > 1 ? (memberIds[(aIdx + 1) % memberIds.length] ?? null) : null;

    set({ open: true, memberIds, aId: photoId, bId });
  },

  setB: (id) => {
    set({ bId: id });
  },

  swap: () => {
    const { aId, bId } = get();
    if (aId !== null && bId !== null) {
      set({ aId: bId, bId: aId });
    }
  },

  stepB: (dir) => {
    const { memberIds, aId, bId } = get();
    if (memberIds.length <= 1) return;

    const startIdx = bId !== null ? memberIds.indexOf(bId) : 0;
    const n = memberIds.length;

    // Walk in the given direction, skipping whichever id is currently in A.
    let idx = startIdx;
    for (let i = 0; i < n; i++) {
      idx = (((idx + dir) % n) + n) % n;
      const candidate = memberIds[idx];
      if (candidate !== undefined && candidate !== aId) {
        set({ bId: candidate });
        return;
      }
    }
    // No suitable candidate (e.g., only A is left) — leave B unchanged.
  },

  close: () => {
    set({ open: false });
  },
}));
