use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{params, Connection};
use serde_json::json;
use tauri::State;

use crate::commands::current_project;
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
    // Scope the import to the open project — photo ids derive from it (R1/R3).
    let project_id = current_project(&state)?;

    // why: clone the Arc so the DB Connection is locked inside the blocking
    // thread (blocking_lock), never held across the command's .await.
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.blocking_lock();
        scanner::scan_folder(&conn, &project_id, &root)
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
/// collision, how many were skipped because the source already lives in the
/// destination folder, and per-item failures (which never abort the whole export).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub exported: usize,
    pub renamed: usize,
    pub skipped: usize,
    pub failed: Vec<ExportFailure>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFailure {
    pub source: String,
    pub reason: String,
}

/// Copy every `status='keep'` original into `dest_dir`, preserving the original
/// file name; on a name collision write `name (n).ext` instead. A keep that
/// already lives directly in `dest_dir` is skipped (counted in `summary.skipped`)
/// rather than cloned next to itself. Sources are strictly read-only — never
/// moved, never modified. A per-item copy failure is recorded in `summary.failed`
/// and does not abort the export; an export with no keeps returns `exported = 0`
/// (not an error).
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
    // Export only the open project's keeps.
    let project_id = current_project(&state)?;

    // 1. Read the keep source paths. DB work runs inside spawn_blocking and the
    //    lock is released before the (potentially long) copy phase below.
    let db = state.db.clone();
    let paths = tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<Vec<String>> {
        let conn = db.blocking_lock();
        select_keep_paths(&conn, &project_id)
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

/// Pure DB helper — the open project's `status='keep'` source paths, run inside
/// spawn_blocking.
fn select_keep_paths(conn: &Connection, project_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt =
        conn.prepare_cached("SELECT path FROM photos WHERE status = 'keep' AND project_id = ?1")?;
    let rows = stmt.query_map(params![project_id], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Pure file helper — copy each source into `dest`, renaming on collision.
/// Sources are read-only (`std::fs::copy` reads the source, writes a new target;
/// never rename/remove). A source already living directly in `dest` is skipped
/// (counted in `skipped`) so we never clone a keep next to itself. Never silently
/// drops a keep: a source with no file name, or an exhausted rename space, is
/// recorded in `failed`.
fn copy_keeps(paths: &[String], dest: &Path) -> ExportSummary {
    let mut summary = ExportSummary {
        exported: 0,
        renamed: 0,
        skipped: 0,
        failed: Vec::new(),
    };
    // why: canonicalize dest once so the "source already in dest" check below
    // compares fully-resolved paths (handles `..`, symlinks, and Windows
    // `\\?\` / case differences). On filesystems where canonicalize fails even
    // though `is_dir()` passed (WebDAV / some NAS mounts), the check falls back
    // to a lexical comparison inside `source_already_in_dest` instead of being
    // disabled — otherwise every keep already in dest would be re-cloned as
    // `name (n).ext` on each run.
    let dest_canon = std::fs::canonicalize(dest).ok();
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
        // why: a keep whose folder IS the export target is already at the
        // destination; copying it would write `name (1).ext` beside the original
        // and grow the source library on every re-run. Skip it (a no-op), don't
        // clone it onto itself. A deleted/moved keep falls through to the copy
        // below and is recorded in `failed` — never disguised as a skip.
        if source_already_in_dest(src_path, dest, dest_canon.as_deref()) {
            summary.skipped += 1;
            continue;
        }
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
            Err(e) => {
                // why: a mid-copy failure (disk full, source vanishing) can leave
                // a truncated target under the canonical name; a later retry would
                // then see it in resolve_target, divert the good bytes to
                // `name (1).ext`, and the corrupt file would keep the original
                // name forever, indistinguishable from a clean export.
                // resolve_target only hands out paths that did not exist, so this
                // can only delete what the failed copy itself created. Best-effort:
                // a cleanup error must not mask the copy error being reported.
                let _ = std::fs::remove_file(&target);
                summary.failed.push(ExportFailure {
                    source: src.clone(),
                    reason: e.to_string(),
                });
            }
        }
    }
    summary
}

/// True when `src_path` provably lives directly in `dest`, so exporting it
/// would clone it next to itself. Two tiers:
///   1. canonical — fully-resolved comparison (`..`, symlinks, Windows `\\?\` /
///      case) when the filesystem supports canonicalize on both sides;
///   2. lexical fallback when canonicalize fails on either side (WebDAV / some
///      NAS / virtual filesystems): normalize `.`/`..` components and compare.
///      Gated on the source file existing, so a deleted keep still reaches the
///      copy and lands in `failed`, never disguised as a skip.
fn source_already_in_dest(src_path: &Path, dest: &Path, dest_canon: Option<&Path>) -> bool {
    if let Some(dest_canon) = dest_canon {
        if let Ok(src_canon) = std::fs::canonicalize(src_path) {
            return src_canon.parent() == Some(dest_canon);
        }
    }
    if !src_path.is_file() {
        return false; // missing keep must surface as a copy failure, not a skip
    }
    match src_path.parent() {
        Some(parent) => paths_lexically_equal(parent, dest),
        None => false,
    }
}

/// Lexical (no-IO) path equality for the canonicalize-unavailable fallback:
/// drop `.`, resolve `..` against preceding normal components, then compare —
/// case-insensitively for ASCII on Windows (drive letters, Latin names; full
/// Unicode case folding is filesystem-specific and out of scope for a
/// best-effort guard).
fn paths_lexically_equal(a: &Path, b: &Path) -> bool {
    let (a, b) = (lexical_normalize(a), lexical_normalize(b));
    if a == b {
        return true;
    }
    if cfg!(windows) {
        return a
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy());
    }
    false
}

