//! S06: Owner-facing actor binding management.
//! Owner sees pending (unconfirmed) bindings and explicitly approves or
//! rejects each one before messages from that sender are trusted.
//!
//! GET  /api/v1/branches/:branch_id/actors/pending      — list unconfirmed
//! POST /api/v1/branches/:branch_id/actors/:id/confirm  — trust this binding
//! POST /api/v1/branches/:branch_id/actors/:id/reject   — remove this binding

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use domain::BranchId;
use store::PendingBinding;

use crate::{extractors::AuthorizedBranch, state::AppState};

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// List all unconfirmed actor bindings for this Branch.
/// Owner reviews these and calls /confirm or /reject on each.
pub async fn list_pending_bindings(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<PendingBinding>>, (StatusCode, String)> {
    let rows = state
        .actors
        .list_pending(BranchId::new(branch_id))
        .await
        .map_err(internal)?;
    Ok(Json(rows))
}

/// Owner confirms a binding — sender becomes trusted from this point on.
pub async fn confirm_binding(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let actor_id = params
        .get("actor_id")
        .ok_or((StatusCode::BAD_REQUEST, "missing actor_id".to_string()))?
        .parse::<Uuid>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "actor_id not a valid UUID".to_string()))?;

    let updated = state
        .actors
        .confirm_binding(actor_id, BranchId::new(branch_id))
        .await
        .map_err(internal)?;

    if updated {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "binding not found or already confirmed".to_string()))
    }
}

/// Owner rejects / removes a binding.
/// Also the path for rebinding: reject the old entry, then the next inbound
/// message from that sender creates a new pending row.
pub async fn reject_binding(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let actor_id = params
        .get("actor_id")
        .ok_or((StatusCode::BAD_REQUEST, "missing actor_id".to_string()))?
        .parse::<Uuid>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "actor_id not a valid UUID".to_string()))?;

    let deleted = state
        .actors
        .reject_binding(actor_id, BranchId::new(branch_id))
        .await
        .map_err(internal)?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "binding not found".to_string()))
    }
}