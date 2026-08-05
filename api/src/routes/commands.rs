//! T05: one command endpoint per `DomainEvent` variant the Owner can
//! trigger directly — `WorkerAssigned` (assign a Worker), `OrderDone`
//! (Owner closes the Order), `InvoiceApproved` (Owner approves price/list).
//! Everything else in the enum originates from the Agent (T03), never here.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use domain::{BranchId, DomainEvent, InvoiceId, OrderId, WorkerId};

use crate::{extractors::AuthorizedBranch, state::AppState};

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

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
    let order_id = OrderId::new(order_id);

    let event = DomainEvent::WorkerAssigned { worker_id: WorkerId::new(req.worker_id), order_id };
    append_and_project_order(&state, BranchId::new(branch_id), order_id, event).await
}

/// `POST /branches/:branch_id/orders/:order_id/close`
pub async fn close_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let order_id = path_uuid(&params, "order_id")?;
    let order_id = OrderId::new(order_id);

    let event = DomainEvent::OrderDone { order_id };
    append_and_project_order(&state, BranchId::new(branch_id), order_id, event).await
}

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

    let event = DomainEvent::InvoiceApproved { invoice_id: InvoiceId::new(req.invoice_id), branch_id: BranchId::new(branch_id) };

    let seq = state.supply_request_events.current_sequence(supply_request_id).await.map_err(internal)?;
    state.event_sourcing.append(BranchId::new(branch_id), seq + 1, &event).await.map_err(internal)?;

    let signal = state.projection_worker.project_supply_request(supply_request_id).await.map_err(internal)?;
    state.publish_sse(signal).await;

    Ok(StatusCode::NO_CONTENT)
}

async fn append_and_project_order(
    state: &AppState,
    branch_id: BranchId,
    order_id: OrderId,
    event: DomainEvent,
) -> Result<StatusCode, (StatusCode, String)> {
    let seq = state.order_events.current_sequence(order_id).await.map_err(internal)?;
    state.event_sourcing.append(branch_id, seq + 1, &event).await.map_err(internal)?;

    let signal = state.projection_worker.project_order(order_id).await.map_err(internal)?;
    state.publish_sse(signal).await;

    Ok(StatusCode::NO_CONTENT)
}

fn path_uuid(params: &std::collections::HashMap<String, String>, key: &str) -> Result<Uuid, (StatusCode, String)> {
    params
        .get(key)
        .ok_or((StatusCode::BAD_REQUEST, format!("missing {key} in path")))?
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, format!("{key} is not a valid UUID")))
}
