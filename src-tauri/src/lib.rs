mod commands;
mod db;
mod error;
mod scanner;
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
    // why: Arc<tokio::Mutex> so a command can clone the handle, release the outer
    // ref, then `blocking_lock()` the Connection inside spawn_blocking — keeping
    // rusqlite's blocking work off the tokio worker without holding it across .await.
    pub db: Arc<Mutex<Connection>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let conn = db::open(app.handle()).map_err(boxed)?;
            db::run_migrations(&conn).map_err(boxed)?;

            app.manage(AppState {
                sidecar: Mutex::new(None),
                db: Arc::new(Mutex::new(conn)),
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
        .invoke_handler(tauri::generate_handler![
            commands::echo_via_sidecar,
            commands::photos::scan_folder
        ])
        // why: tauri::Builder::run is the boot path; failure here is unrecoverable
        .run(tauri::generate_context!())
        .expect("tauri builder bootstrap failed");
}

fn boxed(e: Box<dyn std::error::Error + Send + Sync>) -> Box<dyn std::error::Error> {
    e
}
