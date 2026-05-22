"""Echo analyzer — returns the input text verbatim.

Smoke test for the IPC plumbing. Removed once real analyzers land.
"""

from __future__ import annotations


def run(payload: dict) -> dict:
    return {"text": payload.get("text", "")}
