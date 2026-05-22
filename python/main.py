"""Photo Picker sidecar entry point.

Reads JSON-Lines requests from stdin, writes responses to stdout.
All logs go to stderr — stdout is reserved for IPC payloads.
"""

from __future__ import annotations

import json
import sys
from collections.abc import Callable
from typing import Any

from analyzers import echo

OPS: dict[str, Callable[[dict], dict]] = {
    "echo": echo.run,
}


def handle(line: str) -> dict[str, Any]:
    req = json.loads(line)
    req_id = req.get("id")
    op = req.get("op")
    payload = req.get("payload", {}) or {}

    fn = OPS.get(op)
    if fn is None:
        return {"id": req_id, "error": f"unknown op: {op}"}

    try:
        return {"id": req_id, "result": fn(payload)}
    except Exception as e:
        return {"id": req_id, "error": f"{type(e).__name__}: {e}"}


def main() -> None:
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        resp = handle(line)
        sys.stdout.write(json.dumps(resp) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
