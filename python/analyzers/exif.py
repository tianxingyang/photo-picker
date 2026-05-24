"""EXIF shot-time extraction.

Pure function. Does not catch exceptions and does not write stdout — the
dispatch loop in main.py wraps failures into {id, error}.
"""

from __future__ import annotations

import datetime

from PIL.Image import Image

# EXIF tag ids (avoids depending on PIL.ExifTags name tables).
_DATE_TIME_ORIGINAL = 0x9003  # in Exif sub-IFD (pointed to by 0x8769)
_DATE_TIME = 0x0132  # in IFD0
_EXIF_IFD_POINTER = 0x8769  # tag whose value is the offset of the Exif sub-IFD


def extract_shot_at(img: Image) -> str | None:
    """Return capture time as ISO-8601 ("YYYY-MM-DDTHH:MM:SS"), or None.

    Prefers DateTimeOriginal (0x9003) from the Exif sub-IFD, falls back to
    DateTime (0x0132) from IFD0. Both EXIF string values and raw bytes are
    accepted (some readers return bytes). No EXIF / unparsable value => None
    (not an error).
    """
    exif = img.getexif()
    if not exif:
        return None

    # DateTimeOriginal lives in the Exif sub-IFD, NOT in IFD0.
    raw = None
    sub_ifd = exif.get_ifd(_EXIF_IFD_POINTER)
    if sub_ifd:
        raw = sub_ifd.get(_DATE_TIME_ORIGINAL)

    # Fall back to DateTime which IS in IFD0.
    if not raw:
        raw = exif.get(_DATE_TIME)

    if not raw:
        return None

    # Some readers return bytes instead of str; decode and strip trailing NUL.
    if isinstance(raw, bytes):
        raw = raw.decode("ascii", errors="replace").rstrip("\x00")

    if not isinstance(raw, str):
        return None

    return _to_iso8601(raw)


def _to_iso8601(raw: str) -> str | None:
    """Parse "YYYY:MM:DD HH:MM:SS" into ISO-8601 "YYYY-MM-DDTHH:MM:SS".

    Uses strptime to validate: an unset camera clock ("0000:00:00 00:00:00")
    or any other malformed value returns None rather than an invalid string.
    """
    try:
        dt = datetime.datetime.strptime(raw.strip(), "%Y:%m:%d %H:%M:%S")
        return dt.isoformat()
    except ValueError:
        return None
