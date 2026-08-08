//! D05: Invoice listing endpoint.
//! GET /api/v1/branches/:branch_id/invoices?state=Sent
//!
//! Returns invoices from `invoice_current_state` projection.
//! Default filter: state=Sent (only actionable invoices shown in Approve-Invoice picker).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{extractors::AuthorizedBranch, state::AppState};

#[derive(Debug, Serialize)]
pub struct InvoiceView {
    pub id: Uuid,
    pub supply_request_id: Uuid,
    pub supplier_id: Uuid,
    pub state: String,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InvoiceFilter {
    pub state: Option<String>,
}

pub async fn list_invoices(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Query(filter): Query<InvoiceFilter>,
    State(state): State<AppState>,
) -> Result<Json<Vec<InvoiceView>>, (StatusCode, String)> {
    let state_filter = filter.state.as_deref().unwrap_or("Sent");

    let rows: Vec<(Uuid, Uuid, Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT invoice_id, supply_request_id, supplier_id, state, notes
         FROM invoice_current_state
         WHERE branch_id = $1 AND state = $2
         ORDER BY updated_at DESC",
    )
    .bind(branch_id)
    .bind(state_filter)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, supply_request_id, supplier_id, st, notes)| InvoiceView {
                id,
                supply_request_id,
                supplier_id,
                state: st,
                notes,
            })
            .collect(),
    ))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