fn lexical_normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                // only pop a normal component; `..` at or above the root stays.
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
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

    const PROJ: &str = "test-project";

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for sql in [
            include_str!("../../migrations/0001_initial.sql"),
            include_str!("../../migrations/0002_analysis.sql"),
            include_str!("../../migrations/0003_grouping.sql"),
            include_str!("../../migrations/0004_projects.sql"),
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

    fn insert_photo(conn: &Connection, id: &str, status: &str) {
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES (?1, ?2, ?3, ?4, '2026-01-01T00:00:00Z')",
            params![id, PROJ, format!("/{id}.jpg"), status],
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
        let mut paths = select_keep_paths(&conn, PROJ).unwrap();
        paths.sort();
        assert_eq!(paths, vec!["/k1.jpg".to_string(), "/k2.jpg".to_string()]);
    }

    #[test]
    fn select_keep_paths_is_project_scoped() {
        // A keep in another project must never appear in this project's export.
        let conn = mem_conn();
        conn.execute(
            "INSERT INTO projects (id, name, created_at) \
             VALUES ('other', 'other', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        insert_photo(&conn, "mine", "keep"); // belongs to PROJ
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES ('theirs', 'other', '/theirs.jpg', 'keep', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        let paths = select_keep_paths(&conn, PROJ).unwrap();
        assert_eq!(
            paths,
            vec!["/mine.jpg".to_string()],
            "other project's keep excluded"
        );
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

    #[test]
    fn copy_keeps_source_already_in_dest_is_skipped_not_duplicated() {
        let dest = tempfile::tempdir().unwrap();
        // the keep's source IS a file sitting directly in the export target.
        let inside = dest.path().join("a.jpg");
        std::fs::write(&inside, b"x").unwrap();
        let summary = copy_keeps(&[inside.to_string_lossy().into_owned()], dest.path());
        assert_eq!(
            summary.exported, 0,
            "a source already at dest must not be re-copied"
        );
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.renamed, 0);
        assert!(summary.failed.is_empty());
        // the pre-fix behavior cloned it next to itself as "a (1).jpg".
        assert!(
            !dest.path().join("a (1).jpg").exists(),
            "must not duplicate a keep into its own folder"
        );
        // original stays byte-identical.
        assert_eq!(std::fs::read(&inside).unwrap(), b"x".to_vec());
    }

    #[test]
    fn copy_keeps_source_in_subdir_of_dest_is_copied_not_skipped() {
        let dest = tempfile::tempdir().unwrap();
        // a source in a SUBfolder of dest is a genuine export, not a self-copy.
        let sub = dest.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let s = sub.join("a.jpg");
        std::fs::write(&s, b"x").unwrap();
        let summary = copy_keeps(&[s.to_string_lossy().into_owned()], dest.path());
        assert_eq!(summary.exported, 1);
        assert_eq!(summary.skipped, 0);
        assert_eq!(
            std::fs::read(dest.path().join("a.jpg")).unwrap(),
            b"x".to_vec()
        );
    }

    #[test]
    fn copy_keeps_failed_copy_leaves_no_target_residue() {
        let dest = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        // a directory as source makes fs::copy fail; the contract pinned here is
        // that a failed item leaves NOTHING behind in dest (a mid-copy failure
        // like disk-full would otherwise leave a truncated file that a retry
        // then shadows behind "name (1).ext").
        let dir_source = src.path().join("not-a-file.jpg");
        std::fs::create_dir(&dir_source).unwrap();
        let summary = copy_keeps(&[dir_source.to_string_lossy().into_owned()], dest.path());
        assert_eq!(summary.exported, 0);
        assert_eq!(summary.failed.len(), 1);
        assert_eq!(
            std::fs::read_dir(dest.path()).unwrap().count(),
            0,
            "a failed copy must not leave a partial target behind"
        );
    }

    // --- source_already_in_dest: lexical fallback when canonicalize fails ---
    // (dest_canon = None simulates a filesystem — WebDAV / some NAS — where
    // std::fs::canonicalize errors even though the directory exists.)

    #[test]
    fn fallback_source_in_dest_is_still_skipped_without_canonicalize() {
        let dest = tempfile::tempdir().unwrap();
        let inside = dest.path().join("a.jpg");
        std::fs::write(&inside, b"x").unwrap();
        assert!(
            source_already_in_dest(&inside, dest.path(), None),
            "canonicalize being unavailable must not disable the self-clone guard"
        );
    }

    #[test]
    fn fallback_missing_source_is_not_a_skip() {
        let dest = tempfile::tempdir().unwrap();
        let missing = dest.path().join("gone.jpg"); // never created
        assert!(
            !source_already_in_dest(&missing, dest.path(), None),
            "a deleted keep must fall through to the copy and land in failed"
        );
    }

    #[test]
    fn fallback_subdir_source_is_not_a_skip() {
        let dest = tempfile::tempdir().unwrap();
        let sub = dest.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let s = sub.join("a.jpg");
        std::fs::write(&s, b"x").unwrap();
        assert!(!source_already_in_dest(&s, dest.path(), None));
    }

    #[test]
    fn fallback_normalizes_dot_and_dotdot_components() {
        let dest = tempfile::tempdir().unwrap();
        let inside = dest.path().join("a.jpg");
        std::fs::write(&inside, b"x").unwrap();
        // same file spelled via `./sub/..` indirection. `sub` must exist: Unix
        // resolves path components against the filesystem even for `..`.
        std::fs::create_dir(dest.path().join("sub")).unwrap();
        let spelled = dest.path().join(".").join("sub").join("..").join("a.jpg");
        assert!(source_already_in_dest(&spelled, dest.path(), None));
    }

    #[cfg(windows)]
    #[test]
    fn fallback_is_ascii_case_insensitive_on_windows() {
        let dest = tempfile::tempdir().unwrap();
        let inside = dest.path().join("a.jpg");
        std::fs::write(&inside, b"x").unwrap();
        let upper = PathBuf::from(dest.path().to_string_lossy().to_uppercase()).join("a.jpg");
        assert!(source_already_in_dest(&upper, dest.path(), None));
    }

    #[test]
    fn copy_keeps_missing_source_in_dest_is_failed_not_skipped() {
        let dest = tempfile::tempdir().unwrap();
        // a stale keep whose recorded path is inside dest, but the file is gone:
        // the skip must NOT swallow it — a missing keep is a failure, not a no-op.
        let missing = dest.path().join("gone.jpg"); // never created on disk
        let summary = copy_keeps(&[missing.to_string_lossy().into_owned()], dest.path());
        assert_eq!(
            summary.skipped, 0,
            "a deleted keep must not be disguised as a skip"
        );
        assert_eq!(summary.exported, 0);
        assert_eq!(
            summary.failed.len(),
            1,
            "a missing keep whose folder is the target must still surface as a failure"
        );
    }
}
