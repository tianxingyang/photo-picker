pub mod analysis;
pub mod grouping;
pub mod photos;
pub mod progress;
pub mod projects;

use std::sync::Arc;

use serde_json::json;
use tauri::State;

use crate::error::AppError;
use crate::sidecar::Sidecar;
use crate::AppState;

/// Clone the whole sidecar pool out of state (cheap Arc clones). Errors if the
/// boot task hasn't populated it yet. `analyze_pending` spreads work across all.
pub async fn sidecar_pool(state: &AppState) -> Result<Vec<Arc<Sidecar>>, AppError> {
    let guard = state.sidecars.lock().await;
    if guard.is_empty() {
        return Err(AppError::Sidecar(
            "not started; check that uv and python are on PATH".into(),
        ));
    }
    Ok(guard.clone())
}

/// The first sidecar in the pool, for single-shot ops (echo / transcode).
pub async fn first_sidecar(state: &AppState) -> Result<Arc<Sidecar>, AppError> {
    state.sidecars.lock().await.first().cloned().ok_or_else(|| {
        AppError::Sidecar("not started; check that uv and python are on PATH".into())
    })
}

/// Read the currently-open project id from process state, cloning it out so the
/// `std::sync::Mutex` is never held across an `.await` or into `spawn_blocking`.
/// Returns `Validation("no project open")` when none is open — every
/// photo-scoped command calls this at entry so "ran without an open project" is
/// a loud failure, not silent global access.
pub fn current_project(state: &AppState) -> Result<String, AppError> {
    state
        .current_project
        .lock()
        .expect("current_project mutex poisoned")
        .clone()
        .ok_or_else(|| AppError::Validation("no project open".into()))
}

#[tauri::command]
pub async fn echo_via_sidecar(
    text: String,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    // why: clone the Arc under a brief lock so the outer Mutex is released
    // before the RPC await — concurrent calls can then overlap.
    let sidecar = first_sidecar(&state).await?;

    // echo has no per-file semantics, so flatten the two-level result: both an
    // op error and a transport error become the command's error.
    let result = match sidecar.call("echo", json!({ "text": text })).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Err(AppError::Sidecar(e)),
        Err(e) => return Err(AppError::Sidecar(e.to_string())),
    };

    result
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::Sidecar(format!("malformed response: {result}")))
}
