mod commands;
mod db;
mod error;
mod sidecar;

use std::sync::Arc;

use rusqlite::Connection;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::sidecar::Sidecar;

pub struct AppState {
    // why: Arc lets command handlers clone-and-release the outer lock instantly
    // so concurrent RPCs aren't serialized behind a single in-flight call.
    pub sidecar: Mutex<Option<Arc<Sidecar>>>,
    // why: async Mutex so future #[tauri::command] async fns don't block the
    // tokio worker on a sync lock. DB calls themselves should be wrapped in
    // tauri::async_runtime::spawn_blocking when they land.
    pub _db: Mutex<Connection>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let conn = db::open(app.handle()).map_err(boxed)?;
            db::run_migrations(&conn).map_err(boxed)?;

            app.manage(AppState {
                sidecar: Mutex::new(None),
                _db: Mutex::new(conn),
            });

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let sidecar = match Sidecar::spawn_dev().await {
                    Ok(s) => Some(Arc::new(s)),
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
