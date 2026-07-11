use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

use futures::stream::StreamExt;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, State};

use crate::commands::{current_project, progress, sidecar_pool};
use crate::db::{thumb_dest, thumbnails_dir};
use crate::error::AppError;
use crate::AppState;

/// Longest-side cap for generated thumbnails. A single 512px tier covers the
/// grid worst case at retina (≈270 CSS px tile × DPR 2 = 540 physical px) and
/// is generously oversampled for the 64px compare filmstrip — see the task PRD
/// decision (single tier, not dual 256/512).
const THUMB_MAX_SIDE: u32 = 512;
const WEBP_QUALITY: u32 = 80;

/// RAII guard for the single-flight `thumbnails_running` flag. Clearing on Drop
/// guarantees the flag is reset on every exit path (early return, `?`, panic).
struct RunGuard<'a>(&'a AtomicBool);

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailSummary {
    pub generated: u32,
    /// Up-to-date cache hits + sources that vanished + photos skipped after a cancel.
    pub skipped: u32,
    pub failed: u32,
    // why: true when the user cancelled mid-batch (remaining photos stay
    // `pending`). Mirrors AnalyzeSummary.cancelled.
    pub cancelled: bool,
}

/// One row from the up-front scan: what generate_thumbnails needs to decide
/// whether a thumbnail must be (re)generated.
struct PhotoRow {
    id: String,
    path: String,
    thumb_status: String,
    thumb_src_mtime: Option<i64>,
}

/// One unit of generation work after the stat-filter pass.
struct WorkItem {
    id: String,
    path: String,
    dest: PathBuf,
    /// Source mtime (nanos) observed during filtering; persisted on success so a
    /// later run can detect a changed source and self-heal.
    mtime_nanos: i64,
}

/// Per-photo dispatch outcome inside the bounded-concurrency stream.
enum Outcome {
    Ok,
    Failed,
    /// Cancelled before this photo started — skipped, row stays `pending`.
    Skipped,
    /// Transport/infra failure: stop feeding the rest of the batch.
    Transport(AppError),
}

