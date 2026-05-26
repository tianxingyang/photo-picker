"""Tests for the transcode op (HEIC / JPEG / PNG → display JPEG).

All fixtures are generated in-process so no real/copyrighted photos are needed.
"""

from __future__ import annotations

import os

import pillow_heif
import pytest
from PIL import Image

from analyzers import transcode


def _rgb_image(width: int = 256, height: int = 256) -> Image.Image:
    """Solid mid-gray RGB image."""
    return Image.new("RGB", (width, height), color=(128, 128, 128))


def _save_jpeg(img: Image.Image, path) -> str:
    img.save(str(path), format="JPEG", quality=85)
    return str(path)


def _save_png(img: Image.Image, path) -> str:
    img.save(str(path), format="PNG")
    return str(path)


# ---------------------------------------------------------------------------
# Basic JPEG → JPEG round-trip
# ---------------------------------------------------------------------------


def test_jpeg_to_jpeg_round_trip(tmp_path):
    src = _save_jpeg(_rgb_image(), tmp_path / "src.jpg")
    dest = str(tmp_path / "out.jpg")
    result = transcode.run({"path": src, "dest": dest})
    assert result["dest"] == dest
    assert os.path.isfile(dest)
    out = Image.open(dest)
    assert out.format == "JPEG"
    assert result["width"] == out.width
    assert result["height"] == out.height


# ---------------------------------------------------------------------------
# PNG with alpha → RGB JPEG (no crash on mode conversion)
# ---------------------------------------------------------------------------


def test_rgba_png_to_jpeg(tmp_path):
    rgba = Image.new("RGBA", (128, 128), color=(200, 100, 50, 180))
    src = str(tmp_path / "rgba.png")
    rgba.save(src, format="PNG")
    dest = str(tmp_path / "out.jpg")
    result = transcode.run({"path": src, "dest": dest})
    assert os.path.isfile(dest)
    out = Image.open(dest)
    assert out.format == "JPEG"
    assert out.mode == "RGB"
    assert result["width"] == 128
    assert result["height"] == 128


# ---------------------------------------------------------------------------
# maxSide downscaling
# ---------------------------------------------------------------------------


def test_large_image_is_downscaled(tmp_path):
    src = _save_png(_rgb_image(800, 600), tmp_path / "large.png")
    dest = str(tmp_path / "small.jpg")
    result = transcode.run({"path": src, "dest": dest, "maxSide": 400})
    assert result["width"] <= 400
    assert result["height"] <= 400
    # Aspect ratio preserved: 800x600 at maxSide=400 -> 400x300
    assert result["width"] == 400
    assert result["height"] == 300


def test_portrait_image_downscale_uses_height(tmp_path):
    src = _save_png(_rgb_image(300, 900), tmp_path / "portrait.png")
    dest = str(tmp_path / "out.jpg")
    result = transcode.run({"path": src, "dest": dest, "maxSide": 450})
    assert result["height"] == 450
    assert result["width"] == 150  # 300 * (450/900)


def test_small_image_not_upscaled(tmp_path):
    """Images already within maxSide must not be enlarged."""
    src = _save_png(_rgb_image(100, 100), tmp_path / "small.png")
    dest = str(tmp_path / "out.jpg")
    result = transcode.run({"path": src, "dest": dest, "maxSide": 4096})
    assert result["width"] == 100
    assert result["height"] == 100


# ---------------------------------------------------------------------------
# maxSide clamping to at least 1
# ---------------------------------------------------------------------------


def test_maxside_zero_clamped_to_one(tmp_path):
    src = _save_png(_rgb_image(50, 50), tmp_path / "src.png")
    dest = str(tmp_path / "out.jpg")
    # maxSide=0 should clamp to 1, not crash or produce 0-size image.
    result = transcode.run({"path": src, "dest": dest, "maxSide": 0})
    assert result["width"] >= 1
    assert result["height"] >= 1
    assert os.path.isfile(dest)


# ---------------------------------------------------------------------------
# HEIC encode/decode (skipped if HEIC encode is unavailable)
# ---------------------------------------------------------------------------


def test_heic_to_jpeg(tmp_path):
    pillow_heif.register_heif_opener()
    src = _rgb_image(256, 256)
    heic_path = tmp_path / "img.heic"
    try:
        src.save(str(heic_path), format="HEIF")
    except Exception as e:
        pytest.skip(f"HEIC encode unavailable in this environment: {e}")

    dest = str(tmp_path / "out.jpg")
    result = transcode.run({"path": str(heic_path), "dest": dest})
    assert os.path.isfile(dest)
    out = Image.open(dest)
    assert out.format == "JPEG"
    assert result["width"] == 256
    assert result["height"] == 256


def test_heic_downscale_to_maxside(tmp_path):
    pillow_heif.register_heif_opener()
    src = _rgb_image(800, 600)
    heic_path = tmp_path / "large.heic"
    try:
        src.save(str(heic_path), format="HEIF")
    except Exception as e:
        pytest.skip(f"HEIC encode unavailable in this environment: {e}")

    dest = str(tmp_path / "out.jpg")
    result = transcode.run({"path": str(heic_path), "dest": dest, "maxSide": 400})
    assert result["width"] == 400
    assert result["height"] == 300


# ---------------------------------------------------------------------------
# Failure path — bad path must raise (main.handle wraps it into {error})
# ---------------------------------------------------------------------------


def test_bad_path_raises(tmp_path):
    with pytest.raises(Exception):  # noqa: B017 — any decode/IO failure must propagate
        transcode.run(
            {"path": str(tmp_path / "nonexistent.jpg"), "dest": str(tmp_path / "out.jpg")}
        )


# ---------------------------------------------------------------------------
# Atomic write: dest dir created automatically
# ---------------------------------------------------------------------------


def test_dest_directory_created_automatically(tmp_path):
    src = _save_jpeg(_rgb_image(), tmp_path / "src.jpg")
    deep_dest = str(tmp_path / "sub" / "nested" / "out.jpg")
    result = transcode.run({"path": src, "dest": deep_dest})
    assert os.path.isfile(deep_dest)
    assert result["dest"] == deep_dest


# ---------------------------------------------------------------------------
# Result dimensions match saved file
# ---------------------------------------------------------------------------


def test_result_dimensions_match_file(tmp_path):
    src = _save_png(_rgb_image(320, 240), tmp_path / "src.png")
    dest = str(tmp_path / "out.jpg")
    result = transcode.run({"path": src, "dest": dest})
    opened = Image.open(dest)
    assert result["width"] == opened.width
    assert result["height"] == opened.height
