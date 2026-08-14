//! S06: Owner-facing actor binding management.
//! Owner sees pending (unconfirmed) bindings and explicitly approves or
//! rejects each one before messages from that sender are trusted.
//!
//! GET  /api/v1/branches/:branch_id/actors/pending      — list unconfirmed
//! POST /api/v1/branches/:branch_id/actors/:id/confirm  — trust + link to worker/supplier
//! POST /api/v1/branches/:branch_id/actors/:id/reject   — remove this binding

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use domain::BranchId;
use store::PendingBinding;

use crate::{extractors::AuthorizedBranch, state::AppState};

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// List all unconfirmed actor bindings (not branch-scoped — pending rows
/// have no branch until the Owner confirms them).
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

#[derive(Debug, Deserialize)]
pub struct ConfirmBindingRequest {
    /// The WorkerId or SupplierId to link this channel sender to.
    pub worker_id: Uuid,
}

/// Owner confirms a binding, supplying which Worker this sender maps to.
pub async fn confirm_binding(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
    Json(req): Json<ConfirmBindingRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let actor_id = params
        .get("actor_id")
        .ok_or((StatusCode::BAD_REQUEST, "missing actor_id".to_string()))?
        .parse::<Uuid>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "actor_id not a valid UUID".to_string()))?;

    let updated = state
        .actors
        .confirm_binding(actor_id, BranchId::new(branch_id), req.worker_id)
        .await
        .map_err(internal)?;

    if updated {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "binding not found or already confirmed".to_string()))
    }
}

/// Owner rejects / removes a binding.
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