/// Generate a 512px WebP thumbnail for every photo in the open project that
/// needs one, dispatching the `thumbnail` op across the **sidecar pool** with
/// one in-flight encode per process (same parallelism model as analyze_pending).
///
/// Self-healing: the scanner is INSERT OR IGNORE (it never re-touches existing
/// rows or records mtime), so staleness is detected here. The up-front
/// stat-filter pass re-stats each source and (re)generates when the thumbnail is
/// missing or the source mtime no longer matches `thumb_src_mtime`. Up-to-date
/// thumbnails are skipped without a sidecar call, so a re-run (or resume after a
/// cancel) is cheap.
///
/// Cancellation: `cancel_thumbnails` sets `thumbnails_cancel`; not-yet-started
/// photos are skipped (stay `pending`), in-flight encodes finish and persist.
///
/// Single-flight: a concurrent invocation returns an empty summary immediately.
#[tauri::command]
pub async fn generate_thumbnails(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ThumbnailSummary, AppError> {
    // why: single-flight — bail out if another run already holds the flag.
    if state
        .thumbnails_running
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Ok(ThumbnailSummary {
            generated: 0,
            skipped: 0,
            failed: 0,
            cancelled: false,
        });
    }
    // RAII: clears thumbnails_running on every exit path below (including `?`).
    let _guard = RunGuard(&state.thumbnails_running);
    // Fresh batch: clear any stale cancel from a previous run.
    state.thumbnails_cancel.store(false, Ordering::Release);

    let project_id = current_project(&state)?;
    let thumbs_dir = thumbnails_dir(&app).map_err(|e| AppError::Io(e.to_string()))?;

    // why: the stat-filter pass can take a beat on a large library — show the bar
    // immediately (indeterminate) so the phase is visible before per-item ticks.
    progress::running(&app, progress::PHASE_THUMBNAIL, 0, None);

    // Clone the sidecar pool out (cheap Arc clones); released before any await.
    let pool = sidecar_pool(&state).await?;
    let n = pool.len();

    // 1. Read every photo in the project. DB lock is released when the closure
    //    returns, BEFORE the (possibly many) filesystem stats below.
    let rows = {
        let db = state.db.clone();
        let project_id = project_id.clone();
        tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<Vec<PhotoRow>> {
            let conn = db.blocking_lock();
            let mut stmt = conn.prepare_cached(
                "SELECT id, path, thumb_status, thumb_src_mtime FROM photos \
                 WHERE project_id = ?1",
            )?;
            let rows = stmt
                .query_map(params![project_id], |r| {
                    Ok(PhotoRow {
                        id: r.get(0)?,
                        path: r.get(1)?,
                        thumb_status: r.get(2)?,
                        thumb_src_mtime: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?
    };

    // 2. Stat-filter pass (no DB lock, no decode): build the worklist. Runs in
    //    spawn_blocking because it does up to one fs::metadata per photo.
    let (worklist, mut skipped) = {
        let thumbs_dir = thumbs_dir.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut worklist: Vec<WorkItem> = Vec::new();
            let mut skipped = 0u32;
            for row in rows {
                let cur_mtime = match std::fs::metadata(&row.path).and_then(|m| m.modified()) {
                    Ok(t) => t.duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as i64,
                    // Source file is gone — nothing to thumbnail; count as skipped.
                    Err(_) => {
                        skipped += 1;
                        continue;
                    }
                };
                let dest = thumb_dest(&thumbs_dir, &row.id);
                if needs_regen(
                    &row.thumb_status,
                    row.thumb_src_mtime,
                    cur_mtime,
                    dest.exists(),
                ) {
                    worklist.push(WorkItem {
                        id: row.id,
                        path: row.path,
                        dest,
                        mtime_nanos: cur_mtime,
                    });
                } else {
                    skipped += 1;
                }
            }
            (worklist, skipped)
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))?
    };

    let total = worklist.len() as u32;
    // why: shared counter so progress is monotonic under out-of-order completion.
    let done = AtomicUsize::new(0);

    // 3. Bounded-concurrency generation: up to `n` encodes in flight, each routed
    //    to a distinct sidecar by index (idx % n).
    let mut stream = futures::stream::iter(worklist.into_iter().enumerate().map(|(idx, item)| {
        let pool = &pool;
        let state = &state;
        let app = &app;
        let done = &done;
        async move {
            // why: check cancel BEFORE starting so a cancelled batch stops feeding
            // new work; in-flight encodes already past this point finish & persist.
            if state.thumbnails_cancel.load(Ordering::Acquire) {
                return Outcome::Skipped;
            }
            let sidecar = &pool[idx % n];
            match generate_one(sidecar, state, &item).await {
                Ok(ok) => {
                    let d = done.fetch_add(1, Ordering::AcqRel) as u32 + 1;
                    progress::running(app, progress::PHASE_THUMBNAIL, d, Some(total));
                    if ok {
                        Outcome::Ok
                    } else {
                        Outcome::Failed
                    }
                }
                Err(e) => Outcome::Transport(e),
            }
        }
    }))
    .buffer_unordered(n);

    let mut generated = 0u32;
    let mut failed = 0u32;
    let mut infra_err: Option<AppError> = None;
    while let Some(outcome) = stream.next().await {
        match outcome {
            Outcome::Ok => generated += 1,
            Outcome::Failed => failed += 1,
            Outcome::Skipped => skipped += 1,
            Outcome::Transport(e) => {
                // Infra failure: stop feeding the batch (reuse the cancel flag as
                // the stop signal). Remaining rows keep their thumb_status; a
                // re-run resumes them. Keep the first error to report.
                eprintln!("generate_thumbnails: stopping batch after infra error: {e}");
                state.thumbnails_cancel.store(true, Ordering::Release);
                if infra_err.is_none() {
                    infra_err = Some(e);
                }
            }
        }
    }

    // Terminal status: error > cancelled > done (an infra stop reports `error`
    // even though it set the stop flag, so it never looks like a user cancel).
    let user_cancelled = infra_err.is_none() && state.thumbnails_cancel.load(Ordering::Acquire);
    let status = if infra_err.is_some() {
        progress::STATUS_ERROR
    } else if user_cancelled {
        progress::STATUS_CANCELLED
    } else {
        progress::STATUS_DONE
    };
    progress::terminal(
        &app,
        progress::PHASE_THUMBNAIL,
        done.load(Ordering::Acquire) as u32,
        Some(total),
        status,
    );

    if let Some(e) = infra_err {
        return Err(e);
    }

    Ok(ThumbnailSummary {
        generated,
        skipped,
        failed,
        cancelled: user_cancelled,
    })
}

/// Cooperatively cancel an in-progress thumbnail batch. Sets `thumbnails_cancel`;
/// the dispatch loop stops starting new photos, in-flight encodes finish and
/// persist. A no-op if nothing is running.
#[tauri::command]
pub fn cancel_thumbnails(state: State<'_, AppState>) {
    state.thumbnails_cancel.store(true, Ordering::Release);
}

/// Decide whether a photo needs its thumbnail (re)generated. Pure (no IO) so the
/// self-heal policy is unit-testable:
/// - `pending`: always (first pass / scanner default).
/// - `done`: only if the file is missing or the source mtime changed (in-place
///   edit) → self-heal.
/// - `failed`: only if the source mtime changed; an unchanged bad file is not
///   retried every run.
fn needs_regen(
    thumb_status: &str,
    stored_mtime: Option<i64>,
    cur_mtime: i64,
    dest_exists: bool,
) -> bool {
    match thumb_status {
        "done" => !dest_exists || stored_mtime != Some(cur_mtime),
        "failed" => stored_mtime != Some(cur_mtime),
        // "pending" or any unexpected value: (re)generate.
        _ => true,
    }
}

/// Generate one thumbnail and persist the outcome. Returns Ok(true) on a stored
/// success, Ok(false) on a stored per-file failure (bad file / decode error). A
/// transport/infra failure (sidecar down / timeout) propagates as Err WITHOUT
/// persisting — the row keeps its status so a re-run retries it. Mirrors
/// analyze_one so the caller can drive it with bounded concurrency.
async fn generate_one(
    sidecar: &crate::sidecar::Sidecar,
    state: &State<'_, AppState>,
    item: &WorkItem,
) -> Result<bool, AppError> {
    // The Python `thumbnail` op os.makedirs() the two-level parent itself (like
    // `transcode`), so no Rust-side create_dir_all is needed.
    let dest = item.dest.to_string_lossy().into_owned();
    let result: Result<(), String> = match sidecar
        .call(
            "thumbnail",
            json!({
                "path": item.path,
                "dest": dest,
                "maxSide": THUMB_MAX_SIDE,
                "quality": WEBP_QUALITY,
            }),
        )
        .await
    {
        // op succeeded — the WebP is on disk.
        Ok(Ok(_)) => Ok(()),
        // op ran but Python returned {error}: per-file decode/encode error.
        Ok(Err(op_err)) => Err(op_err),
        // transport/infra failure: don't persist; let the caller stop the batch.
        Err(transport_err) => return Err(AppError::Sidecar(transport_err.to_string())),
    };

    let succeeded = result.is_ok();
    let db = state.db.clone();
    let id = item.id.clone();
    let mtime_nanos = item.mtime_nanos;
    tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<()> {
        let conn = db.blocking_lock();
        persist_thumb(&conn, &id, result, mtime_nanos)
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))?;

    Ok(succeeded)
}

/// Write one thumbnail outcome into the photos row.
/// - Ok: `thumb_status='done'`, record `thumb_src_mtime`, clear `thumb_error`.
/// - Err: `thumb_status='failed'` + `thumb_error=<msg>`, and record
///   `thumb_src_mtime` too so an unchanged bad file is not re-encoded every run —
///   `needs_regen` only retries a failed photo once its source mtime changes
///   (without this a never-succeeded row stays NULL and is retried forever).
fn persist_thumb(
    conn: &Connection,
    id: &str,
    result: Result<(), String>,
    mtime_nanos: i64,
) -> rusqlite::Result<()> {
    match result {
        Ok(()) => {
            let mut stmt = conn.prepare_cached(
                "UPDATE photos SET thumb_status = 'done', thumb_src_mtime = ?2, \
                 thumb_error = NULL WHERE id = ?1",
            )?;
            stmt.execute(params![id, mtime_nanos])?;
        }
        Err(msg) => {
            let mut stmt = conn.prepare_cached(
                "UPDATE photos SET thumb_status = 'failed', thumb_error = ?2, \
                 thumb_src_mtime = ?3 WHERE id = ?1",
            )?;
            stmt.execute(params![id, msg, mtime_nanos])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJ: &str = "test-project";

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for sql in [
            include_str!("../../migrations/0001_initial.sql"),
            include_str!("../../migrations/0002_analysis.sql"),
            include_str!("../../migrations/0003_grouping.sql"),
            include_str!("../../migrations/0004_projects.sql"),
            include_str!("../../migrations/0005_thumbnails.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute(
            "INSERT INTO projects (id, name, created_at) \
             VALUES (?1, ?1, '2026-01-01T00:00:00Z')",
            params![PROJ],
        )
        .unwrap();
        conn
    }

    fn insert_pending(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES (?1, ?2, ?3, 'pending', '2026-01-01T00:00:00Z')",
            params![id, PROJ, format!("/{id}.jpg")],
        )
        .unwrap();
    }

    fn col_text(conn: &Connection, id: &str, col: &str) -> Option<String> {
        conn.query_row(
            &format!("SELECT {col} FROM photos WHERE id = ?1"),
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn col_i64(conn: &Connection, id: &str, col: &str) -> Option<i64> {
        conn.query_row(
            &format!("SELECT {col} FROM photos WHERE id = ?1"),
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    // --- needs_regen policy ------------------------------------------------

    #[test]
    fn pending_always_regenerates() {
        assert!(needs_regen("pending", None, 100, false));
        assert!(needs_regen("pending", Some(100), 100, true));
    }

    #[test]
    fn done_skips_when_file_present_and_mtime_matches() {
        assert!(!needs_regen("done", Some(100), 100, true));
    }

    #[test]
    fn done_regenerates_when_file_missing() {
        // thumb_status='done' but the webp was deleted off disk → re-make it.
        assert!(needs_regen("done", Some(100), 100, false));
    }

    #[test]
    fn done_regenerates_when_source_mtime_changed() {
        // in-place edit: stored mtime no longer matches the source → self-heal.
        assert!(needs_regen("done", Some(100), 200, true));
    }

    #[test]
    fn failed_retries_only_when_source_changed() {
        // unchanged bad file: do NOT retry every run.
        assert!(!needs_regen("failed", Some(100), 100, false));
        // source replaced: give it another chance.
        assert!(needs_regen("failed", Some(100), 200, false));
        // never generated (no stored mtime) but marked failed: source mtime
        // differs from None → retry.
        assert!(needs_regen("failed", None, 100, false));
    }

    // --- persist_thumb -----------------------------------------------------

    #[test]
    fn persist_ok_marks_done_and_records_mtime() {
        let conn = mem_conn();
        insert_pending(&conn, "a");

        persist_thumb(&conn, "a", Ok(()), 1_700_000_000_000_000_000).unwrap();

        assert_eq!(
            col_text(&conn, "a", "thumb_status").as_deref(),
            Some("done")
        );
        assert_eq!(
            col_i64(&conn, "a", "thumb_src_mtime"),
            Some(1_700_000_000_000_000_000)
        );
        assert_eq!(col_text(&conn, "a", "thumb_error"), None);
    }

    #[test]
    fn persist_err_marks_failed_and_records_mtime() {
        let conn = mem_conn();
        insert_pending(&conn, "b");

        persist_thumb(&conn, "b", Err("UnidentifiedImageError: bad".into()), 555).unwrap();

        assert_eq!(
            col_text(&conn, "b", "thumb_status").as_deref(),
            Some("failed")
        );
        assert_eq!(
            col_text(&conn, "b", "thumb_error").as_deref(),
            Some("UnidentifiedImageError: bad")
        );
        // why: mtime IS recorded on failure so an unchanged bad file is skipped
        // next run (see persist_thumb / needs_regen). Without this the row stays
        // NULL and is re-encoded forever.
        assert_eq!(col_i64(&conn, "b", "thumb_src_mtime"), Some(555));
    }

    #[test]
    fn failed_unchanged_source_is_skipped_next_run() {
        // Lifecycle: a per-file failure records the source mtime, so the same
        // unchanged bad file is NOT regenerated on the next run; a later mtime
        // change (file replaced) does trigger a retry.
        let conn = mem_conn();
        insert_pending(&conn, "d");
        persist_thumb(&conn, "d", Err("bad".into()), 100).unwrap();

        let stored = col_i64(&conn, "d", "thumb_src_mtime");
        assert_eq!(stored, Some(100));
        // unchanged source → skip.
        assert!(!needs_regen("failed", stored, 100, false));
        // source replaced (new mtime) → retry.
        assert!(needs_regen("failed", stored, 200, false));
    }

    #[test]
    fn persist_ok_after_failure_clears_error() {
        let conn = mem_conn();
        insert_pending(&conn, "c");
        persist_thumb(&conn, "c", Err("boom".into()), 0).unwrap();
        // a later successful run must clear the stale error.
        persist_thumb(&conn, "c", Ok(()), 42).unwrap();
        assert_eq!(
            col_text(&conn, "c", "thumb_status").as_deref(),
            Some("done")
        );
        assert_eq!(col_text(&conn, "c", "thumb_error"), None);
        assert_eq!(col_i64(&conn, "c", "thumb_src_mtime"), Some(42));
    }
}
