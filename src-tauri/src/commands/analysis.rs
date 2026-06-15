use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use futures::stream::StreamExt;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, State};

use crate::commands::{current_project, progress, sidecar_pool};
use crate::error::AppError;
use crate::AppState;

/// RAII guard for the single-flight `analysis_running` flag. Clearing on Drop
/// guarantees the flag is reset on every exit path (early return, `?`, break,
/// panic) so a future analyze_pending run can proceed.
struct RunGuard<'a>(&'a AtomicBool);

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeSummary {
    pub analyzed: u32,
    pub failed: u32,
    // why: true when the user cancelled mid-batch (remaining rows stay pending).
    // The frontend uses it to skip auto-grouping partial data after a cancel.
    pub cancelled: bool,
}

/// Deserialized `analyze` op success result. Field names match the camelCase
/// IPC contract (ARCHITECTURE.md §IPC).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub shot_at: Option<String>,
    pub blur_score: f64,
    pub is_blurry: bool,
    pub exposure_score: f64,
    pub exposure_flag: String,
    pub phash: String,
}

/// Per-photo dispatch outcome inside the bounded-concurrency stream.
enum Outcome {
    Ok,
    Failed,
    /// Cancelled before this photo started (or after an infra stop) — skipped,
    /// the row stays `pending`.
    Skipped,
    /// Transport/infra failure: stop feeding the rest of the batch.
    Transport(AppError),
}

