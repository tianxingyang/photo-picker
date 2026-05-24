use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeSummary {
    pub analyzed: u32,
    pub failed: u32,
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

/// Analyze every `analysis_state='pending'` photo by dispatching the single
/// `analyze` op serially through the sidecar, persisting each result before
/// moving on (incremental: an interruption never loses completed progress).
/// Idempotent — already-`done` rows are skipped, so a re-run is a no-op.
#[tauri::command]
pub async fn analyze_pending(state: State<'_, AppState>) -> Result<AnalyzeSummary, AppError> {
    // why: clone the Arc under a brief lock so the outer Mutex is released
    // before any RPC await — never hold the sidecar guard across .await.
    let sidecar = {
        let guard = state.sidecar.lock().await;
        guard.as_ref().cloned().ok_or_else(|| {
            AppError::Sidecar("not started; check that uv and python are on PATH".into())
        })?
    };

    let pending = {
        let db = state.db.clone();
        tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<Vec<(String, String)>> {
            let conn = db.blocking_lock();
            let mut stmt = conn
                .prepare_cached("SELECT id, path FROM photos WHERE analysis_state = 'pending'")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?
    };

    let mut analyzed = 0u32;
    let mut failed = 0u32;
    for (id, path) in pending {
        let outcome = analyze_one(&sidecar, &state, &id, &path).await?;
        if outcome {
            analyzed += 1;
        } else {
            failed += 1;
        }
    }

    Ok(AnalyzeSummary { analyzed, failed })
}

/// Analyze one photo and persist the outcome. Returns Ok(true) on a stored
/// success, Ok(false) on a stored failure (bad file / decode error). Only a DB
/// or join failure propagates as Err.
///
/// D-conc replaceability: the whole "analyze one + persist" step lives here, so
/// swapping the serial loop for a sidecar process pool later touches only the
/// caller — not persistence or schema.
async fn analyze_one(
    sidecar: &crate::sidecar::Sidecar,
    state: &State<'_, AppState>,
    id: &str,
    path: &str,
) -> Result<bool, AppError> {
    let call = sidecar.call("analyze", json!({ "path": path })).await;
    let result: Result<AnalysisResult, String> = match call {
        Ok(value) => serde_json::from_value(value).map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
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

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/0001_initial.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../../migrations/0002_analysis.sql"))
            .unwrap();
        conn
    }

    fn insert_pending(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO photos (id, path, status, created_at) \
             VALUES (?1, ?2, 'pending', '2026-01-01T00:00:00Z')",
            params![id, path],
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
