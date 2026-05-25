use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::json;
use tauri::State;

use crate::error::AppError;
use crate::grouping::{cluster, parse_phash};
use crate::AppState;

/// Hamming-distance threshold (64-bit pHash) for the `phash_burst` method.
/// Stored into each group's `params` so a future re-calibration is auditable.
const PHASH_THRESHOLD: u32 = 8;

/// Grouping method tag written to `similar_groups.method`. M1 ships only this
/// one; M3 (CLIP / faces) adds new method values reusing the same two tables.
const METHOD: &str = "phash_burst";

/// RAII guard for the single-flight `grouping_running` flag. Clearing on Drop
/// guarantees the flag is reset on every exit path (early return, `?`, panic).
struct RunGuard<'a>(&'a AtomicBool);

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    pub groups: u32,
    pub grouped_photos: u32,
}

/// Re-cluster all analysed photos by pHash near-duplication and persist the
/// result into `similar_groups` + `group_members` under `method='phash_burst'`.
///
/// Idempotent by construction: each run first deletes the existing
/// `phash_burst` groups (CASCADE clears their members) then re-derives every
/// group id from its sorted member ids, so the same input yields byte-identical
/// rows. Decoupled from analysis — the frontend calls it after `analyze_pending`
/// completes or on a manual "re-group".
///
/// Single-flight: a concurrent invocation returns a zero summary immediately
/// rather than double-clustering against the same rows.
#[tauri::command]
pub async fn group_photos(state: State<'_, AppState>) -> Result<GroupSummary, AppError> {
    // why: single-flight — bail out if another run already holds the flag.
    if state
        .grouping_running
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Ok(GroupSummary {
            groups: 0,
            grouped_photos: 0,
        });
    }
    // RAII: clears grouping_running on every exit path below (including `?`).
    let _guard = RunGuard(&state.grouping_running);

    let db = state.db.clone();
    let summary =
        tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<GroupSummary> {
            let conn = db.blocking_lock();
            regroup(&conn)
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?;

    Ok(summary)
}

/// Load `(id, phash)` for analysed photos, cluster, and rewrite the
/// `phash_burst` groups in one transaction. Pure DB work — runs inside
/// `spawn_blocking`, never holds the lock across `.await`.
fn regroup(conn: &Connection) -> rusqlite::Result<GroupSummary> {
    // why: collect owned (id, phash_u64) inside the closure — never leak Row
    // borrows; skip rows whose phash fails to parse rather than crash.
    let raw = {
        let mut stmt =
            conn.prepare_cached("SELECT id, phash FROM photos WHERE phash IS NOT NULL")?;
        let rows = stmt
            .query_map([], |r| {
                let id: String = r.get(0)?;
                let phash: String = r.get(1)?;
                Ok((id, phash))
            })?
            .collect::<rusqlite::Result<Vec<(String, String)>>>();
        rows?
    };
    // why: parse phash to u64 here so unparseable rows are silently skipped
    // rather than crashing the whole grouping run.
    let items: Vec<(String, u64)> = raw
        .into_iter()
        .filter_map(|(id, phash)| parse_phash(&phash).map(|h| (id, h)))
        .collect();

    let comps = cluster(&items, PHASH_THRESHOLD);
    let params_json = json!({ "threshold": PHASH_THRESHOLD, "version": 1 }).to_string();

    let tx = conn.unchecked_transaction()?;
    {
        // CASCADE drops member rows for the old phash_burst groups too.
        tx.execute(
            "DELETE FROM similar_groups WHERE method = ?1",
            params![METHOD],
        )?;

        let mut insert_group = tx.prepare_cached(
            "INSERT INTO similar_groups (id, method, params) VALUES (?1, ?2, ?3)",
        )?;
        let mut insert_member =
            tx.prepare_cached("INSERT INTO group_members (group_id, photo_id) VALUES (?1, ?2)")?;

        let mut grouped_photos = 0u32;
        for comp in &comps {
            let gid = derive_id(METHOD, comp);
            insert_group.execute(params![gid, METHOD, params_json])?;
            for photo_id in comp {
                insert_member.execute(params![gid, photo_id])?;
                grouped_photos += 1;
            }
        }

        // Drop the cached statements before committing the borrow of `tx`.
        drop(insert_group);
        drop(insert_member);
        tx.commit()?;

        Ok(GroupSummary {
            groups: comps.len() as u32,
            grouped_photos,
        })
    }
}

