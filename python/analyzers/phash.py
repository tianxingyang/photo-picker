"""Perceptual hash (pHash) of a PIL image.

Pure function; raises on bad input. hash_size=8 => 64-bit => 16 hex chars,
consumed downstream by similar-grouping (Hamming distance clustering).
"""

from __future__ import annotations

import imagehash
from PIL.Image import Image


def compute(img: Image) -> str:
    """Return the 16-hex-char pHash string."""
    return str(imagehash.phash(img, hash_size=8))
