use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::json;
use tauri::State;

use crate::commands::current_project;
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

    // Re-cluster only the open project's photos.
    let project_id = current_project(&state)?;

    let db = state.db.clone();
    let summary =
        tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<GroupSummary> {
            let conn = db.blocking_lock();
            regroup(&conn, &project_id)
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?;

    Ok(summary)
}

/// Load `(id, phash)` for analysed photos, cluster, and rewrite the
/// `phash_burst` groups in one transaction. Pure DB work — runs inside
/// `spawn_blocking`, never holds the lock across `.await`.
fn regroup(conn: &Connection, project_id: &str) -> rusqlite::Result<GroupSummary> {
    // why: collect owned (id, phash_u64) inside the closure — never leak Row
    // borrows; skip rows whose phash fails to parse rather than crash.
    let raw = {
        let mut stmt = conn.prepare_cached(
            "SELECT id, phash FROM photos WHERE phash IS NOT NULL AND project_id = ?1",
        )?;
        let rows = stmt
            .query_map(params![project_id], |r| {
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
        // CASCADE drops member rows for the old phash_burst groups too. Scoped to
        // this project so re-grouping one project never touches another's groups.
        tx.execute(
            "DELETE FROM similar_groups WHERE method = ?1 AND project_id = ?2",
            params![METHOD, project_id],
        )?;

        let mut insert_group = tx.prepare_cached(
            "INSERT INTO similar_groups (id, project_id, method, params) VALUES (?1, ?2, ?3, ?4)",
        )?;
        let mut insert_member =
            tx.prepare_cached("INSERT INTO group_members (group_id, photo_id) VALUES (?1, ?2)")?;

        let mut grouped_photos = 0u32;
        for comp in &comps {
            let gid = derive_id(METHOD, comp);
            insert_group.execute(params![gid, project_id, METHOD, params_json])?;
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

// ---------------------------------------------------------------------------
// Browse model: read groups + their member photos (with analysis fields and
// status) plus the "ungrouped" bucket, for the group-browse UI. Read-only, so
// no single-flight guard — just the standard spawn_blocking + blocking_lock.
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePhoto {
    pub id: String,
    pub path: String,
    pub status: String,
    pub shot_at: Option<String>,
    pub is_blurry: Option<bool>,
    pub blur_score: Option<f64>,
    pub exposure_flag: Option<String>,
    pub analysis_state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseGroup {
    pub id: String,
    pub photos: Vec<BrowsePhoto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseModel {
    pub groups: Vec<BrowseGroup>,
    pub ungrouped: Vec<BrowsePhoto>,
}

/// Read the full browse model: every `phash_burst` group with its members
/// (sorted by capture time, sharpest first as tiebreak) plus the ungrouped
/// bucket (singletons + not-yet-analysed + failed — anything not in a group).
#[tauri::command]
pub async fn list_groups(state: State<'_, AppState>) -> Result<BrowseModel, AppError> {
    // Browse only the open project's photos and groups.
    let project_id = current_project(&state)?;
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<BrowseModel> {
        let conn = db.blocking_lock();
        load_browse_model(&conn, &project_id)
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))
}

/// Read 8 photo columns into a `BrowsePhoto`, starting at column `base`. The
/// SELECT column order below is the contract: id, path, status, shot_at,
/// is_blurry, blur_score, exposure_flag, analysis_state.
fn read_photo(r: &rusqlite::Row, base: usize) -> rusqlite::Result<BrowsePhoto> {
    let is_blurry: Option<i64> = r.get(base + 4)?;
    Ok(BrowsePhoto {
        id: r.get(base)?,
        path: r.get(base + 1)?,
        status: r.get(base + 2)?,
        shot_at: r.get(base + 3)?,
        is_blurry: is_blurry.map(|v| v != 0),
        blur_score: r.get(base + 5)?,
        exposure_flag: r.get(base + 6)?,
        analysis_state: r.get(base + 7)?,
    })
}

/// Ascending compare with NULLs last. ISO8601 strings sort lexically.
/// Fully-qualified `std::cmp::Ordering` — the module-level `Ordering` import is
/// `atomic::Ordering`, a different type.
fn cmp_opt_asc(a: Option<&str>, b: Option<&str>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// Descending compare (higher first) with NULLs last, for blur_score (f64).
fn cmp_opt_f64_desc(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// Within-group order: capture time ascending, then sharpest first, then id.
fn cmp_in_group(a: &BrowsePhoto, b: &BrowsePhoto) -> std::cmp::Ordering {
    cmp_opt_asc(a.shot_at.as_deref(), b.shot_at.as_deref())
        .then_with(|| cmp_opt_f64_desc(a.blur_score, b.blur_score))
        .then_with(|| a.id.cmp(&b.id))
}

/// Group order: by earliest member capture time (members already sorted, so
/// the first member is earliest), NULLs last, then group id.
fn cmp_group(a: &BrowseGroup, b: &BrowseGroup) -> std::cmp::Ordering {
    let ax = a.photos.first().and_then(|p| p.shot_at.as_deref());
    let bx = b.photos.first().and_then(|p| p.shot_at.as_deref());
    cmp_opt_asc(ax, bx).then_with(|| a.id.cmp(&b.id))
}

fn load_browse_model(conn: &Connection, project_id: &str) -> rusqlite::Result<BrowseModel> {
    // Grouped members. Build a map keyed by group id, then sort within and across.
    let mut by_group: std::collections::HashMap<String, Vec<BrowsePhoto>> =
        std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT gm.group_id, p.id, p.path, p.status, p.shot_at, p.is_blurry, \
             p.blur_score, p.exposure_flag, p.analysis_state \
             FROM group_members gm \
             JOIN photos p ON p.id = gm.photo_id \
             JOIN similar_groups sg ON sg.id = gm.group_id \
             WHERE sg.method = ?1 AND sg.project_id = ?2",
        )?;
        let rows = stmt.query_map(params![METHOD, project_id], |r| {
            let gid: String = r.get(0)?;
            Ok((gid, read_photo(r, 1)?))
        })?;
        for row in rows {
            let (gid, photo) = row?;
            by_group.entry(gid).or_default().push(photo);
        }
    }
    let mut groups: Vec<BrowseGroup> = by_group
        .into_iter()
        .map(|(id, mut photos)| {
            photos.sort_by(cmp_in_group);
            BrowseGroup { id, photos }
        })
        .collect();
    groups.sort_by(cmp_group);

    // Ungrouped: photos not a member of any `phash_burst` group — singletons
    // (cluster never emits size-1 components), not-yet-analysed and failed
    // photos. Method-scoped on purpose: a photo grouped *only* under a future
    // method (M3 CLIP/faces, same two tables) is excluded from the grouped
    // bucket by `sg.method=?1` above, so it must surface here rather than
    // vanish from the browse model entirely. At M1 (single method) this is
    // equivalent to "no group_members row".
    let ungrouped = {
        let mut stmt = conn.prepare_cached(
            "SELECT p.id, p.path, p.status, p.shot_at, p.is_blurry, p.blur_score, \
             p.exposure_flag, p.analysis_state \
             FROM photos p \
             WHERE p.project_id = ?2 AND NOT EXISTS ( \
                 SELECT 1 FROM group_members gm \
                 JOIN similar_groups sg ON sg.id = gm.group_id \
                 WHERE gm.photo_id = p.id AND sg.method = ?1 \
             )",
        )?;
        let mut v = stmt
            .query_map(params![METHOD, project_id], |r| read_photo(r, 0))?
            .collect::<rusqlite::Result<Vec<BrowsePhoto>>>()?;
        v.sort_by(cmp_in_group);
        v
    };

    Ok(BrowseModel { groups, ungrouped })
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

    /// Insert a photo with a given (or NULL) phash already analysed.
    fn insert_photo(conn: &Connection, id: &str, phash: Option<&str>) {
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at, analysis_state, phash) \
             VALUES (?1, ?2, ?3, 'pending', '2026-01-01T00:00:00Z', 'done', ?4)",
            params![id, PROJ, format!("/{id}.jpg"), phash],
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

        let summary = regroup(&conn, PROJ).unwrap();
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

        let summary = regroup(&conn, PROJ).unwrap();
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

        let summary = regroup(&conn, PROJ).unwrap();
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

        regroup(&conn, PROJ).unwrap();
        let gids_first = all_group_ids(&conn);
        let members_first: Vec<Vec<String>> =
            gids_first.iter().map(|g| members_of(&conn, g)).collect();

        // Second run: same input => same ids, same members, no stale rows.
        let summary = regroup(&conn, PROJ).unwrap();
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

        regroup(&conn, PROJ).unwrap();
        let first_gid = all_group_ids(&conn);
        assert_eq!(first_gid.len(), 1);

        // Add a third near-duplicate => the {a,b} group is replaced by {a,b,c}
        // with a different derived id; the old group must not linger.
        insert_photo(&conn, "c", Some("0000000000000005")); // near a/b
        regroup(&conn, PROJ).unwrap();

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

    // --- browse model -----------------------------------------------------

    /// Insert a photo with explicit browse-relevant columns.
    fn ins(
        conn: &Connection,
        id: &str,
        status: &str,
        shot_at: Option<&str>,
        blur: Option<f64>,
        analysis_state: &str,
    ) {
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at, shot_at, blur_score, analysis_state) \
             VALUES (?1, ?2, ?3, ?4, '2026-01-01T00:00:00Z', ?5, ?6, ?7)",
            params![id, PROJ, format!("/{id}.jpg"), status, shot_at, blur, analysis_state],
        )
        .unwrap();
    }

    fn add_group(conn: &Connection, gid: &str, members: &[&str]) {
        conn.execute(
            "INSERT INTO similar_groups (id, project_id, method, params) VALUES (?1, ?2, ?3, '{}')",
            params![gid, PROJ, METHOD],
        )
        .unwrap();
        for m in members {
            conn.execute(
                "INSERT INTO group_members (group_id, photo_id) VALUES (?1, ?2)",
                params![gid, m],
            )
            .unwrap();
        }
    }

    fn ids(photos: &[BrowsePhoto]) -> Vec<&str> {
        photos.iter().map(|p| p.id.as_str()).collect()
    }

    #[test]
    fn within_group_sorted_by_shot_at_then_sharpness() {
        let conn = mem_conn();
        // Out-of-insertion-order capture times; "b" and "c" share a time so the
        // sharper (higher blur_score) one wins the tiebreak.
        ins(
            &conn,
            "a",
            "pending",
            Some("2026-05-01T10:00:00"),
            Some(50.0),
            "done",
        );
        ins(
            &conn,
            "b",
            "pending",
            Some("2026-05-01T09:00:00"),
            Some(10.0),
            "done",
        );
        ins(
            &conn,
            "c",
            "pending",
            Some("2026-05-01T09:00:00"),
            Some(99.0),
            "done",
        );
        add_group(&conn, "g1", &["a", "b", "c"]);

        let model = load_browse_model(&conn, PROJ).unwrap();
        assert_eq!(model.groups.len(), 1);
        // 09:00 sharper(c) -> 09:00 blurrier(b) -> 10:00(a)
        assert_eq!(ids(&model.groups[0].photos), vec!["c", "b", "a"]);
        assert!(model.ungrouped.is_empty());
    }

    #[test]
    fn ungrouped_holds_singletons_unanalysed_and_failed() {
        let conn = mem_conn();
        ins(
            &conn,
            "g_a",
            "pending",
            Some("2026-05-01T08:00:00"),
            Some(20.0),
            "done",
        );
        ins(
            &conn,
            "g_b",
            "pending",
            Some("2026-05-01T08:30:00"),
            Some(30.0),
            "done",
        );
        add_group(&conn, "g1", &["g_a", "g_b"]);
        // not in any group:
        ins(
            &conn,
            "single",
            "keep",
            Some("2026-05-01T07:00:00"),
            Some(40.0),
            "done",
        );
        ins(&conn, "pending_one", "pending", None, None, "pending");
        ins(&conn, "failed_one", "reject", None, None, "failed");

        let model = load_browse_model(&conn, PROJ).unwrap();
        let ung = ids(&model.ungrouped);
        // shot_at asc, NULLs last; single(07:00) first, then the two NULLs by id.
        assert_eq!(ung, vec!["single", "failed_one", "pending_one"]);
        // grouped photos never leak into ungrouped.
        assert!(!ung.contains(&"g_a") && !ung.contains(&"g_b"));
    }

    #[test]
    fn groups_ordered_by_earliest_capture_time() {
        let conn = mem_conn();
        ins(
            &conn,
            "late1",
            "pending",
            Some("2026-05-02T10:00:00"),
            Some(10.0),
            "done",
        );
        ins(
            &conn,
            "late2",
            "pending",
            Some("2026-05-02T11:00:00"),
            Some(10.0),
            "done",
        );
        add_group(&conn, "g_late", &["late1", "late2"]);
        ins(
            &conn,
            "early1",
            "pending",
            Some("2026-05-01T10:00:00"),
            Some(10.0),
            "done",
        );
        ins(
            &conn,
            "early2",
            "pending",
            Some("2026-05-01T11:00:00"),
            Some(10.0),
            "done",
        );
        add_group(&conn, "g_early", &["early1", "early2"]);

        let model = load_browse_model(&conn, PROJ).unwrap();
        let order: Vec<&str> = model.groups.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(order, vec!["g_early", "g_late"]);
    }

    #[test]
    fn empty_db_yields_empty_model() {
        let conn = mem_conn();
        let model = load_browse_model(&conn, PROJ).unwrap();
        assert!(model.groups.is_empty());
        assert!(model.ungrouped.is_empty());
    }

    #[test]
    fn is_blurry_int_maps_to_bool() {
        let conn = mem_conn();
        ins(
            &conn,
            "x",
            "pending",
            Some("2026-05-01T10:00:00"),
            Some(5.0),
            "done",
        );
        conn.execute("UPDATE photos SET is_blurry = 1 WHERE id = 'x'", [])
            .unwrap();
        let model = load_browse_model(&conn, PROJ).unwrap();
        assert_eq!(model.ungrouped[0].is_blurry, Some(true));
    }

    #[test]
    fn photo_grouped_only_under_other_method_surfaces_in_ungrouped() {
        // Regression: the ungrouped query is scoped to `phash_burst`. A photo
        // whose only membership is in a non-phash_burst group (e.g. a future
        // M3 'clip' group) is excluded from the grouped bucket by the method
        // filter, so it must still appear in ungrouped — never disappear.
        let conn = mem_conn();
        ins(
            &conn,
            "x",
            "pending",
            Some("2026-05-01T10:00:00"),
            Some(5.0),
            "done",
        );
        conn.execute(
            "INSERT INTO similar_groups (id, project_id, method, params) VALUES ('gc', 'test-project', 'clip', '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO group_members (group_id, photo_id) VALUES ('gc', 'x')",
            [],
        )
        .unwrap();

        let model = load_browse_model(&conn, PROJ).unwrap();
        assert!(model.groups.is_empty(), "no phash_burst groups exist");
        assert_eq!(
            ids(&model.ungrouped),
            vec!["x"],
            "other-method member must surface in ungrouped, not vanish"
        );
    }
}
