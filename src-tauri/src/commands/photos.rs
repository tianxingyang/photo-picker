use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use rusqlite::{params, Connection};
use serde_json::json;
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

/// Transcode a photo (any format, incl. HEIC) to a display-ready JPEG in the OS
/// temp directory, keyed by `source_path + mtime_nanos` so:
///   - cache hits are instant (file already exists → no re-transcode),
///   - a changed source file automatically gets a fresh key.
///
/// Returns the temp-file path as a string; the frontend layer calls
/// `convertFileSrc` on it (never exposes raw paths to components).
#[tauri::command]
pub async fn transcode_for_display(
    photo_id: String,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    // 1. Resolve source path from DB (components never pass raw paths).
    let db = state.db.clone();
    let photo_id_clone = photo_id.clone();
    let path = tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<Option<String>> {
        let conn = db.blocking_lock();
        let mut stmt = conn.prepare_cached("SELECT path FROM photos WHERE id = ?1")?;
        let mut rows = stmt.query(params![photo_id_clone])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))?
    .ok_or_else(|| AppError::NotFound(format!("no photo with id: {photo_id}")))?;

    // 2. Derive cache key: blake3(path | mtime_nanos). Source change → new key.
    let mtime_nanos = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos())
        .map_err(|e| AppError::Io(e.to_string()))?;
    let key = transcode_cache_key(&path, mtime_nanos).to_hex();
    let dest_dir = std::env::temp_dir().join("photo-picker-display");
    let dest = dest_dir.join(format!("{key}.jpg"));

    // 3. Cache hit — return immediately without re-transcoding.
    if dest.exists() {
        return Ok(dest.to_string_lossy().into_owned());
    }
    std::fs::create_dir_all(&dest_dir).map_err(|e| AppError::Io(e.to_string()))?;

    // 4. Call sidecar: clone Arc then release lock before .await (echo_via_sidecar pattern).
    let sidecar = {
        let guard = state.sidecar.lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::Sidecar("not started".into()))?
    };

    match sidecar
        .call(
            "transcode",
            json!({ "path": path, "dest": dest.to_string_lossy() }),
        )
        .await
    {
        Ok(Ok(_)) => Ok(dest.to_string_lossy().into_owned()),
        Ok(Err(e)) => Err(AppError::Sidecar(e)), // op-level (bad/unsupported file)
        Err(e) => Err(AppError::Sidecar(e.to_string())), // transport-level
    }
}

/// Derive a deterministic cache key for a transcode result.
/// Key = blake3(path + "|" + mtime_nanos_as_decimal) → 64 hex chars.
/// Changing either component (path renamed, file overwritten) produces a new key
/// and triggers a fresh transcode on the next request.
fn transcode_cache_key(path: &str, mtime_nanos: u128) -> blake3::Hash {
    let key_input = format!("{path}|{mtime_nanos}");
    blake3::hash(key_input.as_bytes())
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

    // --- transcode_cache_key tests ---

    #[test]
    fn cache_key_is_stable_for_same_inputs() {
        let k1 = transcode_cache_key("/photos/img.heic", 1_700_000_000_000_000_000);
        let k2 = transcode_cache_key("/photos/img.heic", 1_700_000_000_000_000_000);
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_differs_when_mtime_changes() {
        let k1 = transcode_cache_key("/photos/img.heic", 1_000);
        let k2 = transcode_cache_key("/photos/img.heic", 2_000);
        assert_ne!(k1, k2, "different mtime must produce a different key");
    }

    #[test]
    fn cache_key_differs_when_path_changes() {
        let k1 = transcode_cache_key("/photos/a.heic", 1_000);
        let k2 = transcode_cache_key("/photos/b.heic", 1_000);
        assert_ne!(k1, k2, "different path must produce a different key");
    }

    #[test]
    fn cache_key_hex_is_64_chars() {
        let k = transcode_cache_key("/foo/bar.heic", 0);
        assert_eq!(k.to_hex().len(), 64);
    }

    // --- NotFound path for transcode ---

    #[test]
    fn notfound_path_when_id_missing() {
        let conn = mem_conn();
        // Verify the SELECT returns no rows for an unknown id.
        let mut stmt = conn
            .prepare("SELECT path FROM photos WHERE id = ?1")
            .unwrap();
        let mut rows = stmt.query(params!["nonexistent"]).unwrap();
        assert!(
            rows.next().unwrap().is_none(),
            "unknown id must yield no rows"
        );
    }
}
