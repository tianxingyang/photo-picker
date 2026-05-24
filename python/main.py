"""Photo Picker sidecar entry point.

Reads JSON-Lines requests from stdin, writes responses to stdout.
All logs go to stderr — stdout is reserved for IPC payloads.
"""

from __future__ import annotations

import json
import sys
from collections.abc import Callable
from typing import Any

from analyzers import analyze, echo

OPS: dict[str, Callable[[dict], dict]] = {
    "echo": echo.run,
    "analyze": analyze.run,
}


def handle(line: str) -> dict[str, Any]:
    req_id: Any = None
    try:
        req = json.loads(line)
        req_id = req.get("id")
        op = req.get("op")
        payload = req.get("payload", {}) or {}

        fn = OPS.get(op)
        if fn is None:
            return {"id": req_id, "error": f"unknown op: {op}"}

        return {"id": req_id, "result": fn(payload)}
    except Exception as e:
        # why: keep the sidecar alive on any single-line failure
        return {"id": req_id, "error": f"{type(e).__name__}: {e}"}


def main() -> None:
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        resp = handle(line)
        # Serialize defensively: a non-finite float (NaN/Infinity) would produce
        # a bare token that the Rust JSON reader can't parse, causing a 30s hang.
        try:
            out = json.dumps(resp, allow_nan=False)
        except (ValueError, TypeError) as e:
            out = json.dumps({"id": resp.get("id"), "error": f"unserializable result: {e}"})
        sys.stdout.write(out + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
