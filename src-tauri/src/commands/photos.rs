use std::path::PathBuf;

use tauri::State;

use crate::error::AppError;
use crate::scanner::{self, ScanOutcome};
use crate::AppState;

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
