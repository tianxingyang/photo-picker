//! Pipeline progress events. A single Tauri event drives the frontend's thin
//! top loading bar for all three phases (import / analyze / group). Emitting is
//! best-effort: a failed emit must never abort the real work, so callers ignore
//! the result.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// The event name the frontend listens on (`@tauri-apps/api/event`). The `:`/`/`
/// chars are permitted by Tauri's event-name validator (tauri 2.x
/// `event_name.rs::is_event_name_valid`), so this name emits fine.
pub const EVENT: &str = "pipeline://progress";

/// One progress tick. `total = None` means indeterminate (import-discovering,
/// grouping). `status` flips to `done`/`cancelled`/`error` on the terminal tick
/// so the frontend can clear the bar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub phase: &'static str, // "import" | "analyze" | "group"
    pub done: u32,
    pub total: Option<u32>,
    pub status: &'static str, // "running" | "done" | "cancelled" | "error"
}

pub const PHASE_IMPORT: &str = "import";
pub const PHASE_ANALYZE: &str = "analyze";
pub const PHASE_GROUP: &str = "group";

pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";
pub const STATUS_CANCELLED: &str = "cancelled";
/// Terminal tick for an infra/transport failure (sidecar down / timeout / DB
/// error) — distinct from a user `cancelled` so the UI can tell them apart.
pub const STATUS_ERROR: &str = "error";

/// Emit a progress tick. Best-effort — the `_ =` drop is intentional: a UI event
/// that fails to send must not fail the import/analyze/group operation.
pub fn emit(app: &AppHandle, ev: ProgressEvent) {
    let _ = app.emit(EVENT, ev);
}

/// Convenience for a determinate/indeterminate running tick.
pub fn running(app: &AppHandle, phase: &'static str, done: u32, total: Option<u32>) {
    emit(
        app,
        ProgressEvent {
            phase,
            done,
            total,
            status: STATUS_RUNNING,
        },
    );
}

/// Convenience for a terminal tick. `status` must be one of `STATUS_DONE`,
/// `STATUS_CANCELLED`, or `STATUS_ERROR`.
pub fn terminal(
    app: &AppHandle,
    phase: &'static str,
    done: u32,
    total: Option<u32>,
    status: &'static str,
) {
    emit(
        app,
        ProgressEvent {
            phase,
            done,
            total,
            status,
        },
    );
}
