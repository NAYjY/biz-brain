//! T05: one command endpoint per `DomainEvent` variant the Owner can
//! trigger directly. After storing each event, calls event_handler::fan_out
//! so Workers/Suppliers receive their LINE/WhatsApp/Telegram push.

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

// ── Message Worker ────────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct MessageWorkerRequest {
    pub text: String,
}

/// `POST /branches/:branch_id/orders/:order_id/message-worker`
/// Owner sends a free-text message to the Worker assigned to this Order.
/// Works on any Order state — Owner may want to send instructions at any point.
pub async fn message_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
    Json(req): Json<MessageWorkerRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let order_id = path_uuid(&params, "order_id")?;

    let text = req.text.trim().to_string();
    if text.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message text required".to_string()));
    }

    // Get worker_id from projection
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
        .ok_or((StatusCode::BAD_REQUEST, "no worker assigned to this order".to_string()))?;

    // Get worker's confirmed channel identity
    let identity_row: Option<(String, String)> = sqlx::query_as(
        "SELECT channel, external_id FROM actor_directory \
         WHERE actor_id = $1 AND actor_type = 'worker' AND owner_confirmed = TRUE",
    )
    .bind(worker_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let (channel_str, external_id) = identity_row.ok_or((
        StatusCode::BAD_REQUEST,
        "worker has no confirmed channel binding".to_string(),
    ))?;

    let channel = match channel_str.as_str() {
        "line"      => Channel::Line,
        "whats_app" => Channel::WhatsApp,
        "telegram"  => Channel::Telegram,
        other => return Err((StatusCode::BAD_REQUEST, format!("unknown channel: {other}"))),
    };

    let identity = ChannelIdentity { channel, external_id };

    match identity.channel {
        Channel::Line      => state.line.send_push(&identity, &text).await,
        Channel::WhatsApp  => state.whatsapp.send_push(&identity, &text).await,
        Channel::Telegram  => state.telegram.send_push(&identity, &text).await,
    }
    .map_err(|e| internal(e))?;

    Ok(StatusCode::NO_CONTENT)
}

// ── Shared helpers ────────────────────────────────────────────────────────── //

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
        .ok_or((StatusCode::BAD_REQUEST, format!("missing {key} in path")))?
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, format!("{key} is not a valid UUID")))
}
