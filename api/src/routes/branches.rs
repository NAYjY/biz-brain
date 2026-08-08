//! D02: Branch management.
//! POST /api/v1/branches — Owner creates a new Branch (in-dashboard, not seed).
//! GET  /api/v1/branches — list owned Branches (for navigation / branch picker).

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{extractors::AuthedOwner, state::AppState};

#[derive(Debug, Serialize)]
pub struct BranchView {
    pub id: Uuid,
    pub name: String,
}

pub async fn list_branches(
    AuthedOwner(claims): AuthedOwner,
    State(state): State<AppState>,
) -> Result<Json<Vec<BranchView>>, (StatusCode, String)> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, name FROM branches WHERE owner_id = $1 ORDER BY created_at ASC",
    )
    .bind(claims.sub)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    Ok(Json(rows.into_iter().map(|(id, name)| BranchView { id, name }).collect()))
}

#[derive(Debug, Deserialize)]
pub struct CreateBranchRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CreateBranchResponse {
    pub id: Uuid,
}

pub async fn create_branch(
    AuthedOwner(claims): AuthedOwner,
    State(state): State<AppState>,
    Json(req): Json<CreateBranchRequest>,
) -> Result<(StatusCode, Json<CreateBranchResponse>), (StatusCode, String)> {
    let id = Uuid::new_v4();

    sqlx::query("INSERT INTO branches (id, owner_id, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(claims.sub)
        .bind(&req.name)
        .execute(&state.pool)
        .await
        .map_err(internal)?;

    Ok((StatusCode::CREATED, Json(CreateBranchResponse { id })))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
