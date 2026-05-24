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
  | { kind: "NotFound"; message: string };
// The final union mirrors the chosen Rust enum once the OPEN above is resolved.
```

---

## Common Mistakes

- Returning `Result<T, String>` from commands — loses structured matching on the frontend.
- `unwrap()` on `mutex.lock()` in async commands — locks are infallible-by-API but `unwrap` hides poisoning across panic recovery.
- Crashing the sidecar dispatch loop on a single bad payload (must per-request `try`).
- Translating Rust error variants in Rust (i18n belongs to the frontend).
