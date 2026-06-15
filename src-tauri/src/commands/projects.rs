use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::{AppHandle, State};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

/// A project row plus its photo count, for the landing-page list. `id` is a
/// UUID v4 generated in `create_project`; `last_opened_at` is NULL until the
/// project is first opened.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub last_opened_at: Option<String>,
    pub photo_count: i64,
}

/// Create a new project and return it (photo_count = 0). The id is a freshly
/// generated UUID v4; `created_at` is now (RFC3339 UTC). Does NOT open the
/// project — the frontend calls `open_project` separately so creation and entry
/// stay distinct steps.
#[tauri::command]
pub async fn create_project(
    name: String,
    state: State<'_, AppState>,
) -> Result<ProjectSummary, AppError> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(AppError::Validation(
            "project name must not be empty".into(),
        ));
    }
    let id = Uuid::new_v4().to_string();
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| AppError::Io(e.to_string()))?;

    let db = state.db.clone();
    let (id, name, now) = (id, name, now);
    let summary =
        tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<ProjectSummary> {
            let conn = db.blocking_lock();
            insert_project(&conn, &id, &name, &now)?;
            Ok(ProjectSummary {
                id,
                name,
                created_at: now,
                last_opened_at: None,
                photo_count: 0,
            })
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?;

    Ok(summary)
}

/// List every project with its photo count, most-recently-opened first (never
/// opened sinks below opened ones), then newest-created first as a tiebreak.
#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectSummary>, AppError> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<Vec<ProjectSummary>> {
        let conn = db.blocking_lock();
        list_projects_db(&conn)
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))
}

/// Open a project: validate it exists, stamp `last_opened_at`, and set it as the
/// process-wide current project so subsequent photo commands scope to it.
#[tauri::command]
pub async fn open_project(project_id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| AppError::Io(e.to_string()))?;

    let db = state.db.clone();
    let id = project_id.clone();
    let touched = tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<usize> {
        let conn = db.blocking_lock();
        touch_last_opened(&conn, &id, &now)
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))?;

    if touched == 0 {
        return Err(AppError::NotFound(format!(
            "no project with id: {project_id}"
        )));
    }

    // Only flip current after the DB confirms the project exists.
    *state
        .current_project
        .lock()
        .expect("current_project mutex poisoned") = Some(project_id);
    Ok(())
}

/// Close the current project (return to the landing page). Idempotent: closing
/// when none is open is a no-op.
#[tauri::command]
pub async fn close_project(state: State<'_, AppState>) -> Result<(), AppError> {
    *state
        .current_project
        .lock()
        .expect("current_project mutex poisoned") = None;
    Ok(())
}

/// Delete a project and all of its photos / groups (cascade via the FKs). If the
/// deleted project happens to be the current one, also clear it so no command
/// keeps operating on a vanished project. External export copies on disk are not
/// touched (PRD R6); the project's WebP thumbnail cache IS purged (the FK cascade
/// only removes DB rows, not the on-disk thumbnail files).
#[tauri::command]
pub async fn delete_project(
    app: AppHandle,
    project_id: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let db = state.db.clone();
    let id = project_id.clone();
    // why: collect the project's photo ids BEFORE the cascade removes the rows,
    // so the on-disk thumbnails can be purged afterward (same blocking closure to
    // keep it one DB lock).
    let (deleted, photo_ids) =
        tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<(usize, Vec<String>)> {
            let conn = db.blocking_lock();
            let ids = select_photo_ids(&conn, &id)?;
            let deleted = delete_project_db(&conn, &id)?;
            Ok((deleted, ids))
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?;

    if deleted == 0 {
        return Err(AppError::NotFound(format!(
            "no project with id: {project_id}"
        )));
    }

    // why: the FK cascade dropped the photo rows but not their thumbnail files —
    // purge them best-effort so app_data_dir doesn't accumulate a deleted
    // project's cache. A remove failure (already gone, never generated) must not
    // fail the already-committed delete.
    if let Ok(thumbs_dir) = crate::db::thumbnails_dir(&app) {
        for id in &photo_ids {
            let _ = std::fs::remove_file(crate::db::thumb_dest(&thumbs_dir, id));
        }
    }

    // why: if we just deleted the open project, drop the dangling current id so
    // photo commands fail the "no project open" guard instead of querying a
    // project whose rows are gone.
    let mut current = state
        .current_project
        .lock()
        .expect("current_project mutex poisoned");
    if current.as_deref() == Some(project_id.as_str()) {
        *current = None;
    }
    Ok(())
}

// --- pure DB helpers (run inside spawn_blocking) ---------------------------

fn insert_project(conn: &Connection, id: &str, name: &str, now: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO projects (id, name, created_at) VALUES (?1, ?2, ?3)",
        params![id, name, now],
    )?;
    Ok(())
}

fn list_projects_db(conn: &Connection) -> rusqlite::Result<Vec<ProjectSummary>> {
    let mut stmt = conn.prepare_cached(
        "SELECT pr.id, pr.name, pr.created_at, pr.last_opened_at, \
         COUNT(ph.id) AS photo_count \
         FROM projects pr \
         LEFT JOIN photos ph ON ph.project_id = pr.id \
         GROUP BY pr.id \
         ORDER BY pr.last_opened_at IS NULL, pr.last_opened_at DESC, pr.created_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ProjectSummary {
            id: r.get(0)?,
            name: r.get(1)?,
            created_at: r.get(2)?,
            last_opened_at: r.get(3)?,
            photo_count: r.get(4)?,
        })
    })?;
    rows.collect()
}

