"""EXIF shot-time extraction.

Pure function. Does not catch exceptions and does not write stdout — the
dispatch loop in main.py wraps failures into {id, error}.
"""

from __future__ import annotations

from PIL.Image import Image

# EXIF tag ids (avoids depending on PIL.ExifTags name tables).
_DATE_TIME_ORIGINAL = 0x9003
_DATE_TIME = 0x0132


def extract_shot_at(img: Image) -> str | None:
    """Return capture time as ISO-8601 ("YYYY-MM-DDTHH:MM:SS"), or None.

    Prefers DateTimeOriginal (0x9003), falls back to DateTime (0x0132).
    EXIF stores "YYYY:MM:DD HH:MM:SS"; we rewrite the date separators and the
    space into ISO form. No EXIF / unparsable value => None (not an error).
    """
    exif = img.getexif()
    if not exif:
        return None

    raw = exif.get(_DATE_TIME_ORIGINAL) or exif.get(_DATE_TIME)
    if not raw or not isinstance(raw, str):
        return None

    return _to_iso8601(raw)


def _to_iso8601(raw: str) -> str | None:
    raw = raw.strip()
    parts = raw.split(" ")
    if len(parts) != 2:
        return None
    date_part, time_part = parts
    date_fields = date_part.split(":")
    if len(date_fields) != 3:
        return None
    return f"{date_fields[0]}-{date_fields[1]}-{date_fields[2]}T{time_part}"
