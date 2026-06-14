mod commands;
mod db;
mod error;
mod grouping;
mod scanner;
mod sidecar;

use std::sync::atomic::AtomicBool;
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
    // why: the id (UUID) of the currently-open project. One project is open at a
    // time; every photo-scoped command reads this to scope its queries. A
    // std::sync::Mutex (not tokio) because the read is a trivial clone done
    // synchronously before any await — see commands::current_project.
    pub current_project: std::sync::Mutex<Option<String>>,
    // why: single-flight guard so two concurrent analyze_pending invocations
    // don't both pick up the same pending rows and double-process them.
    pub analysis_running: AtomicBool,
    // why: single-flight guard so two concurrent group_photos invocations
    // don't both re-cluster and race the delete-then-reinsert transaction.
    pub grouping_running: AtomicBool,
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
                current_project: std::sync::Mutex::new(None),
                analysis_running: AtomicBool::new(false),
                grouping_running: AtomicBool::new(false),
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
            commands::projects::create_project,
            commands::projects::list_projects,
            commands::projects::open_project,
            commands::projects::close_project,
            commands::projects::delete_project,
            commands::photos::scan_folder,
            commands::photos::set_status,
            commands::photos::export_keep,
            commands::photos::transcode_for_display,
            commands::analysis::analyze_pending,
            commands::grouping::group_photos,
            commands::grouping::list_groups
        ])
        // why: tauri::Builder::run is the boot path; failure here is unrecoverable
        .run(tauri::generate_context!())
        .expect("tauri builder bootstrap failed");
}

fn boxed(e: Box<dyn std::error::Error + Send + Sync>) -> Box<dyn std::error::Error> {
    e
}