/// Analyze every `analysis_state='pending'` photo by dispatching the `analyze`
/// op across the **sidecar pool** with one in-flight analysis per process,
/// persisting each result as it completes and emitting a progress event. Each
/// Python sidecar is single-threaded; running N of them is what gives multicore
/// analysis (in-process Python threads are unstable under piped stdio on
/// Windows — see lib.rs AppState).
///
/// `failed` counts per-file decode failures only (the op ran but the file was
/// bad), persisted as `analysis_state='failed'`. An infra failure (sidecar down
/// / timeout / DB error) sets the cancel flag and stops feeding new photos; the
/// remaining rows stay `pending` so a re-run resumes them. Already-`done` rows
/// are skipped, so a re-run is a no-op.
///
/// Cancellation: `cancel_analysis` sets `analysis_cancel`; not-yet-started
/// photos are skipped (stay `pending`), in-flight analyses finish and persist.
///
/// Single-flight: a concurrent invocation returns an empty summary immediately
/// rather than double-processing the same pending rows.
#[tauri::command]
pub async fn analyze_pending(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AnalyzeSummary, AppError> {
    // why: single-flight — bail out if another run already holds the flag.
    if state
        .analysis_running
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Ok(AnalyzeSummary {
            analyzed: 0,
            failed: 0,
            cancelled: false,
        });
    }
    // RAII: clears analysis_running on every exit path below (including `?`).
    let _guard = RunGuard(&state.analysis_running);
    // Fresh batch: clear any stale cancel from a previous run.
    state.analysis_cancel.store(false, Ordering::Release);

    // Analyze only the open project's pending photos.
    let project_id = current_project(&state)?;

    // Clone the sidecar pool out (cheap Arc clones); released before any await.
    // Concurrency = pool size: one in-flight analyze per sidecar process.
    let pool = sidecar_pool(&state).await?;
    let n = pool.len();

    let pending = {
        let db = state.db.clone();
        let project_id = project_id.clone();
        tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<Vec<(String, String)>> {
            let conn = db.blocking_lock();
            let mut stmt = conn.prepare_cached(
                "SELECT id, path FROM photos \
                 WHERE analysis_state = 'pending' AND project_id = ?1",
            )?;
            let rows = stmt
                .query_map(params![project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?
    };

    let total = pending.len() as u32;
    // why: out-of-order completions need a shared counter so progress is
    // monotonic regardless of which photo finishes first.
    let done = AtomicUsize::new(0);

    // Bounded-concurrency dispatch: up to `n` analyses in flight, each routed to
    // a distinct sidecar by index (idx % n). The futures borrow pool/state/app
    // for the lifetime of the stream (all consumed before this fn returns).
    let mut stream =
        futures::stream::iter(pending.into_iter().enumerate().map(|(idx, (id, path))| {
            let pool = &pool;
            let state = &state;
            let app = &app;
            let done = &done;
            async move {
                // why: check cancel BEFORE starting so a cancelled/​stopped batch
                // stops feeding new work; in-flight ones already past this point
                // finish and persist.
                if state.analysis_cancel.load(Ordering::Acquire) {
                    return Outcome::Skipped;
                }
                // Route to one sidecar; buffer_unordered(n) keeps ≤ n in flight, so
                // the first n items hit distinct processes (true parallelism).
                let sidecar = &pool[idx % n];
                match analyze_one(sidecar, state, &id, &path).await {
                    Ok(ok) => {
                        let d = done.fetch_add(1, Ordering::AcqRel) as u32 + 1;
                        progress::running(app, progress::PHASE_ANALYZE, d, Some(total));
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

    let mut analyzed = 0u32;
    let mut failed = 0u32;
    // why: an infra failure must be distinguishable from a user cancel — it
    // surfaces as an Err (so the frontend shows it and skips grouping) and a
    // distinct terminal status, not a silent "cancelled".
    let mut infra_err: Option<AppError> = None;
    while let Some(outcome) = stream.next().await {
        match outcome {
            Outcome::Ok => analyzed += 1,
            Outcome::Failed => failed += 1,
            Outcome::Skipped => {}
            Outcome::Transport(e) => {
                // Infra failure: stop feeding the rest of the batch (reuse the
                // cancel flag purely as the stop signal). Remaining rows stay
                // 'pending' (analyze_one never persisted them) and are retried on
                // re-run. Keep the first error to report to the caller.
                eprintln!("analyze_pending: stopping batch after infra error: {e}");
                state.analysis_cancel.store(true, Ordering::Release);
                if infra_err.is_none() {
                    infra_err = Some(e);
                }
            }
        }
    }

    // Terminal status: error > cancelled > done. An infra stop reports `error`
    // even though it also set the stop flag, so it never masquerades as a cancel.
    let user_cancelled = infra_err.is_none() && state.analysis_cancel.load(Ordering::Acquire);
    let status = if infra_err.is_some() {
        progress::STATUS_ERROR
    } else if user_cancelled {
        progress::STATUS_CANCELLED
    } else {
        progress::STATUS_DONE
    };
    progress::terminal(
        &app,
        progress::PHASE_ANALYZE,
        done.load(Ordering::Acquire) as u32,
        Some(total),
        status,
    );

    // why: propagate an infra failure so the frontend surfaces it and does NOT
    // proceed to grouping on partial data. Completed rows are already persisted,
    // so a re-run resumes the still-`pending` remainder.
    if let Some(e) = infra_err {
        return Err(e);
    }

    Ok(AnalyzeSummary {
        analyzed,
        failed,
        cancelled: user_cancelled,
    })
}

/// Cooperatively cancel an in-progress analyze batch. Sets `analysis_cancel`;
/// the dispatch loop stops starting new photos (they stay `pending`), in-flight
/// analyses finish and persist. A no-op if nothing is running.
#[tauri::command]
pub fn cancel_analysis(state: State<'_, AppState>) {
    state.analysis_cancel.store(true, Ordering::Release);
}

/// Analyze one photo and persist the outcome. Returns Ok(true) on a stored
/// success, Ok(false) on a stored per-file failure (bad file / decode error /
/// malformed result). A transport/infra failure (sidecar down / timeout)
/// propagates as Err WITHOUT persisting — the row stays 'pending' so a re-run
/// retries it. A DB or join failure during persist also propagates as Err.
///
/// D-conc replaceability: the whole "analyze one + persist" step lives here, so
/// the caller can drive it with bounded concurrency (see `analyze_pending`'s
/// `buffer_unordered`) without touching persistence or schema.
async fn analyze_one(
    sidecar: &crate::sidecar::Sidecar,
    state: &State<'_, AppState>,
    id: &str,
    path: &str,
) -> Result<bool, AppError> {
    // why: the sidecar transport is id-keyed and concurrency-safe, so the caller
    // runs up to pool-size of these at once, each routed to a DISTINCT sidecar
    // PROCESS (idx % n; no head-of-line blocking). A 30s CALL_TIMEOUT surfaces as a
    // transport Err here, which stops the batch cleanly (no false 'failed'
    // cascade); the row stays 'pending' and is retried on re-run.
    let result: Result<AnalysisResult, String> =
        match sidecar.call("analyze", json!({ "path": path })).await {
            // op succeeded: a malformed result for a valid op is a real per-file
            // problem, so a deserialize error is persisted as 'failed'.
            Ok(Ok(value)) => serde_json::from_value(value).map_err(|e| e.to_string()),
            // op ran but Python returned {error}: per-file decode error.
            Ok(Err(op_err)) => Err(op_err),
            // transport/infra failure: don't persist; let the caller stop the batch.
            Err(transport_err) => return Err(AppError::Sidecar(transport_err.to_string())),
        };

    let succeeded = result.is_ok();
    let db = state.db.clone();
    let id = id.to_owned();
    tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<()> {
        let conn = db.blocking_lock();
        persist_analysis(&conn, &id, result)
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))?;

    Ok(succeeded)
}

/// Write one analysis outcome into the wide `photos` row.
///
/// - Ok  => six analysis columns + `analysis_state='done'`, clear `analysis_error`.
/// - Err => `analysis_state='failed'` + `analysis_error=<msg>`; analysis columns untouched.
fn persist_analysis(
    conn: &Connection,
    id: &str,
    result: Result<AnalysisResult, String>,
) -> rusqlite::Result<()> {
    match result {
        Ok(r) => {
            let mut stmt = conn.prepare_cached(
                "UPDATE photos SET \
                 shot_at = ?2, blur_score = ?3, is_blurry = ?4, \
                 exposure_score = ?5, exposure_flag = ?6, phash = ?7, \
                 analysis_state = 'done', analysis_error = NULL \
                 WHERE id = ?1",
            )?;
            stmt.execute(params![
                id,
                r.shot_at,
                r.blur_score,
                r.is_blurry as i64,
                r.exposure_score,
                r.exposure_flag,
                r.phash,
            ])?;
        }
        Err(msg) => {
            let mut stmt = conn.prepare_cached(
                "UPDATE photos SET analysis_state = 'failed', analysis_error = ?2 WHERE id = ?1",
            )?;
            stmt.execute(params![id, msg])?;
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

    fn insert_pending(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES (?1, ?2, ?3, 'pending', '2026-01-01T00:00:00Z')",
            params![id, PROJ, path],
        )
        .unwrap();
    }

    fn sample_result() -> AnalysisResult {
        AnalysisResult {
            shot_at: Some("2026-05-24T10:30:00".into()),
            blur_score: 124.7,
            is_blurry: false,
            exposure_score: 0.42,
            exposure_flag: "normal".into(),
            phash: "ffc3a18000000000".into(),
        }
    }

    fn col_text(conn: &Connection, id: &str, col: &str) -> Option<String> {
        conn.query_row(
            &format!("SELECT {col} FROM photos WHERE id = ?1"),
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn col_f64(conn: &Connection, id: &str, col: &str) -> Option<f64> {
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

    #[test]
    fn persist_ok_writes_columns_and_marks_done() {
        let conn = mem_conn();
        insert_pending(&conn, "a", "/a.jpg");

        persist_analysis(&conn, "a", Ok(sample_result())).unwrap();

        assert_eq!(
            col_text(&conn, "a", "analysis_state").as_deref(),
            Some("done")
        );
        assert_eq!(
            col_text(&conn, "a", "shot_at").as_deref(),
            Some("2026-05-24T10:30:00")
        );
        assert_eq!(col_f64(&conn, "a", "blur_score"), Some(124.7));
        assert_eq!(col_i64(&conn, "a", "is_blurry"), Some(0));
        assert_eq!(col_f64(&conn, "a", "exposure_score"), Some(0.42));
        assert_eq!(
            col_text(&conn, "a", "exposure_flag").as_deref(),
            Some("normal")
        );
        assert_eq!(
            col_text(&conn, "a", "phash").as_deref(),
            Some("ffc3a18000000000")
        );
        assert_eq!(col_text(&conn, "a", "analysis_error"), None);
    }

    #[test]
    fn persist_err_marks_failed_and_records_error() {
        let conn = mem_conn();
        insert_pending(&conn, "b", "/b.jpg");

        persist_analysis(&conn, "b", Err("UnidentifiedImageError: bad".into())).unwrap();

        assert_eq!(
            col_text(&conn, "b", "analysis_state").as_deref(),
            Some("failed")
        );
        assert_eq!(
            col_text(&conn, "b", "analysis_error").as_deref(),
            Some("UnidentifiedImageError: bad")
        );
        assert_eq!(
            col_f64(&conn, "b", "blur_score"),
            None,
            "analysis columns stay untouched on failure"
        );
    }

    #[test]
    fn pending_query_returns_only_pending_rows() {
        let conn = mem_conn();
        insert_pending(&conn, "p1", "/p1.jpg");
        insert_pending(&conn, "p2", "/p2.jpg");
        insert_pending(&conn, "done", "/done.jpg");
        insert_pending(&conn, "fail", "/fail.jpg");
        persist_analysis(&conn, "done", Ok(sample_result())).unwrap();
        persist_analysis(&conn, "fail", Err("boom".into())).unwrap();

        let mut stmt = conn
            .prepare("SELECT id FROM photos WHERE analysis_state = 'pending' ORDER BY id")
            .unwrap();
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();

        assert_eq!(ids, vec!["p1".to_string(), "p2".to_string()]);
    }

    #[test]
    fn analysis_result_deserializes_camel_case() {
        let value = json!({
            "shotAt": null,
            "blurScore": 12.5,
            "isBlurry": true,
            "exposureScore": 0.9,
            "exposureFlag": "over",
            "phash": "8000000000000000"
        });
        let r: AnalysisResult = serde_json::from_value(value).unwrap();
        assert_eq!(r.shot_at, None);
        assert!(r.is_blurry);
        assert_eq!(r.exposure_flag, "over");
    }
}
