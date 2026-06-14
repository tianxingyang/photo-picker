# Implement — Project-based workspace isolation

Requirements: `prd.md`. Design: `design.md`. This is the ordered execution plan.
Build backend-first (schema → scoping → commands), then frontend, then the
landing UI. Each numbered block is independently compilable/testable.

## Ordered checklist

### Phase A — Schema & migration (backend foundation)

- [ ] A1. Add `src-tauri/migrations/0004_projects.sql` with the full
  drop+recreate from `design.md §2` (projects table; rebuild photos,
  similar_groups, group_members with `project_id` + `UNIQUE(project_id,path)` +
  indexes).
- [ ] A2. Append `include_str!("../../migrations/0004_projects.sql")` to
  `MIGRATIONS` in `src-tauri/src/db/mod.rs:9`.
- [ ] A3. Update `db/mod.rs` migration tests: the existing tests assert
  `user_version == MIGRATIONS.len()` and probe `photos` columns — re-point any
  hard-coded version (e.g. `grouping_tables_build_at_version_3`) and ensure a
  fresh-DB test seeds a `projects` row before inserting a photo (FK now
  required). Add a test: `delete project cascades photos + groups`.

### Phase B — id derivation & scanner

- [ ] B1. `scanner::scan_folder` signature gains `project_id: &str` (UUID); id
  becomes `blake3(format!("{project_id}\n{path}"))`; INSERT includes
  `project_id`.
- [ ] B2. Update scanner `mem_conn()` + tests: apply through `0004`, seed a
  project, pass its id, recompute expected ids with the new derivation
  (`id_is_stable_blake3_hex_of_path`, `rescan_preserves_existing_status`,
  `incremental_import_is_idempotent`, `filters_unsupported_and_recurses`).

### Phase C — AppState & project commands

- [ ] C0. `src-tauri/Cargo.toml`: add `uuid = { version = "1", features =
  ["v4"] }`.
- [ ] C1. `src-tauri/src/lib.rs`: add
  `current_project: std::sync::Mutex<Option<String>>` to `AppState`; init
  `std::sync::Mutex::new(None)` (lib.rs:41); register the 5 new commands
  (lib.rs:63).
- [ ] C2. New `src-tauri/src/commands/projects.rs`: `ProjectSummary` struct +
  `create_project` (id = `uuid::Uuid::new_v4().to_string()`), `list_projects`,
  `open_project`, `close_project`, `delete_project` (contracts in
  `design.md §3`). Add `pub mod projects;` to `commands/mod.rs`.
- [ ] C3. Add the `current_project(state) -> Result<String, AppError>` helper
  (lock+clone the `Option<String>`; guard returns `Validation("no project
  open")`). Place in `commands/mod.rs` or a shared spot importable by
  photos/analysis/grouping.
- [ ] C4. Unit-test `projects.rs`: create→list shows photo_count 0; open sets
  last_opened_at; delete removes the row; list order (last_opened desc nulls
  last). Use an in-memory conn migrated through 0004.

### Phase D — scope existing commands

- [ ] D1. `commands/photos.rs::scan_folder`: read project via `current_project`,
  pass to scanner. `set_status` / `transcode_for_display`: **unchanged**
  (verify, don't edit).
- [ ] D2. `commands/photos.rs::export_keep`: `select_keep_paths` takes
  `project_id`, query `WHERE status='keep' AND project_id=?1`. Update its tests
  (seed project + project_id; pass id).
- [ ] D3. `commands/analysis.rs::analyze_pending`: pending SELECT gains
  `AND project_id=?1` (read project before spawn_blocking). Persist unchanged.
- [ ] D4. `commands/grouping.rs::regroup`: takes `project_id`; photo SELECT
  `WHERE project_id=?1`; DELETE `WHERE method=?1 AND project_id=?2`; INSERT
  similar_groups includes `project_id`. `load_browse_model` takes `project_id`;
  add `p.project_id=?` to grouped JOIN and ungrouped SELECT, scope
  similar_groups reads. Update all grouping tests (seed project, thread id).

### Phase E — frontend API & state

- [ ] E1. `src/api/projectsApi.ts`: 5 functions + `ProjectSummary` type +
  boundary validation (mirror `photosApi` style).
- [ ] E2. `src/store/projectsStore.ts`: per `design.md §4`. `open`/`close` must
  clear `photosStore`, `groupsStore`, `compareStore` and call
  `clearDisplayCache()`.

### Phase F — routing & landing UI

- [ ] F1. `src/App.tsx`: branch on `currentProjectId` (null → Landing; set →
  existing view). Add 「切换项目」 header button → `projectsStore.close()`.
  Hydrate `loadGroups()` after `open()`, not on mount.
- [ ] F2. `LandingView` component **through the `ui-ux-pro-max` skill**: project
  list (name + photo count + last-opened), create control, per-row delete with
  confirm dialog. Wire to `projectsStore`.

## Validation commands

```bash
# Rust: unit tests + lint (run from src-tauri/)
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check

# Frontend: type-check + lint + build (run from repo root)
npm run lint
npx tsc --noEmit
npm run build
```

Manual GUI smoke (user-driven per project convention — provide steps, do not
script): launch dev (`npm run tauri dev`, port 5173), confirm:
1. fresh launch shows empty Landing; create "P1" and "P2".
2. open P1, import a folder, mark some keep → switch project → open P2, import
   the **same** folder → P2 shows all pending (P1's keep labels not visible).
3. back in P1 the keeps persist (independence, AC4).
4. delete P2 from Landing (confirm dialog) → it disappears; P1 unaffected;
   exported copies on disk (if any) remain.

## Risky files / rollback points

- `src-tauri/migrations/0004_projects.sql` — **destructive** (drops tables).
  Safe only because R5 discards data. Rollback point: before A1.
- `src-tauri/src/db/mod.rs` — migration array + tests; a wrong version number
  fails every migration test loudly (good early signal).
- Test fixtures in `scanner`, `analysis`, `grouping`, `photos` modules — largest
  churn surface; expect many expected-id/photo-insert edits (design.md §5).
- `src/App.tsx` — entry-flow rewrite; keep the existing grid intact, only gate
  it behind the project branch.

## Pre-start checks

- [ ] `prd.md` acceptance criteria AC1–AC9 each map to a checklist item above.
- [ ] No open questions remain in `prd.md`.
- [ ] Decision A (state-held current project) reflected in `design.md` and C1/C3.
- [ ] `implement.jsonl` / `check.jsonl` curated if sub-agent mode is used.

## Rollback

Revert the branch. Because `0004` rebuilds tables, do **not** roll back in place
on a migrated dev DB — delete the DB file
(`app_data_dir/photo-picker.db*`, incl. `-wal`/`-shm`) and relaunch (acceptable
per R5). See `design.md §6`.
