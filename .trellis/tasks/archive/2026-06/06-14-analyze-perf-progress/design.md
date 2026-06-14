# Design — 分析提速与进度可视化

Requirements: `prd.md`. This is the technical design. M2.

## 1. Architecture & boundaries

Three pipeline phases, each gaining progress; analysis additionally gains
multicore parallelism via a **pool of N sidecar processes**:

```
import (scan_folder)   analyze (analyze_pending)        group (group_photos)
  Rust walkdir+upsert    Rust buffer_unordered(N)  ──▶    Rust cluster (fast)
       │                    ├─▶ sidecar #0 (sync python)        │
       │                    ├─▶ sidecar #1 (sync python)        │
       │                    └─▶ … N sidecars                    │
       ▼                             ▼                          ▼
   emit pipeline://progress {phase, done, total, status}  ──▶  frontend
                                                          thin top bar (listen)
```

- **Parallelism lives in RUST** (decision D1, revised): each Python sidecar is
  single-threaded + synchronous (unchanged from M1); Rust spawns a POOL of N
  (`min(cpu-1, 4)`) and routes each `analyze` to a distinct process. In-process
  Python concurrency was abandoned — see §2.
- **Rust `analyze_pending`** dispatches with `buffer_unordered(N)`, routing item
  `idx` to `pool[idx % N]` → up to N analyses in flight, one per process.
- **Progress is a single Tauri event** emitted from each phase's Rust command;
  the frontend listens and renders one thin bar.
- **Cancel** is a cooperative `AtomicBool` in `AppState`, checked by the
  dispatch loop.

## 2. Why a Rust process pool (NOT in-process Python threads)

`main.py` stays the **proven serial loop** — `for raw in sys.stdin:
write(handle(line))`, single-threaded and synchronous, UNCHANGED from M1.

In-process Python parallelism was attempted and rejected after isolation testing
on Windows + Python 3.14, where this sidecar runs with Rust-piped stdin/stdout:

- **`ProcessPoolExecutor`** — `multiprocessing` spawn deadlocks bootstrapping
  workers under the piped-stdio parent: `analyze` calls never return, the Rust
  30s call-timeout fires, the batch ends with zero completions ("处理中…" stuck).
- **`ThreadPoolExecutor`** — a thread parked in a stdin read cannot coexist with
  the IPC: writing the stdout pipe fd from any thread while another is blocked in
  a stdin read raises `OSError(EINVAL)` (buffered flush AND raw `os.write`), and
  even moving the reader to its own thread, the analyze workers hang. Writes
  succeed ONLY when no thread is blocked in a stdin read — i.e. the original
  single-threaded model.

So the robust design keeps **zero Python threads** and parallelizes by running N
**processes**, each a pristine synchronous sidecar. Cost: N× interpreter +
numpy/pillow_heif memory (~50–100MB each; capped at N=4) and N× `uv run` startup
at boot (spawned concurrently, in the background after the window shows). The
sidecar pool is built in `lib.rs` (`sidecar_pool_size()` = `min(cpu-1, 4)`);
`transcode`/`echo` use `pool[0]`.

> Tradeoff vs the original A plan: more boot memory/time and a Rust-side pool to
> manage, in exchange for actually working on this platform. The Python side is
> the simplest possible (the M1 loop, no concurrency primitives).

## 3. Rust `analyze_pending` — bounded concurrency + progress + cancel

Replace the serial `for` loop with a `futures::stream` bounded by N. Preserve
the failure contract (`backend/error-handling.md`): per-file error → `failed`;
transport error → stop, remaining stay `pending`.

```rust
// AppState gains: pub analysis_cancel: AtomicBool   (reset at batch start)
let pool = sidecar_pool(&state).await?;   // Vec<Arc<Sidecar>>, errors if empty
let n = pool.len();                        // = min(cpu-1, 4)

let total = pending.len();
state.analysis_cancel.store(false, Release);
let done = AtomicUsize::new(0);

let outcomes = futures::stream::iter(pending.into_iter().enumerate().map(|(idx, (id, path))| {
    let (pool, state, app) = (&pool, &state, &app);
    async move {
        if state.analysis_cancel.load(Acquire) { return Outcome::Skipped; }
        let sidecar = &pool[idx % n];      // route to one process (≤ n in flight ⇒ distinct)
        match analyze_one(sidecar, state, &id, &path).await {
            Ok(true)  => { let d = done.fetch_add(1, AcqRel) + 1; emit(app, Phase::Analyze, d, total, Running); Outcome::Ok }
            Ok(false) => { let d = done.fetch_add(1, AcqRel) + 1; emit(app, Phase::Analyze, d, total, Running); Outcome::Failed }
            Err(e)    => Outcome::Transport(e),   // infra: do not persist (analyze_one already left it pending)
        }
    }
}))
.buffer_unordered(n);

let (mut analyzed, mut failed) = (0u32, 0u32);
futures::pin_mut!(outcomes);
while let Some(o) = outcomes.next().await {
    match o {
        Outcome::Ok => analyzed += 1,
        Outcome::Failed => failed += 1,
        Outcome::Skipped => {}                         // cancelled; keep draining in-flight
        Outcome::Transport(e) => { eprintln!("analyze: stop after infra error: {e}"); state.analysis_cancel.store(true, Release); }
    }
}
let status = if state.analysis_cancel.load(Acquire) { Status::Cancelled } else { Status::Done };
emit(&app2, Phase::Analyze, done.load(Acquire), total, status);   // terminal event clears the bar
Ok(AnalyzeSummary { analyzed, failed })
```

