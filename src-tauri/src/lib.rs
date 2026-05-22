mod commands;
mod db;
mod error;
mod sidecar;

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::Manager;

use crate::sidecar::Sidecar;

pub struct AppState {
    pub sidecar: tokio::sync::Mutex<Option<Sidecar>>,
    pub _db: Mutex<Connection>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let conn = db::open(app.handle()).map_err(boxed)?;
            db::run_migrations(&conn).map_err(boxed)?;

            app.manage(AppState {
                sidecar: tokio::sync::Mutex::new(None),
                _db: Mutex::new(conn),
            });

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let sidecar = match Sidecar::spawn_dev().await {
                    Ok(s) => Some(s),
                    Err(e) => {
                        eprintln!("sidecar spawn failed: {e}");
                        None
                    }
                };
                let state = handle.state::<AppState>();
                *state.sidecar.lock().await = sidecar;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::echo_via_sidecar])
        // why: tauri::Builder::run is the boot path; failure here is unrecoverable
        .run(tauri::generate_context!())
        .expect("tauri builder bootstrap failed");
}

fn boxed(e: Box<dyn std::error::Error + Send + Sync>) -> Box<dyn std::error::Error> {
    e
}
