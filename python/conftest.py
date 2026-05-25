"""Ensure the `python/` dir is importable so tests can `from analyzers import ...`.

Mirrors how the sidecar runs main.py with cwd=python/, where `analyzers` is a
top-level package.
"""

from __future__ import annotations

import sys
from pathlib import Path

_ROOT = Path(__file__).parent
if str(_ROOT) not in sys.path:
    sys.path.insert(0, str(_ROOT))