- **`buffer_unordered(N)`** polls up to N analyze futures concurrently (each is a
  `sidecar.call` → fed to a pool worker), pulling more as they complete →
  exactly N in flight, pool stays fed.
- **Cancel & infra-stop share one flag**: setting `analysis_cancel` makes
  not-yet-started futures short-circuit to `Skipped`; in-flight finish and
  persist. Remaining rows are untouched (`pending`) → a re-run resumes them.
- **Single-flight** `analysis_running` guard is unchanged (still one batch at a
  time). The cancel flag is reset at batch entry so a stale cancel can't kill the
  next run.
- **`analyze_one` is unchanged** — still persists per result and leaves the row
  `pending` on a transport error. Concurrency does not change per-row semantics.
- New dep: `futures = "0.3"` (for `StreamExt::buffer_unordered`). Cancel uses a
  plain `AtomicBool` (no tokio-util).

### New command
`cancel_analysis(state)` → `state.analysis_cancel.store(true)`. Registered in
`lib.rs`. Frontend calls it from the bar's cancel control.

## 4. Progress event contract

One global Tauri event, emitted by `scan_folder` / `analyze_pending` /
`group_photos` (each gains an `app: AppHandle` param — Tauri injects it).

```
event name: "pipeline://progress"
payload (serde camelCase):
{
  phase:  "import" | "analyze" | "group",
  done:   number,                 // items completed this phase
  total:  number | null,          // null = indeterminate (import-discovering, group)
  status: "running" | "done" | "cancelled"
}
```

- **import**: while walking, `{import, done: discovered, total: null, running}`
  (throttled every ~200 files); after the walk, `total` becomes the matched
  count and `done` advances through the upsert loop; final `{import, total,
  total, done}`.
- **analyze**: `{analyze, done, total, running}` per completion; terminal
  `{analyze, done, total|done, done|cancelled}`.
- **group**: `{group, 0, null, running}` at entry, `{group, 0, null, done}` at
  exit (fast; indeterminate marker only).
- **Throttling**: emit per analyze completion (React batches state updates; a
  few hundred events over a run is fine). Import throttles to every ~200 files.
  If a pathological batch (10k+) floods, add a ~50ms coalesce — deferred unless
  observed.
- **Scope**: commands are already `current_project`-scoped, so counts are the
  open project's. No `projectId` in the payload (one pipeline runs at a time);
  closing a project (App routing) unmounts the listener path anyway.

Emit helper (Rust): `fn emit(app: &AppHandle, phase, done, total, status)` →
`let _ = app.emit("pipeline://progress", ProgressEvent{..})`. Emit failure is
non-fatal (never abort real work because a UI event didn't send).

## 5. Import & group progress

- **`scan_folder` / `scanner::scan_folder`**: take `app: &AppHandle`. Emit
  `import` discovering events during the `WalkDir` collection (every ~200
  matched), then determinate events during the insert loop. Keep the single
  transaction; emit between iterations.
- **`group_photos`**: emit a `group running` at entry and `group done` after
  `regroup` returns. No per-item granularity (clustering is sub-ms).

## 6. Frontend

- **`src/store/progressStore.ts`** (zustand): holds the latest
  `{phase, done, total, status} | null`. A module-level `listen("pipeline://
  progress", …)` (set up once, e.g. in `main.tsx` or a hook) updates it; a
  terminal `done`/`cancelled` clears to `null` after a short delay.
- **`src/api/pipelineApi.ts`**: `cancelAnalysis()` → `invoke("cancel_analysis")`.
- **`PipelineProgressBar`** (new component, via `ui-ux-pro-max`): a thin
  full-width bar under the header. Determinate width = `done/total` when `total`
  is set; indeterminate animated stripe when `total == null`. Shows a compact
  label (`导入 128`, `分析 128/340`, `分组中…`) + a cancel control during the
  analyze phase. Non-blocking; the grid stays scrollable. Hidden when the store
  is `null`.
- **`App.tsx`**: render `<PipelineProgressBar/>` under the header. The existing
  `busy` flag and "处理中…" text stay for button-disable, but the textual
  progress is replaced by the bar. Listener lifecycle: register on mount,
  unregister on unmount.
- **`@tauri-apps/api/event`** `listen` (package already present, `^2`).

## 7. Tradeoffs & risks

- **Spawn overhead (Windows)**: small batches pay pool spin-up. Acceptable —
  small batches are already fast. Optional pre-warm (submit a no-op at sidecar
  start) deferred.
- **Determinism of grouping unaffected**: analysis still persists the same
  columns; grouping reads them after. Out-of-order analysis does not change
  results (each row independent).
- **Event flooding**: mitigated by import throttle; analyze per-completion is
  bounded by photo count. Coalesce only if observed.
- **Broken pool** degenerate path (worker dies): surfaces as op-errors; not
  specially recovered this task.
- **Benchmark, not a guarantee**: the speedup is decode-bound and largely
  parallel, but HEIC decode + numpy contention may cap scaling below linear.
  AC1 asks for "materially faster + multiple workers busy", not linear.

## 8. Rollout / rollback

- Ship Python pool + Rust bounded-concurrency + events + bar together; there is
  no useful half state.
- Rollback: revert the branch. The Python loop change and Rust dispatch change
  are independent of the DB schema (no migration), so rollback is clean — no
  data implications. `futures` dep removal is part of the revert.
