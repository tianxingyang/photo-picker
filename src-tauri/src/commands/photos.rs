use std::ffi::OsStr;
use std::path::{Path, PathBuf};
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

/// Result of an export: how many keeps landed, how many were renamed on a name
/// collision, and per-item failures (which never abort the whole export).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub exported: usize,
    pub renamed: usize,
    pub failed: Vec<ExportFailure>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFailure {
    pub source: String,
    pub reason: String,
}

/// Copy every `status='keep'` original into `dest_dir`, preserving the original
/// file name; on a name collision write `name (n).ext` instead. Sources are
/// strictly read-only — never moved, never modified. A per-item copy failure is
/// recorded in `summary.failed` and does not abort the export; an export with no
/// keeps returns `exported = 0` (not an error).
#[tauri::command]
pub async fn export_keep(
    dest_dir: String,
    state: State<'_, AppState>,
) -> Result<ExportSummary, AppError> {
    let dest = PathBuf::from(&dest_dir);
    // Target must already be a directory (pickFolder yields one; defensive check).
    if !dest.is_dir() {
        return Err(AppError::Validation(format!("not a directory: {dest_dir}")));
    }

    // 1. Read the keep source paths. DB work runs inside spawn_blocking and the
    //    lock is released before the (potentially long) copy phase below.
    let db = state.db.clone();
    let paths = tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<Vec<String>> {
        let conn = db.blocking_lock();
        select_keep_paths(&conn)
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))?;

    // 2. Copy. Pure file IO, no DB lock held — a long copy must not block other RPCs.
    let summary = tauri::async_runtime::spawn_blocking(move || copy_keeps(&paths, &dest))
        .await
        .map_err(|e| AppError::Io(e.to_string()))?;

    Ok(summary)
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

