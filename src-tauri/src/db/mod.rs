use std::error::Error;
use std::path::PathBuf;

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

type DbErr = Box<dyn Error + Send + Sync>;

const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_initial.sql"),
    include_str!("../../migrations/0002_analysis.sql"),
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

    #[test]
    fn fresh_db_runs_all_migrations_and_builds_analysis_columns() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len() as u32);

        // All analysis columns are addressable and default sanely.
        conn.execute(
            "INSERT INTO photos (id, path, status, created_at) \
             VALUES ('a', '/x.jpg', 'pending', '2026-01-01T00:00:00Z')",
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
    fn v1_db_upgrades_and_existing_rows_become_pending() {
        // Simulate a v1 DB: only 0001 applied, user_version pinned to 1.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1u32).unwrap();
        conn.execute(
            "INSERT INTO photos (id, path, status, created_at) \
             VALUES ('legacy', '/old.jpg', 'keep', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        // Upgrade applies only 0002 (0001 is skipped as version <= current).
        run_migrations(&conn).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len() as u32);
        assert_eq!(
            analysis_state_of(&conn, "legacy"),
            "pending",
            "existing rows must default to pending so they get analyzed"
        );
        // status is untouched by the analysis migration.
        let status: String = conn
            .query_row("SELECT status FROM photos WHERE id = 'legacy'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "keep");
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
