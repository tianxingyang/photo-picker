import { useState } from "react";

import type { BrowsePhoto } from "../../types/photo";
import { HeicPlaceholder } from "./HeicPlaceholder";

type ThumbImageProps = {
  photo: BrowsePhoto;
  /** Tailwind classes for the <img>. Defaults to a square cover fill. */
  className?: string;
};

/** Render a photo's pre-generated thumbnail with graceful degradation, shared by
 *  the grid tile and the compare filmstrip:
 *   - thumbnail ready  -> the 512px WebP (cheap to decode, even at 64px);
 *   - not ready / HEIC  -> the full-resolution original (non-HEIC) or the HEIC
 *     text placeholder, exactly as before thumbnails existed.
 *  The displayed source is DERIVED from props (`thumbSrc`, `isHeic`) so it tracks
 *  a thumbnail that becomes ready on the next browse refresh; `<img onError>`
 *  steps down (thumb -> original -> placeholder) if a file is missing on disk. */
export function ThumbImage({ photo, className = "h-full w-full object-cover" }: ThumbImageProps) {
  const [thumbFailed, setThumbFailed] = useState(false);
  const [originalFailed, setOriginalFailed] = useState(false);

  const useThumb = photo.thumbSrc !== null && !thumbFailed;
  if (useThumb) {
    return (
      <img
        src={photo.thumbSrc as string}
        alt={photo.name}
        loading="lazy"
        decoding="async"
        className={className}
        onError={() => setThumbFailed(true)}
      />
    );
  }

  // No usable thumbnail: HEIC can't decode in the webview -> placeholder.
  if (photo.isHeic || originalFailed) {
    return <HeicPlaceholder name={photo.name} />;
  }

  return (
    <img
      src={photo.src}
      alt={photo.name}
      loading="lazy"
      decoding="async"
      className={className}
      onError={() => setOriginalFailed(true)}
    />
  );
}
