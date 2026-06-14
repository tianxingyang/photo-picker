# Project-based workspace isolation

## Goal

Switch the app from a single global workspace to a project-based working model.
A "project" is an isolated picking session: photos imported into one project
must never appear in another; status / analysis / grouping / export are all
scoped to the current project. The user works one project at a time
(create → import → pick → export), and can switch between projects.

## Confirmed Facts (from code inspection, 2026-06-13)

- Single SQLite DB at `app_data_dir/photo-picker.db`, opened once at startup,
  held in `AppState.db: Arc<Mutex<Connection>>` (src-tauri/src/lib.rs).
- `photos` table is global: `id = blake3(path)` PRIMARY KEY, `path` UNIQUE
  (migrations/0001). Analysis columns live on `photos` (0002);
  `similar_groups` / `group_members` (0003) reference `photos.id`.
- All 7 commands are global-scope: `scan_folder`, `set_status`, `export_keep`,
  `transcode_for_display`, `analyze_pending`, `group_photos`, `list_groups`.
- Frontend is a single view (App.tsx): hydrates all groups on mount; no
  routing; zustand stores `photosStore` / `groupsStore` / `compareStore`.
- ROADMAP.md has no "project" concept — this is a new post-M1 feature.
- `AppState` also carries `analysis_running` / `grouping_running`
  single-flight guards (currently global, would be per-project-safe only by
  scoping or by keeping one-project-open-at-a-time).

## Requirements

- R1: Introduce a "project" entity (name, created time). **Decided (Q1,
  2026-06-13): single DB + `projects` table + `photos.project_id` column;
  photo id derivation becomes `blake3(project_id + path)` so the same path
  can live in two projects independently (R3). Per-project DB files
  rejected: physical-isolation benefit unsupported by requirements, much
  larger refactor of connection lifecycle.** **Decided (Q5, 2026-06-14):
  `projects.id` is a backend-generated UUID v4 string (not an INTEGER
  surrogate) — non-sequential, non-guessable, stable/portable across DB
  rebuild or future project export-import. `photos.project_id` /
  `similar_groups.project_id` are TEXT FKs; photo id = `blake3(project_id +
  '\n' + path)`.**
- R2: All photo operations (import/scan, status, analysis, grouping, browse,
  A/B compare, export) operate only on the currently open project.
- R3: The same photo path imported into two different projects must be two
  independent records (independent status/analysis/groups).
- R4: Project management UI: list projects, create, open; app entry flow
  becomes "pick project first, then grid". **Decided (Q3, 2026-06-14):
  blocking project-picker landing page** — launch → project list → open →
  grid; switching projects returns to the landing page. Sidebar/top-bar
  switchers rejected (imply multi-open mental model, conflict with
  one-project-at-a-time, larger change to the no-router single view).
  Project list shows minimal metadata: **name + photo count + last-opened
  time** (all cheaply aggregated from DB); cover thumbnails deferred.
- R5: **Decided (Q2, 2026-06-13): existing global data is discarded** — the
  current DB holds development test imports only. Migration 0004 may simply
  rebuild tables (or clear rows); no id-recompute / data-carry code is
  written. Existing keep/reject labels are accepted as lost.

- R6: **Decided (Q4, 2026-06-14): minimal project delete is in scope** —
  landing-page list item has a delete button + confirm dialog; backend
  `delete_project` command cascades DB records (`photos`,
  `similar_groups`, `group_members` via `ON DELETE CASCADE`). External
  export copies on disk are NOT touched. Delete happens only from the
  landing page (no open project), avoiding the "delete the currently open
  project" edge case.

## Acceptance Criteria

- [ ] AC1 (entity): a `projects` table exists (id, name, created_at,
  last_opened_at); creating a project inserts a row and returns its id.
- [ ] AC2 (scoped id): photo id is `blake3(project_id + path)`; importing
  the same folder into two projects yields two disjoint sets of photo rows.
- [ ] AC3 (scoped ops): scan/status/analysis/grouping/browse/A-B/export each
  read and write only rows of the currently open project; no command can
  observe or mutate another project's photos.
- [ ] AC4 (independence): setting keep/reject in project A leaves the same
  path's record in project B unchanged; groups are computed within one
  project only.
- [ ] AC5 (entry flow): launching the app shows the blocking project list
  (name + photo count + last-opened); opening a project loads only its data
  and enters the grid; there is a way back to the list to switch projects.
- [ ] AC6 (create): from the list the user can create a named project and
  immediately open it into an empty grid.
- [ ] AC7 (delete): from the list the user can delete a project after a
  confirm dialog; its photos/groups rows are gone and it disappears from the
  list; other projects are unaffected; external export copies remain on disk.
- [ ] AC8 (migration): migration 0004 applies cleanly on the existing dev DB
  (rebuild/clear is acceptable per R5); app starts with an empty project list.
- [ ] AC9 (single-flight): analysis/grouping single-flight guards do not leak
  across the project boundary (scoped, or safe because one project is open).

## Out of Scope

- Multi-project open simultaneously (one open project at a time).
- Cross-project search / dedupe.
- Cloud sync, project sharing.
- Project rename, cover thumbnails, deleting external export copies.

## Open Questions

- (none — Q1, Q2 resolved 2026-06-13; Q3, Q4 resolved 2026-06-14)
