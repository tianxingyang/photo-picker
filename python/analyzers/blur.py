"""Blur detection via Laplacian variance.

Operates on a normalized grayscale ndarray (caller resizes first so the score
is comparable across resolutions). Pure function; raises on bad input.
"""

from __future__ import annotations

import numpy as np

from . import constants

# 3x3 Laplacian kernel (8-neighbour).
_LAPLACIAN = np.array(
    [
        [0.0, 1.0, 0.0],
        [1.0, -4.0, 1.0],
        [0.0, 1.0, 0.0],
    ],
    dtype=np.float64,
)


def score(gray: np.ndarray, threshold: float = constants.BLUR_VAR_THRESHOLD) -> tuple[float, bool]:
    """Return (blur_score, is_blurry).

    blur_score is the variance of the Laplacian: high for sharp images, low for
    blurry ones. is_blurry = blur_score < threshold.
    """
    response = _convolve2d_valid(gray.astype(np.float64), _LAPLACIAN)
    var = float(response.var())
    return var, var < threshold


def _convolve2d_valid(img: np.ndarray, kernel: np.ndarray) -> np.ndarray:
    """2D valid-mode convolution without scipy.

    Builds shifted views over the interior region and accumulates the kernel
    weights — small fixed 3x3 kernel, so this is cheap and dependency-free.
    """
    kh, kw = kernel.shape
    h, w = img.shape
    if h < kh or w < kw:
        # Degenerate tiny image: no interior — return a single zero so var()==0.
        return np.zeros((1, 1), dtype=np.float64)

    out_h = h - kh + 1
    out_w = w - kw + 1
    out = np.zeros((out_h, out_w), dtype=np.float64)
    for i in range(kh):
        for j in range(kw):
            out += kernel[i, j] * img[i : i + out_h, j : j + out_w]
    return out
