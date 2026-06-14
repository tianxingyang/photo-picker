# Implement — 分析提速与进度可视化

Requirements: `prd.md`. Design: `design.md`. Ordered plan. Build the speedup
core first (Python pool → Rust dispatch), then progress events, then the UI.
Each phase is independently testable.

## Ordered checklist

### Phase A — Python process pool (parallelism core)

- [ ] A1. `python/main.py`: rewrite `main()` per `design.md §2` — one
  `ProcessPoolExecutor(max_workers=max(1, cpu_count-1))`; `analyze` →
  `pool.submit(analyze.run, payload)` + `add_done_callback`; `echo`/`transcode`
  inline; `_stdout_lock` around all writes; keep UTF-8 reconfigure and the
  `allow_nan=False` + fallback serialize. Parse each line once.
- [ ] A2. Keep `analyze.run` signature/return identical (workers call it
  directly) so `analyzers/analyze.py` and its tests are untouched.
- [ ] A3. Tests: adjust/extend `python` tests for the new loop — a small
  end-to-end that pipes 2–3 `analyze` requests and asserts each `{id, result}`
  comes back (possibly out of order) and a bad path yields `{id, error}` without
  killing the loop. Run `uv run pytest` (per global uv rule).
- [ ] A4. Benchmark process-vs-thread once (a folder of N real images): if
  `ThreadPoolExecutor` matches/beats process for typical batches, flip the
  one-line executor type; otherwise keep ProcessPool. Record the choice in a
  `# why` comment.

### Phase B — Rust bounded-concurrency dispatch + cancel

- [ ] B1. `src-tauri/Cargo.toml`: add `futures = "0.3"`.
- [ ] B2. `src-tauri/src/lib.rs`: add `analysis_cancel: AtomicBool` to
  `AppState` (init `false`); register `cancel_analysis` command.
- [ ] B3. `commands/analysis.rs`: rewrite the serial loop as
  `futures::stream::iter(...).buffer_unordered(worker_count())` per
  `design.md §3`. Worker count via `std::thread::available_parallelism()` →
  `max(1, n-1)`. Reset `analysis_cancel` at batch entry. Preserve failure
  semantics (per-file→failed, transport→set cancel+stop, remaining pending) and
  the `analysis_running` single-flight guard. `analyze_one` stays as-is.
- [ ] B4. Add `cancel_analysis(state)` command → set `analysis_cancel`.
- [ ] B5. Rust tests: a unit test that the dispatch honors the cancel flag
  (skips remaining) and that an injected transport error stops the batch leaving
  remaining rows `pending` (reuse the existing persist/pending test patterns;
  the concurrency wrapper is testable with a stub or by asserting counts).

### Phase C — progress events (backend emit)

- [ ] C1. Define `ProgressEvent { phase, done, total: Option<u32>, status }`
  (serde camelCase) + an `emit(app, …)` helper (emit failure non-fatal). Place
  in a small `commands/progress.rs` or inline in `commands/mod.rs`.
- [ ] C2. `analyze_pending`: add `app: AppHandle` param; emit `analyze`
  per-completion + a terminal `done`/`cancelled` event (§3).
- [ ] C3. `scan_folder` + `scanner::scan_folder`: thread `app: &AppHandle`; emit
  `import` discovering (throttle ~200 files) + determinate during upsert + final
  (§5).
- [ ] C4. `group_photos`: add `app: AppHandle`; emit `group running` at entry,
  `group done` at exit.
- [ ] C5. Confirm all three commands still compile with the new param (Tauri
  injects `AppHandle`); update `invoke_handler!` if signatures changed (params
  don't need registration changes, but verify).

### Phase D — frontend (store + bar + wiring)

- [ ] D1. `src/store/progressStore.ts`: zustand state
  `{ phase, done, total, status } | null`; a `listen("pipeline://progress")`
  registered once; clear to `null` shortly after a terminal event.
- [ ] D2. `src/api/pipelineApi.ts`: `cancelAnalysis()` →
  `invoke("cancel_analysis")`.
- [ ] D3. `PipelineProgressBar` component **via `ui-ux-pro-max`**: thin
  full-width bar under the header; determinate (`done/total`) or indeterminate
  (`total==null`); label (`导入 N` / `分析 d/t` / `分组中…`) + cancel control in
  the analyze phase; hidden when store is `null`; non-blocking.
- [ ] D4. `src/App.tsx`: render `<PipelineProgressBar/>` under the header; keep
  `busy` for button-disable; ensure the listener is registered on mount and torn
  down on unmount (and not duplicated across project open/close).

## Validation commands

```bash
# Python (uv per global rule), from python/
uv run pytest

# Rust, from src-tauri/
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check

# Frontend, from repo root
npx tsc --noEmit
npx eslint "src/**/*.{ts,tsx}"   # new files must be clean
npm run build
```

Manual GUI smoke (user-driven per convention — provide steps, don't script):
1. Import a large folder → thin bar shows `导入 …` then `分析 d/t` advancing;
   observe Task Manager / CPU: multiple python workers busy (not one core).
2. Bar reaches `done==total`, then `分组中…`, then clears.
3. Mid-analysis click cancel → bar clears; re-run "分析并分组" → resumes only the
   remaining (pending) photos; previously analyzed stay done.
4. A corrupt image → counts as failed, batch continues; sidecar stays alive.

## Risky files / rollback points

- `python/main.py` — the loop rewrite is the highest-risk change (IPC stream
  integrity, stdout interleaving). Rollback point: before A1. Pin the stdout
  lock + `allow_nan=False` fallback.
- `commands/analysis.rs` — concurrency must preserve the failure/cancel
  contract; a regression here re-introduces the "dead sidecar marks all failed"
  bug. Reuse `error-handling.md` assertions.
- `App.tsx` / listener lifecycle — a duplicated `listen` across project
  open/close leaks handlers; register once.

## Pre-start checks

- [ ] AC1–AC9 each map to a checklist item.
- [ ] No open questions in `prd.md` (D1–D6 resolved).
- [ ] `futures` dep + `analysis_cancel` reflected in B1/B2.
- [ ] Failure/cancel semantics preserved (B3) — the one true correctness risk.

## Rollback

Revert the branch. No DB migration, so no data implications — the Python loop,
Rust dispatch, events, and UI revert cleanly; remove the `futures` dep with the
revert.
