"""Single `analyze` op: decode once, run blur/exposure/phash/exif, merge.

run(payload) is the IPC entry point. It does NOT catch exceptions — main.handle()
wraps any failure into {id, error} and keeps the dispatch loop alive. No stdout
writes here (that would corrupt the IPC stream); logging, if any, goes to stderr.
"""

from __future__ import annotations

import numpy as np
import pillow_heif
from PIL import Image

from . import blur, constants, exposure, phash
from .exif import extract_shot_at

# why: must run at import time so Image.open transparently decodes HEIC/HEIF.
pillow_heif.register_heif_opener()


def run(payload: dict) -> dict:
    path = payload["path"]
    # Allow threshold overrides via payload (reserved slot); fall back to defaults.
    blur_thr = float(payload.get("blurVarThreshold", constants.BLUR_VAR_THRESHOLD))
    over_mean = float(payload.get("overMean", constants.OVER_MEAN))
    under_mean = float(payload.get("underMean", constants.UNDER_MEAN))
    clip_ratio = float(payload.get("clipRatio", constants.CLIP_RATIO))
    max_side = int(payload.get("normMaxSide", constants.NORM_MAX_SIDE))

    with Image.open(path) as img:
        img.load()

        shot_at = extract_shot_at(img)
        gray = _to_gray_ndarray(img, max_side)
        blur_score, is_blurry = blur.score(gray, blur_thr)
        exposure_score, exposure_flag = exposure.score(gray, over_mean, under_mean, clip_ratio)
        phash_hex = phash.compute(img)

    return {
        "shotAt": shot_at,
        "blurScore": blur_score,
        "isBlurry": is_blurry,
        "exposureScore": exposure_score,
        "exposureFlag": exposure_flag,
        "phash": phash_hex,
    }


def _to_gray_ndarray(img: Image.Image, max_side: int) -> np.ndarray:
    """Downscale so the longest side <= max_side, convert to 8-bit grayscale.

    Resizing first keeps blur_score comparable across source resolutions.
    """
    resized = img
    longest = max(img.width, img.height)
    if longest > max_side and longest > 0:
        scale = max_side / longest
        new_size = (max(1, round(img.width * scale)), max(1, round(img.height * scale)))
        resized = img.resize(new_size, Image.Resampling.BILINEAR)

    gray = resized.convert("L")
    return np.asarray(gray, dtype=np.uint8)