/// Content-derived group id: `blake3(method + "\n" + sorted_member_ids joined
/// by "\n")`. Same membership => same id => byte-identical row on re-run, which
/// is what makes `group_photos` idempotent. `members` is already sorted by
/// `cluster`, but we don't rely on that here.
fn derive_id(method: &str, members: &[String]) -> String {
    let mut sorted = members.to_vec();
    sorted.sort();
    let mut payload = String::from(method);
    for m in &sorted {
        payload.push('\n');
        payload.push_str(m);
    }
    blake3::hash(payload.as_bytes()).to_hex().to_string()
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
        conn.execute_batch(include_str!("../../migrations/0003_grouping.sql"))
            .unwrap();
        conn
    }

    /// Insert a photo with a given (or NULL) phash already analysed.
    fn insert_photo(conn: &Connection, id: &str, phash: Option<&str>) {
        conn.execute(
            "INSERT INTO photos (id, path, status, created_at, analysis_state, phash) \
             VALUES (?1, ?2, 'pending', '2026-01-01T00:00:00Z', 'done', ?3)",
            params![id, format!("/{id}.jpg"), phash],
        )
        .unwrap();
    }

    fn members_of(conn: &Connection, gid: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT photo_id FROM group_members WHERE group_id = ?1 ORDER BY photo_id")
            .unwrap();
        stmt.query_map(params![gid], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    fn all_group_ids(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT id FROM similar_groups ORDER BY id")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    #[test]
    fn known_phashes_persist_expected_partition() {
        let conn = mem_conn();
        // a~b within threshold; c~d within threshold; the two pairs far apart.
        insert_photo(&conn, "a", Some("0000000000000000"));
        insert_photo(&conn, "b", Some("0000000000000003")); // 2 bits from a
        insert_photo(&conn, "c", Some("ffffffffffffffff"));
        insert_photo(&conn, "d", Some("fffffffffffffffc")); // 2 bits from c

        let summary = regroup(&conn).unwrap();
        assert_eq!(summary.groups, 2);
        assert_eq!(summary.grouped_photos, 4);

        let gids = all_group_ids(&conn);
        assert_eq!(gids.len(), 2);
        // Exactly one group is {a,b} and one is {c,d}.
        let partitions: Vec<Vec<String>> = gids.iter().map(|g| members_of(&conn, g)).collect();
        assert!(partitions.contains(&vec!["a".to_string(), "b".to_string()]));
        assert!(partitions.contains(&vec!["c".to_string(), "d".to_string()]));
    }

    #[test]
    fn isolated_row_produces_no_member_row() {
        let conn = mem_conn();
        insert_photo(&conn, "a", Some("0000000000000000"));
        insert_photo(&conn, "b", Some("0000000000000003")); // ~a
        insert_photo(&conn, "lonely", Some("ffffffffffffffff")); // far from both

        let summary = regroup(&conn).unwrap();
        assert_eq!(summary.groups, 1);
        assert_eq!(summary.grouped_photos, 2);

        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM group_members WHERE photo_id = 'lonely'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "isolated photo never appears as a member");
    }

    #[test]
    fn null_phash_rows_are_filtered() {
        let conn = mem_conn();
        insert_photo(&conn, "a", Some("0000000000000000"));
        insert_photo(&conn, "b", Some("0000000000000003"));
        insert_photo(&conn, "no_phash", None); // not analysed for phash
        insert_photo(&conn, "bad_phash", Some("zzzz")); // unparseable, skipped

        let summary = regroup(&conn).unwrap();
        assert_eq!(summary.groups, 1);
        assert_eq!(summary.grouped_photos, 2);
        for absent in ["no_phash", "bad_phash"] {
            let count: i64 = conn
                .query_row(
                    "SELECT count(*) FROM group_members WHERE photo_id = ?1",
                    params![absent],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "{absent} must not be grouped");
        }
    }

    #[test]
    fn rerun_is_byte_identical_and_leaves_no_stale_groups() {
        let conn = mem_conn();
        insert_photo(&conn, "a", Some("0000000000000000"));
        insert_photo(&conn, "b", Some("0000000000000003"));
        insert_photo(&conn, "c", Some("ffffffffffffffff"));
        insert_photo(&conn, "d", Some("fffffffffffffffc"));

        regroup(&conn).unwrap();
        let gids_first = all_group_ids(&conn);
        let members_first: Vec<Vec<String>> =
            gids_first.iter().map(|g| members_of(&conn, g)).collect();

        // Second run: same input => same ids, same members, no stale rows.
        let summary = regroup(&conn).unwrap();
        assert_eq!(summary.groups, 2);

        let gids_second = all_group_ids(&conn);
        let members_second: Vec<Vec<String>> =
            gids_second.iter().map(|g| members_of(&conn, g)).collect();

        assert_eq!(gids_first, gids_second, "group ids stable across re-run");
        assert_eq!(members_first, members_second, "memberships stable");

        let total_groups: i64 = conn
            .query_row("SELECT count(*) FROM similar_groups", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total_groups, 2, "no stale phash_burst groups remain");
    }

    #[test]
    fn rerun_after_membership_change_drops_old_group() {
        let conn = mem_conn();
        insert_photo(&conn, "a", Some("0000000000000000"));
        insert_photo(&conn, "b", Some("0000000000000003"));

        regroup(&conn).unwrap();
        let first_gid = all_group_ids(&conn);
        assert_eq!(first_gid.len(), 1);

        // Add a third near-duplicate => the {a,b} group is replaced by {a,b,c}
        // with a different derived id; the old group must not linger.
        insert_photo(&conn, "c", Some("0000000000000005")); // near a/b
        regroup(&conn).unwrap();

        let gids = all_group_ids(&conn);
        assert_eq!(gids.len(), 1, "single group after re-cluster");
        assert_ne!(gids[0], first_gid[0], "membership change => new derived id");
        assert_eq!(
            members_of(&conn, &gids[0]),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn derive_id_is_order_independent() {
        let a = derive_id(METHOD, &["x".to_string(), "y".to_string()]);
        let b = derive_id(METHOD, &["y".to_string(), "x".to_string()]);
        assert_eq!(a, b, "id depends on the set, not member order");
        assert_eq!(a.len(), 64, "blake3 hex digest");
    }
}
