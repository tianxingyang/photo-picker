# 分析提速与进度可视化

## Goal

Make the analyze→group pipeline visibly faster and give the user live progress
feedback while it runs. Two coupled deliverables on the same loop:
(1) parallelize photo analysis to use multiple CPU cores; (2) stream progress
events to a frontend progress UI. Milestone 2 ("体验增强").

## Confirmed Facts (from code inspection, 2026-06-14)

- **Single sidecar process, strictly serial.** One `uv run python main.py`
  spawned once (`sidecar/mod.rs:46`). `main.py:47` is `for raw in sys.stdin:` —
  read one line → handle synchronously → write → next. One image at a time,
  one CPU core.
- **`analyze_pending` also serial.** The Rust batch loop awaits each
  `analyze_one` before dispatching the next (`commands/analysis.rs`), so even
  though `Sidecar::call` supports concurrent in-flight requests (id-keyed
  `pending` map), only one analysis is ever in flight.
- **Per-image work** (`analyzers/analyze.py:31-38`): `Image.open`+`load`
  (HEIC via `pillow_heif`, decode is the dominant cost) + EXIF + grayscale
  downscale (`NORM_MAX_SIDE`) + blur (Laplacian var) + exposure + pHash.
- **Grouping is NOT the bottleneck.** `grouping/mod.rs:78 cluster` is O(n²)
  popcount in Rust (sub-ms at n≈thousands). The "分组慢" perception comes from
  the UI "分析并分组" button running `analyzePending()`(slow) → `groupPhotos()`
  (fast) → `loadGroups()` sequentially — the wall-clock is analysis-bound.
- **No progress channel.** Commands return only a final summary; no events are
  emitted mid-run. Frontend shows a static "处理中…" (App.tsx).
- **Sidecar is dev-only** (`spawn_dev`, `uv run`); release bundling deferred to
  M4. Any parallelism design works within the dev sidecar model.
- **Failure/retry contract must be preserved** (spec `backend/error-handling.md`):
  per-file error → persist `failed`; transport/infra error → leave `pending`,
  stop the batch; single-flight `analysis_running` guard with RAII reset.
- **ROADMAP status**: progress visualization is an existing M2 line ("导入进度 /
  分析进度可视化（前端订阅 Rust 事件）"). Analysis *parallelism* is NOT on the
  roadmap (M4 plans the ONNX rewrite); this task adds a pragmatic mid-point and
  will be appended to ROADMAP M2 on delivery.

## Requirements

- R1 (speed): analysis uses multiple CPU cores so wall-clock for N photos drops
  roughly proportional to core count (decode-bound, largely parallelizable).
- R2 (correctness preserved): per-photo results identical to today; the
  per-file-vs-transport failure semantics and single-flight guard are preserved
  under concurrency.
- R3 (progress): the analyze (and group) run streams progress to the frontend
  (determinate done/total for analysis; phase marker for grouping), rendered as
  a progress bar via the `ui-ux-pro-max` skill.
- R4 (no behavior regression): grouping result, browse model, and project
  scoping (current_project) are unaffected.

## Decisions (2026-06-14)

- **D1 (parallelism = approach B, Rust process pool — REVISED 2026-06-15)**:
  Originally planned as approach A (one sidecar with an internal Python pool),
  but **A is not viable on this platform** (Windows + Python 3.14): a
  `ProcessPoolExecutor` deadlocks under the piped-stdio sidecar, and a
  `ThreadPoolExecutor` hits `OSError(EINVAL)` / worker hangs because a thread
  parked in a stdin read cannot coexist with concurrent stdout writes or worker
  IO. Isolation-tested and confirmed. **Final design: the Python sidecar stays
  single-threaded + synchronous (the proven, stable model), and RUST spawns a
  POOL of N sidecar processes** (`N = min(cpu-1, 4)`), routing each `analyze`
  to a distinct process via `buffer_unordered(N)`. No Python threads anywhere.
  transcode/echo use the first sidecar. See `design.md §2-3`.
- **D2 (progress scope = import + analyze + group)**: all three pipeline phases
  emit progress (fulfills the ROADMAP M2 "导入进度 / 分析进度" line in full).
- **D3 (progress UI = thin top loading bar)**: a full-width slim bar under the
  header with a phase + count label; non-blocking (grid stays scrollable). Built
  via the `ui-ux-pro-max` skill.
- **D4 (minimal cancel = in scope)**: cooperative cancel — stop dispatching new
  photos, let in-flight finish, analyzed rows stay `done`, the rest stay
  `pending` for a later resume. Reuses the existing "stop batch / keep progress"
  semantics.
- **D5 (worker count)**: auto `max(1, cpu_count-1)`, NOT user-configurable (no
  settings UI this task). Decided in design; not a user-facing option now.
- **D6 (task structure)**: single task with an ordered `implement.md` (speed and
  progress are coupled — progress events are emitted from the very loops the
  speed work rewrites). No parent/child split.

## Acceptance Criteria

- [ ] AC1 (parallel): analysis dispatches up to N = `max(1, cpu_count-1)` photos
  concurrently through a Python process pool; a batch of many photos shows
  multiple workers busy (not one-at-a-time). Wall-clock for a representative
  batch drops materially vs the serial baseline.
- [ ] AC2 (correctness preserved): per-photo persisted columns are identical to
  the serial implementation; per-file decode error → `analysis_state='failed'`
  + `analysis_error`; transport/infra error → row stays `pending`, batch stops;
  single-flight `analysis_running` guard still prevents concurrent batches; a
  re-run resumes only `pending` rows.
- [ ] AC3 (import progress): `scan_folder` emits progress (files discovered →
  indexed) consumed by the bar.
- [ ] AC4 (analyze progress): determinate `done/total` events drive the bar;
  the final event satisfies `done == total` (or `done + remaining-on-cancel`).
- [ ] AC5 (group progress): an indeterminate "grouping" phase marker is shown
  while `group_photos` runs.
- [ ] AC6 (UI): a thin top loading bar shows the active phase + progress, is
  non-blocking (the grid remains scrollable), and clears on completion/cancel;
  built via `ui-ux-pro-max`.
- [ ] AC7 (cancel): a cancel control stops dispatching; in-flight analyses
  finish; analyzed rows stay `done`, the rest stay `pending`; a subsequent run
  resumes them; the UI returns to idle.
- [ ] AC8 (project scope): progress counts and all DB work stay scoped to the
  open project (`current_project`); no cross-project leakage.
- [ ] AC9 (no regression): grouping output, browse model, and existing Rust +
  Python tests still pass (adjusted for the pool where needed); tsc + build green.

## Out of Scope

- M4 ONNX / de-Python rewrite.
- Thumbnail pre-generation / WebP cache (separate M2 line).
- Changing blur/exposure/pHash metrics (bokeh backlog is its own task).
- User-configurable worker count / a settings UI.
- Pause/resume beyond the natural "remaining stay pending" resume.
- Parallelizing across multiple open projects (one project open at a time).

## Open Questions

- (none — D1–D6 resolved 2026-06-14; process-vs-thread pool benchmarked in design)
