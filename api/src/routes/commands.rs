//! T05 / P04: one command endpoint per `DomainEvent` variant the Owner can
//! trigger directly. After storing each event, calls event_handler::fan_out
//! so Workers/Suppliers receive their LINE/WhatsApp/Telegram push.
//!
//! P04: three new endpoints —
//!   POST /orders/:id/cancel         → OwnerCancelled
//!   POST /orders/:id/reset          → OrderReset
//!   POST /orders/:id/resolve-clarification → ClarificationResolved

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

/// `POST /branches/:branch_id/orders/:order_id/assign-worker`
pub async fn assign_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
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
            "This worker has no confirmed channel binding. \
             Go to Workers & Suppliers to confirm their binding first.",
        ));
    }

    let order_id = path_uuid(&params, "order_id")?;
    let event = DomainEvent::WorkerAssigned {
        worker_id: WorkerId::new(req.worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── Close Order ───────────────────────────────────────────────────────────── //

/// `POST /branches/:branch_id/orders/:order_id/close`
pub async fn close_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let order_id = path_uuid(&params, "order_id")?;
    let event = DomainEvent::OrderDone { order_id: OrderId::new(order_id) };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P04: Cancel Order (OwnerCancelled) ───────────────────────────────────── //

/// `POST /branches/:branch_id/orders/:order_id/cancel`
/// Owner explicitly cancels an order from any non-Done state.
/// Pushes a notification to the assigned Worker if one exists.
pub async fn cancel_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let order_id = path_uuid(&params, "order_id")?;
    let event = DomainEvent::OwnerCancelled { order_id: OrderId::new(order_id) };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P04: Reset Order (OrderReset) ────────────────────────────────────────── //

/// `POST /branches/:branch_id/orders/:order_id/reset`
/// Owner resets a Cancelled or Unavailable order back to Unassigned.
pub async fn reset_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let order_id = path_uuid(&params, "order_id")?;
    let event = DomainEvent::OrderReset { order_id: OrderId::new(order_id) };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P04: Resolve Clarification (ClarificationResolved) ───────────────────── //

#[derive(Debug, Deserialize)]
pub struct ResolveClarificationRequest {
    /// Message text sent back to the Worker.
    pub message: String,
}

/// `POST /branches/:branch_id/orders/:order_id/resolve-clarification`
/// Owner resolves a Worker's clarification, sending a message and re-assigning.
pub async fn resolve_clarification(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
    Json(req): Json<ResolveClarificationRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let order_id = path_uuid(&params, "order_id")?;

    // Look up the assigned worker_id from the projection.
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

    // Send the Owner's reply message to the Worker first.
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

/// `POST /branches/:branch_id/supply-requests/:supply_request_id/approve-invoice`
pub async fn approve_invoice(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
    Json(req): Json<ApproveInvoiceRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let supply_request_id = path_uuid(&params, "supply_request_id")?;
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

// ── Message Worker (Owner → Worker free text) ─────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct MessageWorkerRequest {
    pub text: String,
}

/// `POST /branches/:branch_id/orders/:order_id/message-worker`
pub async fn message_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
    Json(req): Json<MessageWorkerRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let order_id = path_uuid(&params, "order_id")?;

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
    let identity_row: Option<(String, String)> = sqlx::query_as(
        "SELECT channel, external_id FROM actor_directory \
         WHERE actor_id = $1 AND actor_type = 'worker' AND owner_confirmed = TRUE",
    )
    .bind(worker_id)
    .fetch_optional(&state.pool)
    .await?;

    let (channel_str, external_id) = identity_row
        .ok_or("worker has no confirmed channel binding")?;

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

fn path_uuid(
    params: &std::collections::HashMap<String, String>,
    key: &str,
) -> Result<Uuid, (StatusCode, String)> {
    params
        .get(key)
        .ok_or_else(|| bad_request(format!("missing {key} in path")))?
        .parse()
        .map_err(|_| bad_request(format!("{key} is not a valid UUID")))
}

fn parse_channel(s: &str) -> Result<Channel, Box<dyn std::error::Error>> {
    match s {
        "line"      => Ok(Channel::Line),
        "whats_app" => Ok(Channel::WhatsApp),
        "telegram"  => Ok(Channel::Telegram),
        other       => Err(format!("unknown channel: {other}").into()),
    }
}
