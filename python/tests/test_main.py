"""Integration test for the sidecar dispatch loop (`main.py`).

Spawns the real sidecar process, feeds mixed JSON-Lines requests over stdin, and
asserts id-keyed responses come back. The sidecar is single-threaded and
synchronous (parallelism comes from Rust running a POOL of these processes — in
-process Python threading is unstable under piped stdio on Windows); these tests
pin that it handles a batch, survives a malformed line and a bad op, and exits
on stdin close.
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

_PYTHON_DIR = Path(__file__).resolve().parent.parent
_MAIN = _PYTHON_DIR / "main.py"


def _run_sidecar(lines: list[str], expected: int, timeout: float = 60.0) -> dict:
    """Pipe `lines` into a fresh sidecar, return {id: response} once `expected`
    responses arrive (or the process exits). Closes stdin to end the loop. The
    sidecar is single-threaded and synchronous — Rust runs a POOL of these
    processes for parallelism; each process here handles its lines serially."""
    proc = subprocess.Popen(
        [sys.executable, str(_MAIN)],
        cwd=str(_PYTHON_DIR),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        encoding="utf-8",
    )
    assert proc.stdin is not None and proc.stdout is not None
    try:
        stdout, _ = proc.communicate("".join(f"{line}\n" for line in lines), timeout=timeout)
    finally:
        # why: on a communicate() timeout (a wedged sidecar — exactly the
        # failure these tests guard) the process would otherwise leak across
        # the CI run. Kill and reap it.
        if proc.poll() is None:
            proc.kill()
            proc.communicate()

    by_id: dict = {}
    for raw in stdout.splitlines():
        raw = raw.strip()
        if not raw:
            continue
        resp = json.loads(raw)
        by_id[resp.get("id")] = resp
    assert len(by_id) >= expected, f"expected >= {expected} responses, got {by_id}"
    return by_id


def test_loop_handles_inline_and_errors_without_dying():
    requests = [
        json.dumps({"id": 1, "op": "echo", "payload": {"text": "hi"}}),
        json.dumps({"id": 2, "op": "bogus", "payload": {}}),
        "this is not json",  # malformed → {id: null, error}
        json.dumps({"id": 4, "op": "analyze", "payload": {"path": "does_not_exist_xyz.jpg"}}),
        json.dumps({"id": 5, "op": "echo", "payload": {"text": "after"}}),
    ]
    # 5 ids: 1, 2, null, 4, 5
    by_id = _run_sidecar(requests, expected=5)

    # Inline echo round-trips (loop is alive at start and after the errors).
    assert by_id[1]["result"] == {"text": "hi"}
    assert by_id[5]["result"] == {"text": "after"}

    # Unknown op → error, loop survives.
    assert "error" in by_id[2]
    assert "unknown op" in by_id[2]["error"]

    # Malformed line → {id: null, error}, loop survives.
    assert by_id[None]["error"]

    # analyze on a missing file → per-file {id, error} (NOT a crash), proving
    # the synchronous handle() path reports per-file failures and keeps looping.
    assert "error" in by_id[4]


def test_concurrent_analyze_failures_all_return():
    # Several bad-path analyze requests must each come back (id-keyed), proving
    # the serial loop drains every queued line and loses no response.
    n = 6
    requests = [
        json.dumps({"id": i, "op": "analyze", "payload": {"path": f"missing_{i}.jpg"}})
        for i in range(n)
    ]
    by_id = _run_sidecar(requests, expected=n)
    for i in range(n):
        assert "error" in by_id[i], f"id {i} missing/ok unexpectedly: {by_id.get(i)}"


# Keep an explicit marker so a hung pool fails fast rather than blocking CI.
def test_sidecar_exits_on_stdin_close():
    start = time.monotonic()
    _run_sidecar([json.dumps({"id": 1, "op": "echo", "payload": {"text": "x"}})], expected=1)
    assert time.monotonic() - start < 60.0
