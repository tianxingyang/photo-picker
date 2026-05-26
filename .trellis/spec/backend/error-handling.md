# Error Handling

Three error boundaries:

1. **Inside Rust** — between modules.
2. **Rust → Frontend** — return value of `#[tauri::command]` functions.
3. **Python sidecar → Rust** — JSON-Lines `error` field.

---

## Rust Internal Errors — OPEN

> **DECISION pending**: error library and shape.
> - **Candidate A — `thiserror`**: one crate-wide enum `AppError` with typed variants (`Db(rusqlite::Error)`, `Sidecar(String)`, `Io(io::Error)`, ...). Pros: exhaustive pattern matching, stable shapes for the frontend. Cons: verbose `From` impls.
> - **Candidate B — `anyhow`**: every fallible function returns `anyhow::Result<T>`. Pros: terse. Cons: no exhaustive matching, weak when mapping to user-facing codes.
> - **Candidate C — hybrid**: `thiserror` `AppError` enum at the `commands/` boundary; `anyhow::Result` in deeper layers, converted at the boundary via `?`.
>
> Until decided, no module hardcodes a `String` error type. If blocked, use `Result<T, Box<dyn std::error::Error + Send + Sync>>` as a placeholder.

---

## Rust → Frontend

- Commands return `Result<T, AppError>` (final variant name pending OPEN above). Tauri serializes the error as JSON; the frontend receives `{ kind, message, detail? }`.
- The frontend switches on `kind`, never on free-text `message`. Adding a new branch requires adding a new variant first.
- Localized strings live in the frontend. Rust ships codes, not prose.

---

## Python Sidecar Errors

JSON-Lines response shape (from ARCHITECTURE.md §IPC):

```json
{ "id": 42, "error": "FileNotFoundError: C:/photos/IMG_001.jpg" }
```

- Python wraps every analyzer in `try/except Exception as e: return {"id": req_id, "error": f"{type(e).__name__}: {e}"}`. An exception MUST NOT kill the dispatch loop.
- Rust treats `error` as opaque text and maps it to `AppError::Sidecar(String)` (or hybrid equivalent). Failed analysis sets `photos.analysis_state = 'failed'` and records the raw text in `photos.analysis_error` (decoupled from the user-facing `status` enum); the dispatch loop keeps running.

---

## Sidecar Call Contract & Analysis Failure Semantics

> **Established 2026-05-24** (task `05-24-analysis-subsystem`). Refines "Python Sidecar Errors" above: the Rust side MUST tell a *per-file application error* apart from a *transport/infra failure* — they have **opposite** retry semantics, and conflating them marks every remaining photo permanently `failed` on a single sidecar crash.

### Signature

```rust
// sidecar/mod.rs
pub async fn call(&self, op: &str, payload: Value) -> Result<Result<Value, String>, SideErr>;
```

Two-level result — every caller MUST match all three arms:

| Result | Meaning | Persist / retry |
|--------|---------|-----------------|
| `Ok(Ok(value))` | op ran, returned a result | persist columns + `analysis_state='done'` |
| `Ok(Err(msg))` | op ran but Python returned `{id, error}` (bad/corrupt file) | persist `analysis_state='failed'` + `analysis_error=msg`; **NOT** retried |
| `Err(e)` | transport/infra failure: serialize, write, reader dropped, **timeout** | do **NOT** persist; row stays `pending` and is retried on the next run |

An `Ok(Ok(value))` whose shape fails `serde_json::from_value` is a per-file failure (persist `'failed'`), not a transport error.

### Batch command (`analyze_pending`)

- Selects only `analysis_state='pending'`; `'done'`/`'failed'` rows are skipped, so a re-run resumes pending work.
- Per-file failure → count `failed`, continue. Infra failure (`Err`) → `eprintln!` + **`break`**, then return the **partial** `AnalyzeSummary { analyzed, failed }`. Never lose completed progress; remaining rows stay `pending` for retry.
- **Single-flight**: guarded by an `AtomicBool` in `AppState` with an RAII reset on *every* exit path (`?`, `break`, return, panic). A concurrent second call returns an empty summary `{ analyzed: 0, failed: 0 }` rather than double-processing the same set.

