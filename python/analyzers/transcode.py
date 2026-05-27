"""Transcode op: decode an image (incl. HEIC) to a display-ready JPEG.

run(payload) is the IPC entry point.
  payload fields:
    path    str  — source file path (any format PIL can open, incl. HEIC)
    dest    str  — absolute destination path for the output JPEG
    maxSide int  — longest-side cap (default 4096); only downscales, never up

Returns: {"dest": dest, "width": w, "height": h} (dimensions of saved JPEG).

Does NOT catch exceptions — main.handle() wraps any failure into {id, error}
and keeps the dispatch loop alive. Does not write stdout.
"""

from __future__ import annotations

import os

import pillow_heif
from PIL import Image

# why: must run before any Image.open so HEIC/HEIF files are transparently
# decoded. Idempotent — safe to call at import time even if analyze.py already
# called it.
pillow_heif.register_heif_opener()

DISPLAY_MAX_SIDE = 4096
JPEG_QUALITY = 90


def run(payload: dict) -> dict:
    src = payload["path"]
    dest = payload["dest"]
    max_side = max(1, int(payload.get("maxSide", DISPLAY_MAX_SIDE)))

    with Image.open(src) as img:
        # Convert to RGB so JPEG encoding is always safe (handles RGBA, P,
        # CMYK, HDR modes that JPEG cannot represent).
        img = img.convert("RGB")

        longest = max(img.width, img.height)
        if longest > max_side:
            scale = max_side / longest
            new_w = max(1, round(img.width * scale))
            new_h = max(1, round(img.height * scale))
            img = img.resize((new_w, new_h), Image.Resampling.BILINEAR)

        out_w, out_h = img.width, img.height

        # Write to a .part file first, then atomic-rename to dest so the cache
        # never contains a half-written JPEG that a concurrent reader would pick up.
        tmp = dest + ".part"
        # Ensure the destination directory exists before writing.
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        img.save(tmp, format="JPEG", quality=JPEG_QUALITY)
        os.replace(tmp, dest)

    return {"dest": dest, "width": out_w, "height": out_h}
