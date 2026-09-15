//! Worker communication commands.
//! POST /branches/:branch_id/orders/:order_id/resolve-clarification
//! POST /branches/:branch_id/orders/:order_id/message-worker

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use domain::{BranchId, DomainEvent, OrderId, WorkerId};

use crate::{extractors::AuthorizedBranch, state::AppState};

use super::{append_and_project_order, bad_request, internal, send_message_to_worker};

// ── Resolve clarification ─────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct ResolveClarificationRequest {
    pub message: String,
}

/// POST /branches/:branch_id/orders/:order_id/resolve-clarification
pub async fn resolve_clarification(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<ResolveClarificationRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id)
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let worker_id = row
        .and_then(|(id,)| id)
        .ok_or_else(|| bad_request("no worker assigned to this order"))?;

    send_message_to_worker(&state, worker_id, &req.message)
        .await
        .map_err(internal)?;

    let event = DomainEvent::ClarificationResolved {
        worker_id: WorkerId::new(worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await?;

    // F04: Owner resolved clarification — reset unread count.
    let _ = state.projections.clear_unread(OrderId::new(order_id)).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── Message worker ────────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct MessageWorkerRequest {
    pub text: String,
}

/// POST /branches/:branch_id/orders/:order_id/message-worker
pub async fn message_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<MessageWorkerRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return Err(bad_request("message text required"));
    }

    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id)
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let worker_id = row
        .and_then(|(id,)| id)
        .ok_or_else(|| bad_request("no worker assigned to this order"))?;

    send_message_to_worker(&state, worker_id, &text)
        .await
        .map_err(internal)?;

    // F04: Owner sent a direct message — reset unread count and low-confidence flag.
    let _ = state.projections.clear_unread(OrderId::new(order_id)).await;

    Ok(StatusCode::NO_CONTENT)
}