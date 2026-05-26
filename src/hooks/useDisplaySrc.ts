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
  // Non-HEIC photos are always ready — return a stable object using a ref so
  // callers wrapped in React.memo don't re-render on every call.
  const stableReady = useRef<DisplaySrcState>({ state: "ready", src: photo.src });

  const [heicState, setHeicState] = useState<DisplaySrcState>(() => {
    if (!photo.isHeic) return { state: "ready", src: photo.src };
    const cached = transcodeCache.get(photo.id);
    return cached !== undefined ? { state: "ready", src: cached } : { state: "loading" };
  });

  useEffect(() => {
    if (!photo.isHeic) return;

    // Cache hit — nothing to do; initial state was already set from the cache.
    const cached = transcodeCache.get(photo.id);
    if (cached !== undefined) {
      setHeicState({ state: "ready", src: cached });
      return;
    }

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

  if (!photo.isHeic) return stableReady.current;
  return heicState;
}