fn touch_last_opened(conn: &Connection, id: &str, now: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE projects SET last_opened_at = ?2 WHERE id = ?1",
        params![id, now],
    )
}

fn delete_project_db(conn: &Connection, id: &str) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM projects WHERE id = ?1", params![id])
}

/// The project's photo ids — read before a cascade delete so the caller can
/// purge each photo's on-disk thumbnail.
fn select_photo_ids(conn: &Connection, project_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("SELECT id FROM photos WHERE project_id = ?1")?;
    let rows = stmt.query_map(params![project_id], |r| r.get::<_, String>(0))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for sql in [
            include_str!("../../migrations/0001_initial.sql"),
            include_str!("../../migrations/0002_analysis.sql"),
            include_str!("../../migrations/0003_grouping.sql"),
            include_str!("../../migrations/0004_projects.sql"),
            include_str!("../../migrations/0005_thumbnails.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        conn
    }

    fn add_photo(conn: &Connection, id: &str, project_id: &str) {
        conn.execute(
            "INSERT INTO photos (id, project_id, path, status, created_at) \
             VALUES (?1, ?2, ?3, 'pending', '2026-01-01T00:00:00Z')",
            params![id, project_id, format!("/{id}.jpg")],
        )
        .unwrap();
    }

    #[test]
    fn create_then_list_shows_zero_photo_count() {
        let conn = mem_conn();
        insert_project(&conn, "p1", "Trip", "2026-01-01T00:00:00Z").unwrap();
        let list = list_projects_db(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "p1");
        assert_eq!(list[0].name, "Trip");
        assert_eq!(list[0].photo_count, 0);
        assert_eq!(list[0].last_opened_at, None);
    }

    #[test]
    fn photo_count_reflects_owned_rows_only() {
        let conn = mem_conn();
        insert_project(&conn, "p1", "A", "2026-01-01T00:00:00Z").unwrap();
        insert_project(&conn, "p2", "B", "2026-01-01T00:00:00Z").unwrap();
        add_photo(&conn, "a", "p1");
        add_photo(&conn, "b", "p1");
        add_photo(&conn, "c", "p2");
        let list = list_projects_db(&conn).unwrap();
        let counts: std::collections::HashMap<_, _> = list
            .iter()
            .map(|p| (p.id.as_str(), p.photo_count))
            .collect();
        assert_eq!(counts["p1"], 2);
        assert_eq!(counts["p2"], 1);
    }

    #[test]
    fn list_orders_opened_before_never_opened() {
        let conn = mem_conn();
        // created oldest→newest: p_old, p_mid, p_new
        insert_project(&conn, "p_old", "old", "2026-01-01T00:00:00Z").unwrap();
        insert_project(&conn, "p_mid", "mid", "2026-01-02T00:00:00Z").unwrap();
        insert_project(&conn, "p_new", "new", "2026-01-03T00:00:00Z").unwrap();
        // open the oldest one most recently → it must sort first.
        touch_last_opened(&conn, "p_old", "2026-06-01T00:00:00Z").unwrap();

        let order: Vec<String> = list_projects_db(&conn)
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        // opened first, then never-opened by created_at desc.
        assert_eq!(order, vec!["p_old", "p_new", "p_mid"]);
    }

    #[test]
    fn touch_last_opened_unknown_id_affects_zero_rows() {
        let conn = mem_conn();
        let n = touch_last_opened(&conn, "nope", "2026-06-01T00:00:00Z").unwrap();
        assert_eq!(n, 0, "command maps 0 rows → NotFound");
    }

    #[test]
    fn delete_unknown_id_affects_zero_rows() {
        let conn = mem_conn();
        let n = delete_project_db(&conn, "nope").unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn select_photo_ids_returns_only_project_rows() {
        let conn = mem_conn();
        insert_project(&conn, "p1", "A", "2026-01-01T00:00:00Z").unwrap();
        insert_project(&conn, "p2", "B", "2026-01-01T00:00:00Z").unwrap();
        add_photo(&conn, "a", "p1");
        add_photo(&conn, "b", "p1");
        add_photo(&conn, "c", "p2");
        let mut ids = select_photo_ids(&conn, "p1").unwrap();
        ids.sort();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn delete_cascades_photos_and_leaves_others() {
        let conn = mem_conn();
        insert_project(&conn, "p1", "A", "2026-01-01T00:00:00Z").unwrap();
        insert_project(&conn, "p2", "B", "2026-01-01T00:00:00Z").unwrap();
        add_photo(&conn, "a", "p1");
        add_photo(&conn, "b", "p2");

        let n = delete_project_db(&conn, "p1").unwrap();
        assert_eq!(n, 1);

        let remaining: i64 = conn
            .query_row("SELECT count(*) FROM photos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 1, "only p2's photo remains");
        let owner: String = conn
            .query_row("SELECT project_id FROM photos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(owner, "p2");
    }
}
