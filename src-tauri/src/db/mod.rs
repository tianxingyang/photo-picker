use std::error::Error;
use std::path::PathBuf;

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

type DbErr = Box<dyn Error + Send + Sync>;

const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_initial.sql"),
    include_str!("../../migrations/0002_analysis.sql"),
    include_str!("../../migrations/0003_grouping.sql"),
    include_str!("../../migrations/0004_projects.sql"),
];

pub fn open(app_handle: &AppHandle) -> Result<Connection, DbErr> {
    let path = db_path(app_handle)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

pub fn run_migrations(conn: &Connection) -> Result<(), DbErr> {
    let current: u32 = conn.query_row("SELECT user_version FROM pragma_user_version", [], |r| {
        r.get(0)
    })?;
    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let version = (idx + 1) as u32;
        if version <= current {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
        tx.commit()?;
    }
    Ok(())
}

fn db_path(app_handle: &AppHandle) -> Result<PathBuf, DbErr> {
    let dir = app_handle.path().app_data_dir()?;
    Ok(dir.join("photo-picker.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_version(conn: &Connection) -> u32 {
        conn.query_row("SELECT user_version FROM pragma_user_version", [], |r| {
            r.get(0)
        })
        .unwrap()
    }

    /// `analysis_state` must exist after migrations and survive a probe insert.
    fn analysis_state_of(conn: &Connection, id: &str) -> String {
        conn.query_row(
            "SELECT analysis_state FROM photos WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// Seed a project row so FK-bearing photo inserts succeed (0004 makes
    /// `photos.project_id` a NOT NULL FK).
    fn insert_project(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, created_at) \
             VALUES (?1, ?1, '2026-01-01T00:00:00Z')",
            [id],
        )
        .unwrap();
    }

    #[test]
    fn fresh_db_runs_all_migrations_and_builds_analysis_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        run_migrations(&conn).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len() as u32);

        insert_project(&conn, "proj");
        // All analysis columns are addressable and default sanely; project_id is
        // a required FK after 0004.
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES ('a', 'proj', '/x.jpg', 'pending', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        assert_eq!(analysis_state_of(&conn, "a"), "pending");

        // The six analysis columns + analysis_error exist (query would error otherwise).
        conn.query_row(
            "SELECT shot_at, blur_score, is_blurry, exposure_score, \
             exposure_flag, phash, analysis_error FROM photos WHERE id = 'a'",
            [],
            |r| {
                let _: Option<String> = r.get(0)?;
                let _: Option<f64> = r.get(1)?;
                let _: Option<i64> = r.get(2)?;
                let _: Option<f64> = r.get(3)?;
                let _: Option<String> = r.get(4)?;
                let _: Option<String> = r.get(5)?;
                let _: Option<String> = r.get(6)?;
                Ok(())
            },
        )
        .unwrap();
    }

    #[test]
    fn v3_db_upgrades_to_v4_and_discards_preexisting_photos() {
        // Simulate a v3 DB (0001..0003 applied) holding a pre-isolation photo.
        // Per PRD R5, 0004 rebuilds the tables and that data is discarded.
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for sql in &MIGRATIONS[0..3] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 3u32).unwrap();
        conn.execute(
            "INSERT INTO photos (id, path, status, created_at) \
             VALUES ('legacy', '/old.jpg', 'keep', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        // Upgrade applies only 0004 (0001..0003 skipped as version <= current).
        run_migrations(&conn).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len() as u32);
        // The legacy row is gone — 0004 dropped+recreated photos (R5).
        let count: i64 = conn
            .query_row("SELECT count(*) FROM photos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 0,
            "0004 rebuilds photos; pre-isolation data discarded"
        );
        // The new project-scoped schema is in place: project_id is queryable.
        insert_project(&conn, "proj");
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES ('n', 'proj', '/new.jpg', 'pending', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        let pid: String = conn
            .query_row("SELECT project_id FROM photos WHERE id = 'n'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(pid, "proj");
    }

    #[test]
    fn isolation_tables_build_at_version_4() {
        let conn = Connection::open_in_memory().unwrap();
        // why: CASCADE only fires with foreign_keys ON (prod sets it in db::open).
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        run_migrations(&conn).unwrap();

        assert_eq!(
            user_version(&conn),
            4,
            "0004_projects bumps user_version to 4"
        );

        // The project + grouping tables must be queryable (would error if not created).
        let projects: i64 = conn
            .query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        let groups: i64 = conn
            .query_row("SELECT count(*) FROM similar_groups", [], |r| r.get(0))
            .unwrap();
        let members: i64 = conn
            .query_row("SELECT count(*) FROM group_members", [], |r| r.get(0))
            .unwrap();
        assert_eq!(projects, 0);
        assert_eq!(groups, 0);
        assert_eq!(members, 0);

        // ON DELETE CASCADE: deleting a group must remove its member rows.
        insert_project(&conn, "proj");
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES ('p', 'proj', '/p.jpg', 'pending', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO similar_groups (id, project_id, method, params) \
             VALUES ('g', 'proj', 'phash_burst', '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO group_members (group_id, photo_id) VALUES ('g', 'p')",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM similar_groups WHERE id = 'g'", [])
            .unwrap();
        let members: i64 = conn
            .query_row("SELECT count(*) FROM group_members", [], |r| r.get(0))
            .unwrap();
        assert_eq!(members, 0, "ON DELETE CASCADE clears member rows");
    }

    #[test]
    fn delete_project_cascades_photos_and_groups() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        run_migrations(&conn).unwrap();

        insert_project(&conn, "p1");
        insert_project(&conn, "p2");
        // p1 owns a photo that is a member of a p1 group.
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES ('a', 'p1', '/a.jpg', 'pending', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO similar_groups (id, project_id, method, params) \
             VALUES ('g', 'p1', 'phash_burst', '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO group_members (group_id, photo_id) VALUES ('g', 'a')",
            [],
        )
        .unwrap();
        // p2 owns an independent photo that must survive p1's deletion.
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES ('b', 'p2', '/a.jpg', 'pending', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM projects WHERE id = 'p1'", [])
            .unwrap();

        // p1's photo, group, and member rows all cascade away.
        let photos: i64 = conn
            .query_row(
                "SELECT count(*) FROM photos WHERE project_id = 'p1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let groups: i64 = conn
            .query_row(
                "SELECT count(*) FROM similar_groups WHERE project_id = 'p1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let members: i64 = conn
            .query_row("SELECT count(*) FROM group_members", [], |r| r.get(0))
            .unwrap();
        assert_eq!(photos, 0, "p1 photos cascade on project delete");
        assert_eq!(groups, 0, "p1 groups cascade on project delete");
        assert_eq!(members, 0, "member rows cascade off both photo and group");
        // p2 is untouched — same path, independent record.
        let p2: i64 = conn
            .query_row(
                "SELECT count(*) FROM photos WHERE project_id = 'p2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(p2, 1, "other projects are unaffected by a delete");
    }

    #[test]
    fn migrations_are_idempotent_when_rerun() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        // A second run is a no-op (every version <= current).
        run_migrations(&conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len() as u32);
    }
}
