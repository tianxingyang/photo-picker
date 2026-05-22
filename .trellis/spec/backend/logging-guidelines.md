# Logging Guidelines

Cross-process logging: Rust main + Python sidecar. Stdout is reserved for IPC payloads — **all** Python logs go to stderr.

---

## Library — OPEN

> **DECISION pending** for Rust logging library:
> - **Candidate A — `tracing` + `tracing-subscriber`**: structured, span-aware, plays well with `tokio`. Recommended default.
> - **Candidate B — `log` + `env_logger`**: smaller dependency footprint, no spans.
>
> Python sidecar uses stdlib `logging`, configured to write **stderr only**.

---

## Log Levels

| Level | When to use |
|---|---|
| `error` | Unrecoverable per-operation failure that the user sees. |
| `warn`  | Recoverable issue (retried, fell back, skipped one file). |
| `info`  | Lifecycle events (scan started/finished, sidecar spawned/exited, batch summaries). |
| `debug` | Per-record traces (one log per photo is `debug`, not `info`). |
| `trace` | IPC frame dumps, dev only, behind a feature flag. |

---

## Structured Logging

Required fields when applicable:

- `photo_id` — blake3 hex of the photo path.
- `op` — IPC op name (`blur`, `phash`, ...).
- `path` — absolute path (`info` and above; `debug` may include).
- `duration_ms` — for any analysis/scan operation.

Format: structured records (key=value or JSON), not free-text prose. Sub-agents emitting log lines should always include the relevant ids over a sentence summary.

---

## What to Log

- Sidecar lifecycle: spawn, ready, exit (with code), restart.
- DB migration: each migration applied, with version.
- Per-batch summary at `info`: counts (`scanned=N analyzed=M failed=K duration_ms=...`).
- Errors: full chain (`tracing::error!(error = ?e, photo_id, op)` or equivalent).

---

## What NOT to Log

- The IPC stdout/stdin stream itself (frame dumps live at `trace` behind a feature flag).
- User file contents or EXIF metadata at `info`+ (path is OK; the photo bytes are not).
- Absolute home-directory paths in release builds unless `debug`+ — privacy.
- Per-record `info` events in hot loops (use `debug`, summarize at `info`).

---

## Output Sinks

- Rust dev build: stderr.
- Rust release build: rolling file under `<app_data_dir>/logs/photo-picker.log` (size-rotated, e.g. 5 × 10 MB).
- Python: stderr, always. The Rust sidecar manager pipes Python stderr into the same Rust logger at `debug`.
