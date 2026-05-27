import { useEffect, useRef, useState } from "react";

import { transcodeForDisplay } from "../api/displayApi";
import type { BrowsePhoto, PhotoId, PhotoSrc } from "../types/photo";

type DisplaySrcState =
  | { state: "loading" }
  | { state: "ready"; src: PhotoSrc }
  | { state: "error"; message: string };

// Module-level cache: once a HEIC has been transcoded and its asset URL
// returned, we reuse the URL for the session lifetime. This avoids redundant
// invoke round-trips when the same photo is mounted in multiple panes or the
// filmstrip.
const transcodeCache = new Map<PhotoId, PhotoSrc>();

/**
 * Resolve a displayable `PhotoSrc` for any photo type.
 *
 * - Non-HEIC: returns the photo's existing `src` synchronously (state = "ready"
 *   from the very first render, no async round-trip needed).
 * - HEIC: calls `transcodeForDisplay` once per unique `PhotoId`; subsequent
 *   mounts hit the module-level cache without issuing a new invoke.
 *
 * Returns a discriminated-union object so callers can render a loading skeleton
 * or error fallback without branching on undefined/null.
 */
export function useDisplaySrc(photo: BrowsePhoto): DisplaySrcState {
  const [heicState, setHeicState] = useState<DisplaySrcState>(() => {
    if (!photo.isHeic) return { state: "ready", src: photo.src };
    const cached = transcodeCache.get(photo.id);
    return cached !== undefined ? { state: "ready", src: cached } : { state: "loading" };
  });

  // why: this hook instance is reused when the pane's photo changes (the pane is
  // NOT remounted), and useState's initializer runs only on first mount. Without
  // a render-phase reset, the first render after photo.id changes would return
  // the PREVIOUS photo's resolved heicState — painting the old image under the
  // new id for one frame (the effect below resets state only AFTER paint).
  // Setting state during render, guarded by the id ref, makes React discard that
  // stale render before commit, so the wrong image never reaches the screen.
  const prevIdRef = useRef(photo.id);
  if (prevIdRef.current !== photo.id) {
    prevIdRef.current = photo.id;
    if (photo.isHeic) {
      const cached = transcodeCache.get(photo.id);
      setHeicState(cached !== undefined ? { state: "ready", src: cached } : { state: "loading" });
    }
  }

  useEffect(() => {
    if (!photo.isHeic) return;

    // Cache hit — show it immediately.
    const cached = transcodeCache.get(photo.id);
    if (cached !== undefined) {
      setHeicState({ state: "ready", src: cached });
      return;
    }

    // State was already reset during render by the photo-id guard above (to a
    // cache hit or to "loading"), so here we only kick off the async transcode.
    let cancelled = false;
    transcodeForDisplay(photo.id)
      .then((src) => {
        if (cancelled) return;
        transcodeCache.set(photo.id, src);
        setHeicState({ state: "ready", src });
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        const message =
          e instanceof Error ? e.message : typeof e === "string" ? e : "transcode failed";
        setHeicState({ state: "error", message });
      });

    return () => {
      cancelled = true;
    };
    // photo.id and photo.isHeic are stable for a given photo; re-run only when
    // the photo itself changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [photo.id, photo.isHeic]);

  // Non-HEIC: compute fresh each render so switching between non-HEIC photos
  // (filmstrip navigation) reflects the current photo's src, not a stale one.
  if (!photo.isHeic) return { state: "ready", src: photo.src };
  return heicState;
}
