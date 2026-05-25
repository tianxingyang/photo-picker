pub mod analysis;
pub mod grouping;
pub mod photos;

use serde_json::json;
use tauri::State;

use crate::error::AppError;
use crate::AppState;

#[tauri::command]
pub async fn echo_via_sidecar(
    text: String,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    // why: clone the Arc under a brief lock so the outer Mutex is released
    // before the RPC await — concurrent calls can then overlap.
    let sidecar = {
        let guard = state.sidecar.lock().await;
        guard.as_ref().cloned().ok_or_else(|| {
            AppError::Sidecar("not started; check that uv and python are on PATH".into())
        })?
    };

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
