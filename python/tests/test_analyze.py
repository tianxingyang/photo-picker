"""Analyzer tests with synthetic fixtures (no real/copyrighted photos).

Fixtures are generated in-process and written to a tmp dir so the decode-once
path (analyze.run reading from disk) is exercised end to end.
"""

from __future__ import annotations

import numpy as np
import pillow_heif
import pytest
from PIL import Image

from analyzers import analyze


def _checkerboard(size: int = 256, cell: int = 8) -> Image.Image:
    """High-frequency checkerboard => sharp, high Laplacian variance."""
    rows = (np.arange(size) // cell) % 2
    cols = (np.arange(size) // cell) % 2
    board = (rows[:, None] ^ cols[None, :]).astype(np.uint8) * 255
    rgb = np.stack([board, board, board], axis=-1)
    return Image.fromarray(rgb, mode="RGB")


def _gaussian_blur(img: Image.Image, radius: float = 6.0) -> Image.Image:
    from PIL import ImageFilter

    return img.filter(ImageFilter.GaussianBlur(radius=radius))


def _solid(value: int, size: int = 64) -> Image.Image:
    arr = np.full((size, size, 3), value, dtype=np.uint8)
    return Image.fromarray(arr, mode="RGB")


def _gradient(size: int = 256) -> Image.Image:
    """Smooth left-to-right luminance ramp — a structurally distinct image."""
    row = np.linspace(0, 255, size, dtype=np.uint8)
    arr = np.tile(row, (size, 1))
    rgb = np.stack([arr, arr, arr], axis=-1)
    return Image.fromarray(rgb, mode="RGB")


def _save(img: Image.Image, path, fmt: str = "PNG", **kwargs) -> str:
    img.save(path, format=fmt, **kwargs)
    return str(path)


# --- blur ---------------------------------------------------------------


def test_sharp_scores_higher_than_blurred(tmp_path):
    sharp = _save(_checkerboard(), tmp_path / "sharp.png")
    blurred = _save(_gaussian_blur(_checkerboard()), tmp_path / "blurred.png")

    sharp_res = analyze.run({"path": sharp})
    blurred_res = analyze.run({"path": blurred})

    assert sharp_res["blurScore"] > blurred_res["blurScore"]
    # Obvious gaussian blur on a checkerboard must trip the blurry flag.
    assert blurred_res["isBlurry"] is True
    # A crisp checkerboard must not be flagged blurry.
    assert sharp_res["isBlurry"] is False


def test_blur_score_stable_across_resolution(tmp_path):
    # Same source image at two resolutions, BOTH above NORM_MAX_SIDE, so the
    # normalize-to-longest-side step brings them back to comparable scores.
    # Note: sub-1024 images are intentionally not upscaled; this test only
    # covers the >max_side normalization path.
    base = _checkerboard(size=2048, cell=64)
    native = _save(base, tmp_path / "native.png")
    downscaled = _save(
        base.resize((1280, 1280), Image.Resampling.BILINEAR),
        tmp_path / "downscaled.png",
    )

    native_score = analyze.run({"path": native})["blurScore"]
    down_score = analyze.run({"path": downscaled})["blurScore"]

    # After normalize-to-max-side, scores should be within a modest ratio.
    ratio = max(native_score, down_score) / max(1e-9, min(native_score, down_score))
    assert ratio < 2.0, f"blur_score drifted across resolution: {native_score} vs {down_score}"


# --- exposure -----------------------------------------------------------


def test_white_is_over(tmp_path):
    path = _save(_solid(255), tmp_path / "white.png")
    res = analyze.run({"path": path})
    assert res["exposureFlag"] == "over"
    assert res["exposureScore"] > 0.9


def test_black_is_under(tmp_path):
    path = _save(_solid(0), tmp_path / "black.png")
    res = analyze.run({"path": path})
    assert res["exposureFlag"] == "under"
    assert res["exposureScore"] < 0.1


def test_mid_gray_is_normal(tmp_path):
    path = _save(_solid(128), tmp_path / "gray.png")
    res = analyze.run({"path": path})
    assert res["exposureFlag"] == "normal"
    assert 0.4 < res["exposureScore"] < 0.6


# --- exif ---------------------------------------------------------------


def test_exif_datetime_original_parsed(tmp_path):
    """DateTimeOriginal (0x9003) must be read from the Exif sub-IFD (0x8769).

    Writing it only into the sub-IFD is the path that real camera/phone JPEGs
    follow and that the old IFD0-only getexif().get(0x9003) misses.
    """
    img = _checkerboard()
    exif = img.getexif()
    # Write 0x9003 into the Exif sub-IFD (pointed to by tag 0x8769) — NOT IFD0.
    sub_ifd = exif.get_ifd(0x8769)
    sub_ifd[0x9003] = "2026:05:24 10:30:00"
    path = tmp_path / "with_exif.jpg"
    img.save(path, format="JPEG", exif=exif)

    res = analyze.run({"path": str(path)})
    assert res["shotAt"] == "2026-05-24T10:30:00"


def test_no_exif_returns_null(tmp_path):
    path = _save(_checkerboard(), tmp_path / "no_exif.png")
    res = analyze.run({"path": path})
    assert res["shotAt"] is None


def test_exif_all_zero_datetime_returns_null(tmp_path):
    """An unset camera clock ("0000:00:00 00:00:00") must yield shotAt=None.

    strptime rejects month=00, so the invalid date is filtered rather than
    persisted as "0000-00-00T00:00:00".
    """
    img = _checkerboard()
    exif = img.getexif()
    sub_ifd = exif.get_ifd(0x8769)
    sub_ifd[0x9003] = "0000:00:00 00:00:00"
    path = tmp_path / "zero_exif.jpg"
    img.save(path, format="JPEG", exif=exif)

    res = analyze.run({"path": str(path)})
    assert res["shotAt"] is None


def test_palette_image_gives_finite_blur_score(tmp_path):
    """A palette ('P') image larger than NORM_MAX_SIDE must yield a sane blurScore.

    The old code resized in palette-index space before converting to 'L', which
    produced garbage pixel values. Converting to 'L' first (F3 fix) makes the
    score comparable to processing the same image as 'L' directly.
    """
    from analyzers import constants

    # Build an image bigger than NORM_MAX_SIDE so the resize path is exercised.
    size = constants.NORM_MAX_SIDE + 128
    rgb_img = _checkerboard(size=size, cell=16)
    palette_img = rgb_img.convert("P")

    p_path = tmp_path / "palette.png"
    l_path = tmp_path / "luminance.png"
    palette_img.save(p_path, format="PNG")
    rgb_img.convert("L").save(l_path, format="PNG")

    res_p = analyze.run({"path": str(p_path)})
    res_l = analyze.run({"path": str(l_path)})

    # Both must produce a finite, positive blurScore.
    assert isinstance(res_p["blurScore"], float)
    assert res_p["blurScore"] > 0
    # The palette-mode result should be within a reasonable factor of the
    # direct-grayscale result (not orders of magnitude off).
    ratio = max(res_p["blurScore"], res_l["blurScore"]) / max(
        1e-9, min(res_p["blurScore"], res_l["blurScore"])
    )
    assert ratio < 4.0, (
        f"palette blurScore diverged from L-mode: {res_p['blurScore']} vs {res_l['blurScore']}"
    )


# --- phash --------------------------------------------------------------


def test_identical_images_equal_phash(tmp_path):
    a = _save(_checkerboard(), tmp_path / "a.png")
    b = _save(_checkerboard(), tmp_path / "b.png")
    assert analyze.run({"path": a})["phash"] == analyze.run({"path": b})["phash"]


def test_different_images_unequal_phash(tmp_path):
    board = _save(_checkerboard(), tmp_path / "board.png")
    # A smooth gradient is structurally distinct from a checkerboard; a flat
    # solid would be degenerate (all DCT coeffs equal => collides with other
    # low-frequency images), so use a textured-but-different image instead.
    gradient = _save(_gradient(), tmp_path / "gradient.png")
    assert analyze.run({"path": board})["phash"] != analyze.run({"path": gradient})["phash"]


def test_phash_is_16_hex_chars(tmp_path):
    path = _save(_checkerboard(), tmp_path / "h.png")
    phash_hex = analyze.run({"path": path})["phash"]
    assert len(phash_hex) == 16
    assert all(c in "0123456789abcdef" for c in phash_hex)


# --- HEIC ---------------------------------------------------------------


def test_heic_decodes_to_full_result(tmp_path):
    pillow_heif.register_heif_opener()
    src = _checkerboard()
    heic_path = tmp_path / "img.heic"
    try:
        src.save(heic_path, format="HEIF")
    except Exception as e:  # encode support is environment-dependent
        pytest.skip(f"HEIC encode unavailable in this environment: {e}")

    res = analyze.run({"path": str(heic_path)})
    assert set(res.keys()) == {
        "shotAt",
        "blurScore",
        "isBlurry",
        "exposureScore",
        "exposureFlag",
        "phash",
    }
    assert isinstance(res["blurScore"], float)
    assert len(res["phash"]) == 16


# --- failure path -------------------------------------------------------


def test_bad_path_raises(tmp_path):
    # analyze.run must NOT swallow errors; main.handle() wraps them into {error}.
    with pytest.raises(Exception):  # noqa: B017 — any decode/IO failure must propagate
        analyze.run({"path": str(tmp_path / "does_not_exist.jpg")})
