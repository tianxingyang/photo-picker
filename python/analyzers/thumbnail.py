"""Thumbnail op: decode an image (incl. HEIC) to a small WebP thumbnail.

run(payload) is the IPC entry point.
  payload fields:
    path    str  — source file path (any format PIL can open, incl. HEIC)
    dest    str  — absolute destination path for the output WebP
    maxSide int  — longest-side cap (default 512); only downscales, never up
    quality int  — WebP quality 1..100 (default 80)

Returns: {"dest": dest, "width": w, "height": h} (dimensions of saved WebP).

Does NOT catch exceptions — main.handle() wraps any failure into {id, error}
and keeps the dispatch loop alive. Does not write stdout.
"""

from __future__ import annotations

import os

import pillow_heif
from PIL import Image, ImageOps

# why: must run before any Image.open so HEIC/HEIF thumbnails decode. Idempotent —
# safe even if analyze.py / transcode.py already registered the opener.
pillow_heif.register_heif_opener()

THUMB_MAX_SIDE = 512
WEBP_QUALITY = 80


def run(payload: dict) -> dict:
    src = payload["path"]
    dest = payload["dest"]
    max_side = max(1, int(payload.get("maxSide", THUMB_MAX_SIDE)))
    quality = max(1, min(100, int(payload.get("quality", WEBP_QUALITY))))

    with Image.open(src) as img:
        # why: browsers auto-rotate originals per EXIF orientation, but a re-encoded
        # WebP drops EXIF metadata — bake the rotation in so the thumbnail matches
        # the original's on-screen orientation (otherwise rotated photos lie sideways).
        img = ImageOps.exif_transpose(img)
        # Convert to RGB so WebP encoding is always safe (handles RGBA, P, CMYK).
        img = img.convert("RGB")

        longest = max(img.width, img.height)
        if longest > max_side:
            scale = max_side / longest
            new_w = max(1, round(img.width * scale))
            new_h = max(1, round(img.height * scale))
            img = img.resize((new_w, new_h), Image.Resampling.BILINEAR)

        out_w, out_h = img.width, img.height

        # Write to a .part file first, then atomic-rename to dest so the cache
        # never contains a half-written WebP that a concurrent reader would pick up.
        tmp = dest + ".part"
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        img.save(tmp, format="WEBP", quality=quality, method=6)
        os.replace(tmp, dest)

    return {"dest": dest, "width": out_w, "height": out_h}
