//! D04: read-only Worker listing — populates the Assign-Worker dropdown.
//! No create-Worker endpoint here; Worker onboarding belongs to messaging/T06.
//!
//! GET /api/v1/branches/:branch_id/workers

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use uuid::Uuid;

use crate::{extractors::AuthorizedBranch, state::AppState};

#[derive(Debug, Serialize)]
pub struct WorkerView {
    pub id: Uuid,
    pub name: String,
}

/// Returns all Workers for the Branch.
/// Filtering to "available" Workers is non-trivial without an assignment read-model;
/// return all and let the Owner pick. Flagged for improvement once an assignments
/// projection exists (README known-risk note).
pub async fn list_workers(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<WorkerView>>, (StatusCode, String)> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, name FROM workers WHERE branch_id = $1 ORDER BY name ASC",
    )
    .bind(branch_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    Ok(Json(rows.into_iter().map(|(id, name)| WorkerView { id, name }).collect()))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
