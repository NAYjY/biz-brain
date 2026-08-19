//! T05 / P04: one command endpoint per `DomainEvent` variant the Owner can
//! trigger directly.
//!
//! KEY DESIGN: every route that lives under
//!   /branches/:branch_id/orders/:order_id/...
//! uses a *typed* `Path<(Uuid, Uuid)>` to extract both ids at once.
//! `AuthorizedBranch` internally extracts `Path<HashMap<String,String>>`
//! for the branch ownership check — combining it with a *second* Path
//! extractor of a *different* type is fine because Axum de-duplicates by
//! type, but we must not use `Path<HashMap>` twice.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use domain::{BranchId, Channel, ChannelIdentity, DomainEvent, InvoiceId, OrderId, WorkerId};
use messaging::ChannelAdapter;

use crate::{event_handler, extractors::AuthorizedBranch, state::AppState};

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
fn bad_request(msg: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, msg.into())
}

// ── Assign Worker ─────────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct AssignWorkerRequest {
    pub worker_id: Uuid,
}

/// POST /branches/:branch_id/orders/:order_id/assign-worker
pub async fn assign_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<AssignWorkerRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let binding: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM actor_directory \
         WHERE actor_id = $1 AND actor_type = 'worker' AND owner_confirmed = TRUE",
    )
    .bind(req.worker_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    if binding.is_none() {
        return Err(bad_request(
            "Worker has no confirmed channel binding. \
             Confirm it on the Workers & Suppliers page first.",
        ));
    }

    let event = DomainEvent::WorkerAssigned {
        worker_id: WorkerId::new(req.worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── Close Order ───────────────────────────────────────────────────────────── //

/// POST /branches/:branch_id/orders/:order_id/close
pub async fn close_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let event = DomainEvent::OrderDone { order_id: OrderId::new(order_id) };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P04: Cancel Order ─────────────────────────────────────────────────────── //

/// POST /branches/:branch_id/orders/:order_id/cancel
pub async fn cancel_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let event = DomainEvent::OwnerCancelled { order_id: OrderId::new(order_id) };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P04: Reset Order ──────────────────────────────────────────────────────── //

/// POST /branches/:branch_id/orders/:order_id/reset
pub async fn reset_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let event = DomainEvent::OrderReset { order_id: OrderId::new(order_id) };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P16: Force-set state ──────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct ForceStateRequest {
    /// Optional note recorded in the audit trail.
    pub note: Option<String>,
}

/// POST /branches/:branch_id/orders/:order_id/force-accepted
/// Owner forces the order to Accepted without waiting for Worker to reply.
pub async fn force_accepted(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(_req): Json<ForceStateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id).bind(branch_id)
    .fetch_optional(&state.pool).await.map_err(internal)?;

    let worker_id = row
        .and_then(|(id,)| id)
        .ok_or_else(|| bad_request("no worker assigned to this order"))?;

    let event = DomainEvent::OwnerForceAccepted {
        worker_id: WorkerId::new(worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

/// POST /branches/:branch_id/orders/:order_id/force-unavailable
/// Owner marks Worker as unavailable for this order.
pub async fn force_unavailable(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(_req): Json<ForceStateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Use the currently-assigned worker_id from the projection.
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id).bind(branch_id)
    .fetch_optional(&state.pool).await.map_err(internal)?;

    let worker_id = row
        .and_then(|(id,)| id)
        .ok_or_else(|| bad_request("no worker assigned — assign a worker first"))?;

    let event = DomainEvent::OwnerForceUnavailable {
        worker_id: WorkerId::new(worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

/// POST /branches/:branch_id/orders/:order_id/force-clarification
/// Owner manually opens a PendingClarification state.
pub async fn force_clarification(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(_req): Json<ForceStateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id).bind(branch_id)
    .fetch_optional(&state.pool).await.map_err(internal)?;

    let worker_id = row
        .and_then(|(id,)| id)
        .ok_or_else(|| bad_request("no worker assigned to this order"))?;

    let event = DomainEvent::OwnerForceClarification  {
        worker_id: WorkerId::new(worker_id),
        order_id: OrderId::new(order_id),
    };

    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

/// POST /branches/:branch_id/orders/:order_id/force-ready
/// Owner marks the order as ready for pickup.
pub async fn force_ready(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(_req): Json<ForceStateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id).bind(branch_id)
    .fetch_optional(&state.pool).await.map_err(internal)?;

    let worker_id = row
        .and_then(|(id,)| id)
        .ok_or_else(|| bad_request("no worker assigned to this order"))?;

    let event = DomainEvent::OwnerForceReady {
        worker_id: WorkerId::new(worker_id),
        order_id: OrderId::new(order_id),
    };

    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P16: Reassign worker ──────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct ReassignWorkerRequest {
    pub worker_id: Uuid,
}

/// POST /branches/:branch_id/orders/:order_id/reassign-worker
/// Swap the assigned Worker without cancel+reset.
pub async fn reassign_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<ReassignWorkerRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let binding: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM actor_directory \
         WHERE actor_id = $1 AND actor_type = 'worker' AND owner_confirmed = TRUE",
    )
    .bind(req.worker_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    if binding.is_none() {
        return Err(bad_request(
            "Worker has no confirmed channel binding. Confirm it on the Workers & Suppliers page first.",
        ));
    }

    let event = DomainEvent::OwnerReassignWorker {
        new_worker_id: WorkerId::new(req.worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P16: Edit description ─────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct EditDescriptionRequest {
    pub description: String,
}

/// PATCH /branches/:branch_id/orders/:order_id/description
/// Edit order description — logged to order_description_edits; not an event.
pub async fn edit_description(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<EditDescriptionRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let desc = req.description.trim().to_string();
    if desc.is_empty() {
        return Err(bad_request("description cannot be empty"));
    }
    if desc.len() > 1000 {
        return Err(bad_request("description max 1000 characters"));
    }

    // Verify order belongs to this branch.
    let exists: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM orders WHERE id = $1 AND branch_id = $2 AND deleted_at IS NULL",
    )
    .bind(order_id).bind(branch_id)
    .fetch_optional(&state.pool).await.map_err(internal)?;

    if exists.is_none() {
        return Err((StatusCode::NOT_FOUND, "order not found".to_string()));
    }

    sqlx::query(
        "INSERT INTO order_description_edits (order_id, new_description) VALUES ($1, $2)",
    )
    .bind(order_id).bind(&desc)
    .execute(&state.pool).await.map_err(internal)?;

    // Update projection immediately (no event stream — description is metadata).
    sqlx::query(
        "UPDATE order_current_state SET description = $1, updated_at = NOW() \
         WHERE order_id = $2",
    )
    .bind(&desc).bind(order_id)
    .execute(&state.pool).await.map_err(internal)?;

    // Publish SSE so the dashboard refreshes.
    let meta: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT branch_id FROM orders WHERE id = $1",
    )
    .bind(order_id).fetch_optional(&state.pool).await.map_err(internal)?;

    if let Some((bid,)) = meta {
        state.publish_sse(domain::SseSignal::OrderChanged {
            order_id: domain::OrderId::new(order_id),
            branch_id: domain::BranchId::new(bid),
        }).await;
    }

    // Notify worker if one is assigned
    if let Ok(Some((worker_id,))) = sqlx::query_as::<_, (Option<Uuid>,)>(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1",
    )
    .bind(order_id)
    .fetch_optional(&state.pool)
    .await
    {
        if let Some(wid) = worker_id {
            let _ = send_message_to_worker(&state, wid, 
                &format!("ℹ️ Order description updated: {desc}")).await;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── P16: Delete order (soft) ──────────────────────────────────────────────── //

/// DELETE /branches/:branch_id/orders/:order_id
/// Soft-deletes an order. Blocked if the order has in-flight worker activity.
pub async fn delete_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Block deletion if order is actively assigned or accepted.
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT state FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id).bind(branch_id)
    .fetch_optional(&state.pool).await.map_err(internal)?;

    if let Some((state_str,)) = row {
        let blocked = matches!(
            state_str.as_str(),
            "ASSIGNED" | "ACCEPTED" | "PENDING_CLARIFICATION" | "READY_FOR_PICKUP"
        );
        if blocked {
            return Err((StatusCode::CONFLICT,
                format!("Cannot delete an order in state '{state_str}'. Cancel or close it first.")));
        }
    }

    let result = sqlx::query(
        "UPDATE orders SET deleted_at = NOW() WHERE id = $1 AND branch_id = $2 AND deleted_at IS NULL",
    )
    .bind(order_id).bind(branch_id)
    .execute(&state.pool).await.map_err(internal)?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "order not found".to_string()));
    }

    // Remove from projection so it disappears from the dashboard.
    sqlx::query("DELETE FROM order_current_state WHERE order_id = $1")
        .bind(order_id).execute(&state.pool).await.map_err(internal)?;

    // Publish SSE so the list refreshes.
    let meta: Option<(uuid::Uuid,)> = sqlx::query_as("SELECT branch_id FROM orders WHERE id = $1")
        .bind(order_id).fetch_optional(&state.pool).await.map_err(internal)?;
    if let Some((bid,)) = meta {
        state.publish_sse(domain::SseSignal::OrderChanged {
            order_id: domain::OrderId::new(order_id),
            branch_id: domain::BranchId::new(bid),
        }).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── P04: Resolve Clarification ────────────────────────────────────────────── //

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
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── Approve Invoice ───────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct ApproveInvoiceRequest {
    pub invoice_id: Uuid,
}

/// POST /branches/:branch_id/supply-requests/:supply_request_id/approve-invoice
pub async fn approve_invoice(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, supply_request_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<ApproveInvoiceRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let supply_request_id = domain::SupplyRequestId::new(supply_request_id);

    let event = DomainEvent::InvoiceApproved {
        invoice_id: InvoiceId::new(req.invoice_id),
        branch_id: BranchId::new(branch_id),
    };

    let seq = state
        .supply_request_events
        .current_sequence(supply_request_id)
        .await
        .map_err(internal)?;

    state
        .event_sourcing
        .append(BranchId::new(branch_id), seq + 1, &event)
        .await
        .map_err(internal)?;

    event_handler::fan_out(&state, &event).await;

    let signal = state
        .projection_worker
        .project_supply_request(supply_request_id)
        .await
        .map_err(internal)?;
    state.publish_sse(signal).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── Message Worker ────────────────────────────────────────────────────────── //

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

    Ok(StatusCode::NO_CONTENT)
}

// ── Shared helpers ────────────────────────────────────────────────────────── //

async fn send_message_to_worker(
    state: &AppState,
    worker_id: Uuid,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT channel, external_id FROM actor_directory \
         WHERE actor_id = $1 AND actor_type = 'worker' AND owner_confirmed = TRUE",
    )
    .bind(worker_id)
    .fetch_optional(&state.pool)
    .await?;

    let (channel_str, external_id) =
        row.ok_or("worker has no confirmed channel binding")?;

    let channel = parse_channel(&channel_str)?;
    let identity = ChannelIdentity { channel, external_id };

    match identity.channel {
        Channel::Line      => state.line.send_push(&identity, text).await?,
        Channel::WhatsApp  => state.whatsapp.send_push(&identity, text).await?,
        Channel::Telegram  => state.telegram.send_push(&identity, text).await?,
    }
    Ok(())
}

async fn append_and_project_order(
    state: &AppState,
    branch_id: BranchId,
    order_id: OrderId,
    event: DomainEvent,
) -> Result<StatusCode, (StatusCode, String)> {
    let seq = state
        .order_events
        .current_sequence(order_id)
        .await
        .map_err(internal)?;

    state
        .event_sourcing
        .append(branch_id, seq + 1, &event)
        .await
        .map_err(internal)?;

    event_handler::fan_out(state, &event).await;

    let signal = state
        .projection_worker
        .project_order(order_id)
        .await
        .map_err(internal)?;
    state.publish_sse(signal).await;

    Ok(StatusCode::NO_CONTENT)
}

fn parse_channel(s: &str) -> Result<Channel, Box<dyn std::error::Error>> {
    match s {
        "line"      => Ok(Channel::Line),
        "whats_app" => Ok(Channel::WhatsApp),
        "telegram"  => Ok(Channel::Telegram),
        other       => Err(format!("unknown channel: {other}").into()),
    }
}
