//! Supply-request scoped commands.
//! POST /branches/:branch_id/supply-requests/:supply_request_id/approve-invoice

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use domain::{BranchId, DomainEvent, InvoiceId};

use crate::{event_handler, extractors::AuthorizedBranch, state::AppState};

use super::{internal, append_and_project_order};

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