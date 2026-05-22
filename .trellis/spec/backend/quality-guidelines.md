# Quality Guidelines (Backend)

Covers Rust main process and Python sidecar.

---

## Formatting

- **Rust** — `rustfmt.toml` is source of truth: edition 2021, max width 100, 4-space indent, Unix newline, `use_field_init_shorthand`, `use_try_shorthand`, `reorder_imports`. Run `cargo fmt --check` before commit.
- **Python** — `python/ruff.toml` is source of truth: line length 100, target py310, double-quote, space indent, LF. Run `uvx ruff format --check python/` and `uvx ruff check python/`.
- **Editor defaults** — `.editorconfig` already coordinates UTF-8, LF, trim trailing whitespace, 2-space (4 for `.rs` / `.py`), 100 max line length for `.rs` / `.py`.

---

## Lint Rules

- **Rust** — `cargo clippy --workspace --all-targets -- -D warnings`. No `#[allow(...)]` without a one-line `// why` comment on the same line.
- **Python** — ruff `select = [E, F, W, I, B, UP, SIM, RUF]`, `ignore = [E501]`. Per-file `# noqa: XYZ` requires a reason on the same line.

---

## Forbidden Patterns

- `unwrap()` / `expect()` in production code paths. Allowed in tests, build scripts, and "this absolutely cannot fail" cases — the latter requires a `// why` comment.
- `println!` in Rust prod code. Use the logging library. Allowed in `examples/` or one-shot CLI tools.
- `print()` in Python prod code — **breaks IPC** (corrupts stdout). Use `logging` writing to stderr.
- Blocking I/O inside async commands without `spawn_blocking`.
- Schema changes without a migration file.
- `unsafe` Rust without a `// SAFETY:` comment justifying the invariant.

---

## Required Patterns

- Every new `#[tauri::command]` returns `Result<T, AppError>` (or whatever the chosen error type name maps to — see error-handling.md).
- Every new IPC `op` is documented in ARCHITECTURE.md §IPC before merge.
- Every new migration is exercised against a fresh DB AND an existing DB at the previous schema version.
- Every async function holding a DB `Connection` does so inside `spawn_blocking` or via a typed accessor that internally `spawn_blocking`s.

---

## Comments and Docs

- "非必要不形成" — code names carry meaning; comments explain *why*, never *what*.
- A comment IS necessary when it documents one of: an invariant (`// must be called before sidecar spawn`), a non-obvious workaround (`// libheif crashes on truncated …`), a `SAFETY:` justification, or a `# why-allow` for a lint exception.
- No doc comments on `pub fn` unless the function is part of a stable external API. Today: nothing is.

---

## Testing Requirements

- **Rust unit tests** — `#[cfg(test)] mod tests` next to the module under test.
- **Rust integration tests** — `src-tauri/tests/` for command handlers; use in-memory rusqlite (`:memory:`).
- **Python** — `pytest` under `python/tests/`. Analyzers tested against fixture images committed to `python/tests/fixtures/`.
- Tests do NOT spawn the real Python sidecar from Rust; sidecar tests use a fake child process emitting canned JSON-Lines responses.

---

## Code Review Checklist

- [ ] New analysis op? `op` documented + analyzer module + Rust dispatch + test for both.
- [ ] DB touched? Migration committed? Forward-only?
- [ ] Any blocking resource (`Connection`, large `Vec`) held across `.await`?
- [ ] New error variant — does the frontend handle the `kind`?
- [ ] Log volume sane at `info` (no per-record info logs)?
- [ ] `cargo fmt --check` + `cargo clippy -D warnings` clean? `uvx ruff format/check` clean?