### IPC stream integrity (Python side)

`main.py` MUST serialize responses with `allow_nan=False` plus a valid-JSON fallback (this runs *outside* `handle()`'s try/except, so it needs its own guard):

```python
try:
    out = json.dumps(resp, allow_nan=False)
except (ValueError, TypeError) as e:
    out = json.dumps({"id": resp.get("id"), "error": f"unserializable result: {e}"})
```

**Why**: default `allow_nan=True` emits bare `NaN`/`Infinity` tokens — invalid JSON the Rust reader cannot parse, which leaves the pending call hanging the full `CALL_TIMEOUT`.

### Wrong vs Correct

```rust
// WRONG — a dead/timed-out sidecar is recorded as a per-file failure, so
// every remaining photo is marked 'failed' and never retried.
let result = match sidecar.call("analyze", payload).await {
    Ok(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
    Err(e) => Err(e.to_string()),
};
persist_analysis(&conn, id, result)?;

// CORRECT — transport error leaves the row pending and stops the batch.
match sidecar.call("analyze", payload).await {
    Ok(Ok(v))      => /* deserialize → persist 'done' (or 'failed' on bad shape) */,
    Ok(Err(op_err)) => /* persist 'failed' + analysis_error */,
    Err(transport) => return Err(AppError::Sidecar(transport.to_string())), // no persist
}
```

### Tests required (assertion points)
- `persist_analysis` Ok → analysis columns set, `analysis_state='done'`, `analysis_error` cleared.
- `persist_analysis` Err → `analysis_state='failed'` + `analysis_error` set, analysis columns untouched.
- Pending query returns only `'pending'` rows (excludes `'done'`/`'failed'`).

---

## Error Propagation Rules

- `?` is the default. `match` an error only when the next step depends on the variant.
- No swallowing with `let _ =`. If intentional, a one-line `// why` comment is required (this is one of the rare cases where a comment IS necessary).
- Logging an error MUST NOT replace returning it. Log at the boundary that decides "user-visible vs retry-internally".

---

## API Error Responses

Tauri-serialized variant shape, frontend-facing contract:

```ts
type AppErrorPayload =
  | { kind: "Db"; message: string }
  | { kind: "Sidecar"; message: string }
  | { kind: "Io"; message: string }
  | { kind: "NotFound"; message: string }
  | { kind: "Validation"; message: string };
// The final union mirrors the chosen Rust enum once the OPEN above is resolved.
```

**`Validation` (established 2026-05-26, task `05-24-keep-reject-status`)** — the canonical kind
for **command-boundary input/enum validation** (e.g. `set_status` rejects a status
outside `'pending'|'keep'|'reject'`). Use it for client/argument errors that are
neither a DB fault, a missing entity (`NotFound`), nor infra (`Io`/`Sidecar`).

> **Paired contract.** `error.rs::AppError` and `src/types/ipc.ts` (`AppErrorPayload`
> union **and** the `KINDS` array) MUST change together. The frontend's
> `describeAppError` degrades gracefully on an unknown `kind` (keeps the `message`),
> so a missed sync won't crash — but it silently drops `kind`-based branching. Adding
> a new variant on only one side is a contract bug, not a safe no-op.

---

## Common Mistakes

- Returning `Result<T, String>` from commands — loses structured matching on the frontend.
- `unwrap()` on `mutex.lock()` in async commands — locks are infallible-by-API but `unwrap` hides poisoning across panic recovery.
- Crashing the sidecar dispatch loop on a single bad payload (must per-request `try`).
- Translating Rust error variants in Rust (i18n belongs to the frontend).
