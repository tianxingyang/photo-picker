"""Exposure flagging via normalized grayscale mean + highlight/shadow clipping.

Operates on a normalized grayscale ndarray. Pure function; raises on bad input.
"""

from __future__ import annotations

import numpy as np

from . import constants

# 8-bit grayscale extremes considered "clipped".
_BRIGHT_CLIP = 250
_DARK_CLIP = 5


def score(
    gray: np.ndarray,
    over_mean: float = constants.OVER_MEAN,
    under_mean: float = constants.UNDER_MEAN,
    clip_ratio: float = constants.CLIP_RATIO,
) -> tuple[float, str]:
    """Return (exposure_score, exposure_flag).

    exposure_score is the grayscale mean normalized to [0, 1]. The flag is:
      - "over"  if mean > over_mean OR bright-clipped fraction >= clip_ratio
      - "under" if mean < under_mean OR dark-clipped fraction >= clip_ratio
      - "normal" otherwise
    """
    if gray.size == 0:
        # Degenerate empty array: no meaningful exposure to measure.
        return 0.0, "normal"

    g = gray.astype(np.float64)
    mean_norm = float(g.mean()) / 255.0

    total = g.size
    bright_frac = float(np.count_nonzero(g >= _BRIGHT_CLIP)) / total
    dark_frac = float(np.count_nonzero(g <= _DARK_CLIP)) / total

    if mean_norm > over_mean or bright_frac >= clip_ratio:
        flag = "over"
    elif mean_norm < under_mean or dark_frac >= clip_ratio:
        flag = "under"
    else:
        flag = "normal"

    return mean_norm, flag
