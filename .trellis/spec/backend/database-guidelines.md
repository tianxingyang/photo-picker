# Database Guidelines

SQLite via `rusqlite` (bundled). WAL mode. File location: `<app_data_dir>/photo-picker.db`.

---

## Overview

- `rusqlite` with the `bundled` feature (no system sqlite dependency, predictable across platforms).
- Connections opened with `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON` once at boot.
- Blocking work goes through `tokio::task::spawn_blocking`. The runtime MUST NOT hold a `Connection` across `.await`.
- User original files are never written. The DB tracks paths + analysis; the user's photos stay read-only on disk.

---

## Query Patterns

- Prepared statements (`conn.prepare_cached`) for any query run in a loop (scanner, batch analysis).
- Multi-row writes wrapped in a transaction. For >100 inserts, single tx + single prepared statement.
- Never `format!`-build SQL with untrusted input. Use `?` placeholders.
- Reads return owned structs, not borrowed `Row<'_>` — collect inside the closure passed to `query_map`.

---

## Migrations

- Plain `.sql` files in `src-tauri/migrations/`, lex-ordered (`0001_*.sql`, `0002_*.sql`).
- Applied at startup against `PRAGMA user_version`. One file = one schema version bump.
- Forward-only. No down-migrations. A failed migration aborts boot.

---

## Naming Conventions

- Tables: `snake_case`, plural (`photos`, `similar_groups`).
- Columns: `snake_case`. Timestamp columns suffixed `_at`, ISO-8601 strings (e.g. `shot_at`, `imported_at`).
- Primary keys: `id` (TEXT carrying the blake3 hex digest where applicable).
- Indexes: `idx_<table>_<cols>` (e.g. `idx_photos_shot_at`).

---

## Schema Shape — RESOLVED (M1: wide table)

> **DECIDED 2026-05-24** (task `05-24-analysis-subsystem`, D1): **Candidate A — wide table**. Analysis columns live directly on `photos` (`shot_at / blur_score / is_blurry / exposure_score / exposure_flag / phash`), plus an independent `analysis_state` (`pending|done|failed`) + `analysis_error` to track the analysis lifecycle without overloading `status`. Future heavy/sparse ops (M3 CLIP embedding, faces) go to their own tables, not the wide row. Migration `0002_analysis.sql`.
>
> Original options (kept for context):
> - **Candidate A — wide table**: one row per photo, analysis columns added per op. Pros: joinless reads. Cons: `ALTER TABLE` on every new op; null sparsity.
> - **Candidate B — tall table**: `photos` + `photo_analyses(photo_id, op, result_json)`. Pros: ops extend without schema change. Cons: every read joins.
> - **Candidate C — hybrid**: hot columns (`status`, `shot_at`, `blur_score`, `exposure_score`, `phash`) wide on `photos`; future ops tall on `photo_analyses`. Pros: hot path fast, future extensible. Cons: two patterns coexist.
>
> Sub-agents MUST NOT pick on their own. Surface the choice in a task PRD/design before implementing schema.

Related OPEN items (deferred to feature tasks): status column encoding (TEXT vs INT), thumbnail resolution tiers.

---

## Common Mistakes

- Holding `Connection` in a `tauri::State` and calling it from an async command without `spawn_blocking` → blocks the Tauri event loop.
- Returning `rusqlite::Row` borrows across function boundaries (lifetime traps); always collect into owned structs.
- Mutating original files (e.g. updating EXIF). Read-only invariant.
