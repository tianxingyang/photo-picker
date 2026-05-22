use serde_json::json;
use tauri::State;

use crate::error::AppError;
use crate::AppState;

#[tauri::command]
pub async fn echo_via_sidecar(
    text: String,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    let guard = state.sidecar.lock().await;
    let sidecar = guard.as_ref().ok_or_else(|| {
        AppError::Sidecar("not started; check that uv and python are on PATH".into())
    })?;

    let result = sidecar
        .call("echo", json!({ "text": text }))
        .await
        .map_err(|e| AppError::Sidecar(e.to_string()))?;

    result
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::Sidecar(format!("malformed response: {result}")))
}