/// Pure DB helper — the `status='keep'` source paths, run inside spawn_blocking.
fn select_keep_paths(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("SELECT path FROM photos WHERE status = 'keep'")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Pure file helper — copy each source into `dest`, renaming on collision.
/// Sources are read-only (`std::fs::copy` reads the source, writes a new target;
/// never rename/remove). Never silently drops a keep: a source with no file name,
/// or an exhausted rename space, is recorded in `failed`.
fn copy_keeps(paths: &[String], dest: &Path) -> ExportSummary {
    let mut summary = ExportSummary {
        exported: 0,
        renamed: 0,
        failed: Vec::new(),
    };
    for src in paths {
        let src_path = Path::new(src);
        let file_name = match src_path.file_name() {
            Some(n) => n,
            None => {
                summary.failed.push(ExportFailure {
                    source: src.clone(),
                    reason: "source path has no file name".into(),
                });
                continue;
            }
        };
        let (target, renamed) = match resolve_target(dest, file_name) {
            Some(t) => t,
            None => {
                summary.failed.push(ExportFailure {
                    source: src.clone(),
                    reason: "too many name collisions (>9999) at destination".into(),
                });
                continue;
            }
        };
        // why: resolve-then-copy per item (not batch-resolve then batch-copy) so a
        // file written this iteration is on disk before the next exists() probe —
        // two cross-source same-name keeps both land (one original, one renamed).
        match std::fs::copy(src_path, &target) {
            Ok(_) => {
                summary.exported += 1;
                if renamed {
                    summary.renamed += 1;
                }
            }
            Err(e) => summary.failed.push(ExportFailure {
                source: src.clone(),
                reason: e.to_string(),
            }),
        }
    }
    summary
}

/// Find a non-colliding target path under `dest` for `file_name`:
/// absent → original name (`renamed = false`); present → probe `stem (1).ext`,
/// `stem (2).ext`, … up to 9999. Returns `None` if all 9999 slots are taken, so
/// `copy_keeps` records a failure instead of probing the disk forever.
fn resolve_target(dest: &Path, file_name: &OsStr) -> Option<(PathBuf, bool)> {
    let original = dest.join(file_name);
    if !original.exists() {
        return Some((original, false));
    }
    let name = Path::new(file_name);
    let stem = name
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = name.extension().map(|e| e.to_string_lossy().into_owned());
    for n in 1..=9999 {
        let renamed = match &ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dest.join(renamed);
        if !candidate.exists() {
            return Some((candidate, true));
        }
    }
    None
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

    // --- export_keep helpers: select_keep_paths / copy_keeps / resolve_target ---

    #[test]
    fn select_keep_paths_returns_only_keep() {
        let conn = mem_conn();
        insert_photo(&conn, "k1", "keep");
        insert_photo(&conn, "k2", "keep");
        insert_photo(&conn, "r1", "reject");
        insert_photo(&conn, "p1", "pending");
        let mut paths = select_keep_paths(&conn).unwrap();
        paths.sort();
        assert_eq!(paths, vec!["/k1.jpg".to_string(), "/k2.jpg".to_string()]);
    }

    #[test]
    fn resolve_target_no_conflict_keeps_original_name() {
        let dir = tempfile::tempdir().unwrap();
        let (target, renamed) = resolve_target(dir.path(), OsStr::new("a.jpg")).unwrap();
        assert_eq!(target, dir.path().join("a.jpg"));
        assert!(!renamed);
    }

    #[test]
    fn resolve_target_single_conflict_appends_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"x").unwrap();
        let (target, renamed) = resolve_target(dir.path(), OsStr::new("a.jpg")).unwrap();
        assert_eq!(target, dir.path().join("a (1).jpg"));
        assert!(renamed);
    }

    #[test]
    fn resolve_target_double_conflict_appends_two() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"x").unwrap();
        std::fs::write(dir.path().join("a (1).jpg"), b"y").unwrap();
        let (target, _) = resolve_target(dir.path(), OsStr::new("a.jpg")).unwrap();
        assert_eq!(target, dir.path().join("a (2).jpg"));
    }

    #[test]
    fn resolve_target_no_extension_appends_without_dot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("noext"), b"x").unwrap();
        let (target, renamed) = resolve_target(dir.path(), OsStr::new("noext")).unwrap();
        assert_eq!(target, dir.path().join("noext (1)"));
        assert!(renamed);
    }

    #[test]
    fn copy_keeps_cross_source_same_name_both_land() {
        let src1 = tempfile::tempdir().unwrap();
        let src2 = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let a1 = src1.path().join("a.jpg");
        let a2 = src2.path().join("a.jpg");
        std::fs::write(&a1, b"one").unwrap();
        std::fs::write(&a2, b"two").unwrap();
        let paths = vec![
            a1.to_string_lossy().into_owned(),
            a2.to_string_lossy().into_owned(),
        ];
        let summary = copy_keeps(&paths, dest.path());
        assert_eq!(summary.exported, 2);
        assert_eq!(summary.renamed, 1);
        assert!(summary.failed.is_empty());
        // first source keeps the original name, second is renamed; both land.
        assert_eq!(
            std::fs::read(dest.path().join("a.jpg")).unwrap(),
            b"one".to_vec()
        );
        assert_eq!(
            std::fs::read(dest.path().join("a (1).jpg")).unwrap(),
            b"two".to_vec()
        );
    }

    #[test]
    fn copy_keeps_missing_source_goes_to_failed_others_continue() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let good = src.path().join("good.jpg");
        std::fs::write(&good, b"ok").unwrap();
        let missing = src.path().join("missing.jpg"); // never created
        let paths = vec![
            missing.to_string_lossy().into_owned(),
            good.to_string_lossy().into_owned(),
        ];
        let summary = copy_keeps(&paths, dest.path());
        assert_eq!(summary.exported, 1);
        assert_eq!(summary.failed.len(), 1);
        assert!(dest.path().join("good.jpg").exists());
    }

    #[test]
    fn copy_keeps_source_without_file_name_is_reported_not_dropped() {
        let dest = tempfile::tempdir().unwrap();
        // ".." terminates in `..`, so Path::file_name() is None.
        let summary = copy_keeps(&["..".to_string()], dest.path());
        assert_eq!(summary.exported, 0);
        assert_eq!(
            summary.failed.len(),
            1,
            "an unnamed source must be reported, never silently dropped"
        );
    }

    #[test]
    fn copy_keeps_zero_keep_returns_empty_summary() {
        let dest = tempfile::tempdir().unwrap();
        let summary = copy_keeps(&[], dest.path());
        assert_eq!(summary.exported, 0);
        assert_eq!(summary.renamed, 0);
        assert!(summary.failed.is_empty());
    }

    #[test]
    fn copy_keeps_does_not_overwrite_existing_target_file() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        // a pre-existing destination file with known bytes must survive untouched.
        std::fs::write(dest.path().join("a.jpg"), b"preexisting").unwrap();
        let a = src.path().join("a.jpg");
        std::fs::write(&a, b"fresh").unwrap();
        let summary = copy_keeps(&[a.to_string_lossy().into_owned()], dest.path());
        assert_eq!(summary.exported, 1);
        assert_eq!(summary.renamed, 1);
        assert_eq!(
            std::fs::read(dest.path().join("a.jpg")).unwrap(),
            b"preexisting".to_vec()
        );
        assert_eq!(
            std::fs::read(dest.path().join("a (1).jpg")).unwrap(),
            b"fresh".to_vec()
        );
    }

    #[test]
    fn copy_keeps_source_stays_byte_identical_and_unmodified() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let a = src.path().join("photo.jpg");
        let bytes = b"\x00\x01\x02 binary \xff\xfe content".to_vec();
        std::fs::write(&a, &bytes).unwrap();
        let summary = copy_keeps(&[a.to_string_lossy().into_owned()], dest.path());
        assert_eq!(summary.exported, 1);
        // source still present and unchanged.
        assert!(a.exists());
        assert_eq!(std::fs::read(&a).unwrap(), bytes);
        // target is a byte-for-byte copy.
        assert_eq!(std::fs::read(dest.path().join("photo.jpg")).unwrap(), bytes);
    }
}
