//! Supply request list, create, and send endpoints.
//! T09: list_supply_requests returns { items, next_cursor } with cursor pagination.
//!      Supports ?after=<cursor>&limit=50&state=DRAFT,SENT (state filter only per T09).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use domain::{BranchId, DomainEvent, SupplyRequestId};
use store::{PaginatedProjections, SrCursor, SrFilter, PAGE_SIZE};
use crate::{extractors::AuthorizedBranch, state::AppState};

// ── List query params ─────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize, Default)]
pub struct ListSrQuery {
    pub after: Option<String>,
    pub limit: Option<i64>,
    /// Comma-separated state filter e.g. `DRAFT,SENT`
    pub state: Option<String>,
}

// ── Response types ────────────────────────────────────────────────────────── //

#[derive(Debug, Serialize)]
pub struct SupplyRequestView {
    pub id: Uuid,
    pub description: String,
    pub state: String,
    pub order_ids: Vec<Uuid>,
}

/// T09: paginated response envelope.
#[derive(Debug, Serialize)]
pub struct SupplyRequestsPage {
    pub items: Vec<SupplyRequestView>,
    pub next_cursor: Option<String>,
}

// ── Handlers ──────────────────────────────────────────────────────────────── //

pub async fn list_supply_requests(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Query(params): Query<ListSrQuery>,
    State(state): State<AppState>,
) -> Result<Json<SupplyRequestsPage>, (StatusCode, String)> {
    let after_cursor: Option<SrCursor> = params
        .after
        .as_deref()
        .and_then(SrCursor::decode);

    let filter = SrFilter {
        states: params
            .state
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_uppercase)
            .collect(),
    };

    let limit = params.limit.unwrap_or(PAGE_SIZE);

    let paginator = PaginatedProjections::new(state.pool.clone());
    let (rows, next_cursor) = paginator
        .supply_requests_page(branch_id, after_cursor.as_ref(), &filter, limit)
        .await
        .map_err(internal)?;

    let items = rows
        .into_iter()
        .map(|r| {
            let order_ids: Vec<Uuid> =
                serde_json::from_value(r.order_ids).unwrap_or_default();
            SupplyRequestView {
                id: r.id,
                description: r.description,
                state: r.state,
                order_ids,
            }
        })
        .collect();

    Ok(Json(SupplyRequestsPage {
        items,
        next_cursor: next_cursor.map(|c| c.encode()),
    }))
}

// ── Create ────────────────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct CreateSupplyRequestRequest {
    pub description: String,
    pub order_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct CreateSupplyRequestResponse { pub id: Uuid }

pub async fn create_supply_request(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
    Json(req): Json<CreateSupplyRequestRequest>,
) -> Result<(StatusCode, Json<CreateSupplyRequestResponse>), (StatusCode, String)> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO supply_requests (id, branch_id, description, order_ids) VALUES ($1,$2,$3,$4)",
    )
    .bind(id)
    .bind(branch_id)
    .bind(&req.description)
    .bind(serde_json::to_value(&req.order_ids).unwrap_or_default())
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    state.projections
        .upsert_supply_request_state(
            domain::SupplyRequestId::new(id),
            branch_id,
            &req.description,
            domain::SupplyRequestState::Draft,
        )
        .await
        .map_err(internal)?;

    Ok((StatusCode::CREATED, Json(CreateSupplyRequestResponse { id })))
}

// ── Send ──────────────────────────────────────────────────────────────────── //

pub async fn send_supply_request(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, supply_request_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let supply_request_id = SupplyRequestId::new(supply_request_id);
    let branch_id_typed = BranchId::new(branch_id);

    let event = DomainEvent::SupplyRequestSent {
        supply_request_id,
        branch_id: branch_id_typed,
    };

    let seq = state.supply_request_events
        .current_sequence(supply_request_id)
        .await
        .map_err(internal)?;

    state.event_sourcing
        .append(branch_id_typed, seq + 1, &event)
        .await
        .map_err(internal)?;

    crate::event_handler::fan_out(&state, &event).await;

    let signal = state.projection_worker
        .project_supply_request(supply_request_id)
        .await
        .map_err(internal)?;
    state.publish_sse(signal).await;

    Ok(StatusCode::NO_CONTENT)
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}