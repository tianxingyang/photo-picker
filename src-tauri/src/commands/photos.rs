use std::path::PathBuf;

use rusqlite::{params, Connection};
use tauri::State;

use crate::error::AppError;
use crate::scanner::{self, ScanOutcome};
use crate::AppState;

/// Valid `photos.status` enum values. Command-level check maps an unknown value
/// to `Validation`; the DB `CHECK` constraint in `0001_initial.sql` is the backstop.
const STATUSES: [&str; 3] = ["pending", "keep", "reject"];

#[tauri::command]
pub async fn scan_folder(
    path: String,
    state: State<'_, AppState>,
) -> Result<ScanOutcome, AppError> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(AppError::NotFound(format!("not a directory: {path}")));
    }

    // why: clone the Arc so the DB Connection is locked inside the blocking
    // thread (blocking_lock), never held across the command's .await.
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.blocking_lock();
        scanner::scan_folder(&conn, &root)
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))
}

/// Set one photo's keep/reject/pending status. Single-row, idempotent UPDATE —
/// no transaction, no single-flight guard (unlike group_photos' delete+reinsert),
/// because a single-row UPDATE is atomic within SQLite.
#[tauri::command]
pub async fn set_status(
    photo_id: String,
    status: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    // command-level enum check → Validation; DB CHECK is the backstop.
    if !STATUSES.contains(&status.as_str()) {
        return Err(AppError::Validation(format!("invalid status: {status}")));
    }
    // why: clone the Arc so the DB Connection is locked inside the blocking
    // thread (blocking_lock), never held across the command's .await.
    let db = state.db.clone();
    let rows = tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<usize> {
        let conn = db.blocking_lock();
        update_status(&conn, &photo_id, &status)
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))?;

    if rows == 0 {
        return Err(AppError::NotFound("no photo with that id".into()));
    }
    Ok(())
}

/// Pure DB helper — runs inside spawn_blocking, returns rows affected.
fn update_status(conn: &Connection, id: &str, status: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE photos SET status = ?2 WHERE id = ?1",
        params![id, status],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/0001_initial.sql"))
            .unwrap();
        conn
    }

    fn insert_photo(conn: &Connection, id: &str, status: &str) {
        conn.execute(
            "INSERT INTO photos (id, path, status, created_at) \
             VALUES (?1, ?2, ?3, '2026-01-01T00:00:00Z')",
            params![id, format!("/{id}.jpg"), status],
        )
        .unwrap();
    }

    fn status_of(conn: &Connection, id: &str) -> String {
        conn.query_row(
            "SELECT status FROM photos WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn update_status_valid_value_persists() {
        let conn = mem_conn();
        insert_photo(&conn, "a", "pending");
        let rows = update_status(&conn, "a", "keep").unwrap();
        assert_eq!(rows, 1);
        assert_eq!(status_of(&conn, "a"), "keep");
    }

    #[test]
    fn update_status_missing_id_affects_zero_rows() {
        let conn = mem_conn();
        // command maps rows == 0 → NotFound.
        let rows = update_status(&conn, "nope", "keep").unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn db_check_rejects_invalid_status() {
        let conn = mem_conn();
        insert_photo(&conn, "a", "pending");
        // backstop: even bypassing the command enum check, the CHECK constraint
        // rejects an out-of-enum value.
        let err = update_status(&conn, "a", "bogus");
        assert!(err.is_err(), "DB CHECK must reject invalid status");
        // unchanged on rejection.
        assert_eq!(status_of(&conn, "a"), "pending");
    }

    #[test]
    fn statuses_constant_accepts_only_known_values() {
        for ok in ["pending", "keep", "reject"] {
            assert!(STATUSES.contains(&ok), "{ok} must be valid");
        }
        for bad in ["", "Keep", "deleted", "unknown"] {
            assert!(!STATUSES.contains(&bad), "{bad} must be rejected");
        }
    }
}
