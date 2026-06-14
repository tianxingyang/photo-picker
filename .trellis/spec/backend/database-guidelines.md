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
- **One destructive exception — `0004_projects.sql`**: SQLite cannot `ALTER` a primary key or drop a column's `UNIQUE` in place, and project-isolation rewrote `photos`' PK derivation + added `project_id`. Because that task discarded all pre-existing (dev-only) data by decision, `0004` `DROP`s + recreates `photos` / `similar_groups` / `group_members` (drop dependents before dependencies) instead of carrying data. This is the lone migration that destroys rows; a rollback past it cannot be done in place (delete the DB file). Normal migrations stay additive.

---

## Naming Conventions

- Tables: `snake_case`, plural (`photos`, `similar_groups`).
- Columns: `snake_case`. Timestamp columns suffixed `_at`, ISO-8601 strings (e.g. `shot_at`, `imported_at`).
- Primary keys: `id` (TEXT). Content/identity-derived where applicable: `photos.id = blake3(project_id + "\n" + path)`, `similar_groups.id = blake3(method + "\n" + sorted_member_ids)`; `projects.id` is a UUID v4.
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

### Grouping model — RESOLVED (M1: multi-method many-to-many)

> **DECIDED 2026-05-25** (task `05-24-similar-grouping`). Near-duplicate / future semantic grouping is **derived data** stored in two tables (migration `0003_grouping.sql`, `user_version` → 3):
>
> - `similar_groups(id TEXT PK, method TEXT, params TEXT)` — group entity. `method` tags the algorithm (M1 writes `'phash_burst'`); `params` is the algorithm-parameter JSON (e.g. `{"threshold":8,"version":1}`). **No `created_at`**: a per-run timestamp on a stateless, fully-rebuilt, content-id'd cache has no consumer and would be denormalized (same value on every row of a run); it also breaks byte-idempotency. Add a time field with deliberate semantics only when a consumer needs it.
> - `group_members(group_id, photo_id, PRIMARY KEY(group_id, photo_id))` — many-to-many junction, both FKs `ON DELETE CASCADE`. A photo may belong to multiple groups across methods. Indexes `idx_group_members_photo`, `idx_similar_groups_method`.
>
> Rationale: per the wide-table rule this "heavy/sparse" derived data lives in its own tables, NOT on the `photos` wide row. Group `id` is content-derived (`blake3(method + "\n" + sorted_member_ids)`) and the row carries no timestamp, so each `(id, method, params)` row is a pure function of its membership — re-running the grouping command is byte-idempotent (delete-then-reinsert per method). M3 (CLIP / faces) reuses these two tables with new `method` values + per-method commands — **zero schema migration**.

### Project isolation — RESOLVED (M2: single DB, `project_id` column)

> **DECIDED 2026-06-14** (task `06-13-project-isolation`, `user_version` → 4, migration `0004_projects.sql`). The app moved from a single global workspace to **project-scoped** picking sessions: photos imported into one project never appear in another; status / analysis / grouping / browse / A-B / export all scope to the currently open project. Per-project DB *files* were rejected — one DB, scope by column.
>
> - `projects(id TEXT PK, name TEXT, created_at TEXT, last_opened_at TEXT)`. `id` is a **UUID v4** generated backend-side in `create_project` (non-sequential, portable across a DB rebuild) — the one place a non-blake3 TEXT `id` is used.
> - `photos.project_id` and `similar_groups.project_id` are `TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE`. Deleting a project cascades its photos, its groups, and (via both FKs) the `group_members` rows. `photos` gains `UNIQUE(project_id, path)` — the same path may exist in a *different* project, but not twice in one.
> - **Photo id = `blake3(project_id + "\n" + path)`** (was `blake3(path)`). The same path in two projects hashes to two different ids → two fully independent rows (independent status / analysis / group membership). Ids stay globally unique across projects, so `set_status` / `transcode_for_display` remain correct keyed by `photo_id` alone — they do NOT filter by project, and must not start to (a project-A id never collides with a project-B id).
> - **Scoping contract**: the open project's id lives in `AppState.current_project: std::sync::Mutex<Option<String>>`, read via the `commands::current_project()` helper (returns `AppError::Validation("no project open")` when none is open — see `error-handling.md`). Every photo-scoped command (`scan_folder`, `export_keep`, `analyze_pending`, `group_photos`, `list_groups`) reads it before `spawn_blocking` and threads it into the query (`WHERE project_id = ?`) or the scanner / `regroup`. The id is cloned out before the blocking closure — the lock is never held across `.await`.
> - **Single-flight guards stay global**: `analysis_running` / `grouping_running` are NOT per-project. One project is open at a time, so they cannot leak across the boundary — correctness comes from the single-open invariant, not per-project flags.

Related OPEN items (deferred to feature tasks): status column encoding (TEXT vs INT), thumbnail resolution tiers.

---

## Common Mistakes

- Holding `Connection` in a `tauri::State` and calling it from an async command without `spawn_blocking` → blocks the Tauri event loop.
- Returning `rusqlite::Row` borrows across function boundaries (lifetime traps); always collect into owned structs.
- Mutating original files (e.g. updating EXIF). Read-only invariant.
