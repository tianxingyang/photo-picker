use std::error::Error;
use std::path::PathBuf;

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

type DbErr = Box<dyn Error + Send + Sync>;

const MIGRATIONS: &[&str] = &[include_str!("../../migrations/0001_initial.sql")];

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
