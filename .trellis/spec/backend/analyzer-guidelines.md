# Analyzer Guidelines (Python sidecar)

Conventions for `python/analyzers/*`. Each analyzer is a pure function over a decoded `PIL.Image` or a normalized grayscale ndarray. Analyzers **raise** on bad input (the dispatch loop in `main.py` wraps the failure into `{id, error}`) and **never write stdout** — stdout is the IPC stream; logs go to stderr.

> **Established 2026-05-24** (task `05-24-analysis-subsystem`).

---

## Gotcha: EXIF DateTimeOriginal lives in the Exif sub-IFD, not IFD0

> **Warning**: `Image.getexif()` returns only the top-level **IFD0**. `DateTimeOriginal` (`0x9003`) and the other capture-time tags live in the **Exif sub-IFD** pointed to by tag `0x8769`. `getexif().get(0x9003)` returns `None` for virtually all real camera/phone photos — only `DateTime` (`0x0132`) is in IFD0.

**Correct**:

```python
exif = img.getexif()
sub = exif.get_ifd(0x8769)                 # Exif sub-IFD; returns {} if absent
raw = sub.get(0x9003) or exif.get(0x0132)  # DateTimeOriginal, fall back to DateTime
```

- Accept `bytes` (decode ASCII, strip trailing `\x00`) as well as `str`.
- **Validate** with `datetime.strptime(raw.strip(), "%Y:%m:%d %H:%M:%S")`, return `dt.isoformat()`; return `None` on `ValueError`. This rejects the unset-clock sentinel `"0000:00:00 00:00:00"` (month `00` is invalid) instead of persisting a bogus `"0000-00-00T00:00:00"` into `shot_at`. Sub-second / timezone are separate tags (`0x9291` / `0x9011`) never appended to `0x9003`, so strptime does not reject valid values.

**Test point**: write the fixture's `0x9003` into the **sub-IFD** (`exif.get_ifd(0x8769)[0x9003] = ...`) before saving. Assigning it to the flat top-level dict round-trips through a *buggy* reader and gives false confidence.

---

## Convention: normalize to grayscale before resizing

**What**: in the decode → normalize path, `convert("L")` **before** `resize(...)`, and only ever **downscale** (longest side → `NORM_MAX_SIDE`), never upscale.

```python
gray_img = img.convert("L")
if max(gray_img.size) > max_side:          # max_side = max(1, int(override))
    gray_img = gray_img.resize(new_size, Image.Resampling.BILINEAR)
arr = np.asarray(gray_img, dtype=np.uint8)
```

**Why**:
- Resizing a palette (`'P'`) or CMYK image *before* converting interpolates raw palette indices / channel values → garbage luminance (observed ~2.5× `blur_score` divergence). Convert to `'L'` first so interpolation happens in luminance space.
- Downscaling large images to a common longest side keeps `blur_score` (Laplacian variance) comparable across high-resolution sources. Upscaling a small image adds no detail, so images already `≤ NORM_MAX_SIDE` are kept native (and are **not** mutually comparable — the docstring must not claim otherwise).
- Clamp the max-side override to `max(1, int(...))` so a `0`/negative value cannot collapse the image to 1×1.

---

## Pure-function edge cases

- Degenerate inputs must not crash. `exposure.score` returns `(0.0, "normal")` for a zero-size array; `blur` returns `0.0` (→ flagged blurry) for images smaller than the 3×3 Laplacian kernel — these are intentional, documented defaults, not silent failures.
- Threshold comparisons that flag at a boundary use `>=` (e.g. clip fraction `>= clip_ratio`) so an exactly-at-threshold image is flagged rather than missed.
