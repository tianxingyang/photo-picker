use std::error::Error;
use std::path::Path;

use rusqlite::{params, Connection};
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use walkdir::WalkDir;

type ScanErr = Box<dyn Error + Send + Sync>;

const SUPPORTED_EXTS: [&str; 5] = ["jpg", "jpeg", "png", "heic", "heif"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoRow {
    pub id: String,
    pub path: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub photos: Vec<PhotoRow>,
    /// Count of walk entries that errored (permission denied, unreadable
    /// subtree, …) and were skipped — surfaced so a partial scan is never
    /// reported to the user as a complete import.
    pub skipped: u32,
}

/// Recursively scan `root`, filter to supported image extensions, and
/// incrementally upsert each into `photos` under `project_id`. Returns every
/// matched photo with its current DB state (so a re-scan reports
/// already-kept/rejected photos truthfully, not as fresh `pending`) plus the
/// count of unreadable entries. The photo id is `blake3(project_id + '\n' +
/// path)`, so the same path scanned into two projects yields two independent
/// rows.
///
/// `on_progress(done, total)` reports import progress: during the walk
/// `total = None` (indeterminate, `done` = files discovered so far); during the
/// upsert `total = Some(matched_count)` (determinate). It is a decoupling seam —
/// the command passes a closure that emits the Tauri progress event, while the
/// scanner stays free of any UI dependency. Throttled so a huge tree doesn't
/// flood: every ~200 files while walking, every ~50 while inserting.
pub fn scan_folder(
    conn: &Connection,
    project_id: &str,
    root: &Path,
    on_progress: &dyn Fn(u32, Option<u32>),
) -> Result<ScanOutcome, ScanErr> {
    let mut matched: Vec<String> = Vec::new();
    let mut skipped: u32 = 0;
    let mut discovered: u32 = 0;
    for entry in WalkDir::new(root) {
        match entry {
            Ok(e) if e.file_type().is_file() && is_supported(e.path()) => {
                matched.push(e.path().to_string_lossy().into_owned());
                discovered += 1;
                if discovered.is_multiple_of(200) {
                    on_progress(discovered, None);
                }
            }
            Ok(_) => {}
            // why: a denied/unreadable subtree must not silently vanish from the
            // import; count it and keep going so the rest still indexes.
            Err(err) => {
                eprintln!("scan: skipped unreadable entry: {err}");
                skipped += 1;
            }
        }
    }

    let total = matched.len() as u32;
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
    let tx = conn.unchecked_transaction()?;
    let mut rows = Vec::with_capacity(matched.len());
    {
        let mut insert = tx.prepare_cached(
            "INSERT OR IGNORE INTO photos (id, project_id, path, status, created_at) \
             VALUES (?1, ?2, ?3, 'pending', ?4)",
        )?;
        let mut select =
            tx.prepare_cached("SELECT status, created_at FROM photos WHERE id = ?1")?;
        let mut indexed: u32 = 0;
        for path in matched {
            let id = photo_id(project_id, &path);
            insert.execute(params![id, project_id, path, now])?;
            let (status, created_at) =
                select.query_row(params![id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.push(PhotoRow {
                id,
                path,
                status,
                created_at,
            });
            indexed += 1;
            if indexed.is_multiple_of(50) || indexed == total {
                on_progress(indexed, Some(total));
            }
        }
    }
    tx.commit()?;
    Ok(ScanOutcome {
        photos: rows,
        skipped,
    })
}

/// Project-scoped photo id: `blake3(project_id + '\n' + path)` → 64 hex chars.
/// The `\n` separator keeps the two fields unambiguous; the same path under two
/// different project ids hashes to two different ids (independent records).
pub fn photo_id(project_id: &str, path: &str) -> String {
    blake3::hash(format!("{project_id}\n{path}").as_bytes())
        .to_hex()
        .to_string()
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const PROJ: &str = "test-project";

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(include_str!("../../migrations/0001_initial.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../../migrations/0002_analysis.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../../migrations/0003_grouping.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../../migrations/0004_projects.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, created_at) \
             VALUES (?1, ?1, '2026-01-01T00:00:00Z')",
            params![PROJ],
        )
        .unwrap();
        conn
    }

    #[test]
    fn filters_unsupported_and_recurses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("a.jpg"), b"x").unwrap();
        fs::write(root.join("b.PNG"), b"x").unwrap();
        fs::write(root.join("c.heic"), b"x").unwrap();
        fs::write(root.join("note.txt"), b"x").unwrap();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("d.jpeg"), b"x").unwrap();

        let conn = mem_conn();
        let outcome = scan_folder(&conn, PROJ, root, &|_, _| {}).unwrap();

        assert_eq!(
            outcome.photos.len(),
            4,
            "only the 4 image files, txt filtered out"
        );
        assert!(outcome.photos.iter().all(|r| r.status == "pending"));
        assert_eq!(outcome.skipped, 0, "readable tree skips nothing");
    }

    #[test]
    fn incremental_import_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.jpg"), b"x").unwrap();
        let conn = mem_conn();

        let first = scan_folder(&conn, PROJ, dir.path(), &|_, _| {}).unwrap();
        let second = scan_folder(&conn, PROJ, dir.path(), &|_, _| {}).unwrap();

        assert_eq!(first.photos.len(), 1);
        assert_eq!(second.photos.len(), 1);
        let count: i64 = conn
            .query_row("SELECT count(*) FROM photos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "re-import must not duplicate rows");
    }

    #[test]
    fn id_is_stable_blake3_of_project_and_path() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.jpg");
        fs::write(&f, b"x").unwrap();
        let conn = mem_conn();

        let outcome = scan_folder(&conn, PROJ, dir.path(), &|_, _| {}).unwrap();
        let id = &outcome.photos[0].id;

        assert_eq!(id.len(), 64);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        let expected = photo_id(PROJ, &f.to_string_lossy());
        assert_eq!(id, &expected);
    }

    #[test]
    fn same_path_in_two_projects_yields_independent_ids() {
        // R3/AC2: the same path scanned under two project ids must produce two
        // different photo ids (two independent records).
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.jpg");
        fs::write(&f, b"x").unwrap();
        let conn = mem_conn();
        conn.execute(
            "INSERT INTO projects (id, name, created_at) \
             VALUES ('other', 'other', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        let a = scan_folder(&conn, PROJ, dir.path(), &|_, _| {}).unwrap();
        let b = scan_folder(&conn, "other", dir.path(), &|_, _| {}).unwrap();

        assert_ne!(
            a.photos[0].id, b.photos[0].id,
            "same path, different project => different id"
        );
        let count: i64 = conn
            .query_row("SELECT count(*) FROM photos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "both projects hold an independent record");
    }

    #[test]
    fn rescan_preserves_existing_status() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.jpg");
        fs::write(&f, b"x").unwrap();
        let id = photo_id(PROJ, &f.to_string_lossy());
        let conn = mem_conn();
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES (?1, ?2, ?3, 'keep', '2026-01-01T00:00:00Z')",
            params![id, PROJ, f.to_string_lossy()],
        )
        .unwrap();

        let outcome = scan_folder(&conn, PROJ, dir.path(), &|_, _| {}).unwrap();

        assert_eq!(outcome.photos.len(), 1);
        assert_eq!(
            outcome.photos[0].status, "keep",
            "re-scan must not reset status"
        );
    }
}
