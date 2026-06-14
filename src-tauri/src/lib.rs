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

/// Upper bound on sidecar processes. Each is a single-threaded synchronous
/// Python (`uv run python main.py`) that loads numpy/PIL/pillow_heif (~50-100MB
/// each), so we cap the pool to bound memory while still giving real multicore
/// analysis. Actual size = min(cpu-1, this).
const MAX_SIDECARS: usize = 4;

/// Number of sidecar processes to spawn: `cpu-1` clamped to `[1, MAX_SIDECARS]`.
pub fn sidecar_pool_size() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1).clamp(1, MAX_SIDECARS))
        .unwrap_or(1)
}

pub struct AppState {
    // why: a POOL of identical sidecar processes. Each Python sidecar is
    // single-threaded + synchronous — the only model stable under piped stdio on
    // Windows (in-process threads deadlock/EINVAL there). Parallelism comes from
    // running several and spreading `analyze` across them (see analyze_pending).
    // transcode/echo use the first. Empty until the boot task fills it.
    pub sidecars: Mutex<Vec<Arc<Sidecar>>>,
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
    // why: cooperative cancel for an in-progress analyze batch. Set by
    // cancel_analysis (or on an infra error); the bounded-concurrency dispatch
    // checks it before starting each photo and stops feeding new work — in-flight
    // analyses finish, remaining rows stay 'pending'. Reset at each batch start.
    pub analysis_cancel: AtomicBool,
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
                sidecars: Mutex::new(Vec::new()),
                db: Arc::new(Mutex::new(conn)),
                current_project: std::sync::Mutex::new(None),
                analysis_running: AtomicBool::new(false),
                analysis_cancel: AtomicBool::new(false),
                grouping_running: AtomicBool::new(false),
            });

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Spawn the sidecar pool concurrently (each `uv run` startup is
                // slow); keep whichever processes came up.
                let n = sidecar_pool_size();
                let spawned =
                    futures::future::join_all((0..n).map(|_| Sidecar::spawn_dev())).await;
                let pool: Vec<Arc<Sidecar>> = spawned
                    .into_iter()
                    .filter_map(|r| match r {
                        Ok(s) => Some(Arc::new(s)),
                        Err(e) => {
                            eprintln!("sidecar spawn failed: {e}");
                            None
                        }
                    })
                    .collect();
                eprintln!("sidecar pool ready: {} process(es)", pool.len());
                let state = handle.state::<AppState>();
                *state.sidecars.lock().await = pool;
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
            commands::analysis::cancel_analysis,
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
