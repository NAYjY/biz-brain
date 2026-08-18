//! D05 / P05: Invoice endpoints.
//! GET /api/v1/branches/:branch_id/invoices          — list by state filter
//! GET /api/v1/branches/:branch_id/invoices/:id/media — P05: serve stored media bytes

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
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
    /// P05: true when media bytes are stored.
    pub has_media: bool,
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

    let rows: Vec<(Uuid, Uuid, Uuid, String, Option<String>, bool)> = sqlx::query_as(
        "SELECT ics.invoice_id, ics.supply_request_id, ics.supplier_id, ics.state, ics.notes,
                (i.media_data IS NOT NULL) AS has_media
         FROM invoice_current_state ics
         JOIN invoices i ON i.id = ics.invoice_id
         WHERE ics.branch_id = $1 AND ics.state = $2
         ORDER BY ics.updated_at DESC",
    )
    .bind(branch_id)
    .bind(state_filter)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, supply_request_id, supplier_id, st, notes, has_media)| InvoiceView {
                id,
                supply_request_id,
                supplier_id,
                state: st,
                notes,
                has_media,
            })
            .collect(),
    ))
}

/// P05: serve the raw media bytes for an invoice.
/// The Owner opens this URL in the dashboard modal to view the image/PDF.
pub async fn get_invoice_media(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(invoice_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Response {

    // Verify the invoice belongs to this Branch.
    let row: Option<(Vec<u8>, String)> = sqlx::query_as(
        "SELECT i.media_data, COALESCE(i.media_mime_type, 'application/octet-stream') \
         FROM invoices i \
         JOIN invoice_current_state ics ON ics.invoice_id = i.id \
         WHERE i.id = $1 AND ics.branch_id = $2 AND i.media_data IS NOT NULL",
    )
    .bind(invoice_id)
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();

    match row {
        Some((data, mime_type)) => (
            [(header::CONTENT_TYPE, mime_type)],
            Body::from(data),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
