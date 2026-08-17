//! S06: Owner-facing actor binding management.
//! GET  /api/v1/branches/:branch_id/actors/pending
//! POST /api/v1/branches/:branch_id/actors/:actor_id/confirm
//! POST /api/v1/branches/:branch_id/actors/:actor_id/reject

use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::Deserialize;
use uuid::Uuid;

use domain::BranchId;
use store::PendingBinding;
use crate::{extractors::AuthorizedBranch, state::AppState};

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub async fn list_pending_bindings(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<PendingBinding>>, (StatusCode, String)> {
    let rows = state.actors.list_pending(BranchId::new(branch_id)).await.map_err(internal)?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct ConfirmBindingRequest {
    pub worker_id: Uuid,
}

pub async fn confirm_binding(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
    Json(req): Json<ConfirmBindingRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let actor_id = params.get("actor_id")
        .ok_or((StatusCode::BAD_REQUEST, "missing actor_id".to_string()))?
        .parse::<Uuid>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid actor_id".to_string()))?;

    let check: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM workers WHERE id = $1 AND branch_id = $2",
    )
    .bind(req.worker_id).bind(branch_id)
    .fetch_optional(&state.pool).await.map_err(internal)?;

    if check.is_none() {
        return Err((StatusCode::BAD_REQUEST, "worker not found in this branch".to_string()));
    }

    let updated = state.actors
        .confirm_binding(actor_id, BranchId::new(branch_id), req.worker_id)
        .await.map_err(internal)?;

    if updated { Ok(StatusCode::NO_CONTENT) }
    else { Err((StatusCode::NOT_FOUND, "binding not found or already confirmed".to_string())) }
}

pub async fn reject_binding(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let actor_id = params.get("actor_id")
        .ok_or((StatusCode::BAD_REQUEST, "missing actor_id".to_string()))?
        .parse::<Uuid>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid actor_id".to_string()))?;

    let deleted = state.actors
        .reject_binding(actor_id, BranchId::new(branch_id))
        .await.map_err(internal)?;

    if deleted { Ok(StatusCode::NO_CONTENT) }
    else { Err((StatusCode::NOT_FOUND, "binding not found".to_string())) }
}
