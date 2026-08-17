//! Supply request list, create, and send endpoints.
use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use domain::{BranchId, DomainEvent, SupplyRequestId};
use crate::{extractors::AuthorizedBranch, state::AppState};

#[derive(Debug, Serialize)]
pub struct SupplyRequestView {
    pub id: Uuid,
    pub description: String,
    pub state: String,
    pub order_ids: Vec<Uuid>,
}

pub async fn list_supply_requests(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<SupplyRequestView>>, (StatusCode, String)> {
    let rows: Vec<(Uuid, String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT s.id, s.description, p.state, COALESCE(s.order_ids::jsonb, '[]'::jsonb) \
         FROM supply_request_current_state p \
         JOIN supply_requests s ON s.id = p.supply_request_id \
         WHERE p.branch_id = $1 ORDER BY p.updated_at DESC",
    ).bind(branch_id).fetch_all(&state.pool).await.map_err(internal)?;

    Ok(Json(rows.into_iter().map(|(id, description, state_str, order_ids_json)| {
        let order_ids: Vec<Uuid> = serde_json::from_value(order_ids_json).unwrap_or_default();
        SupplyRequestView { id, description, state: state_str, order_ids }
    }).collect()))
}

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
    .bind(id).bind(branch_id).bind(&req.description)
    .bind(serde_json::to_value(&req.order_ids).unwrap_or_default())
    .execute(&state.pool).await.map_err(internal)?;

    state.projections.upsert_supply_request_state(
        domain::SupplyRequestId::new(id), branch_id, &req.description,
        domain::SupplyRequestState::Draft,
    ).await.map_err(internal)?;

    Ok((StatusCode::CREATED, Json(CreateSupplyRequestResponse { id })))
}

/// D08-4: POST /supply-requests/:id/send — Owner dispatches Draft SR to Supplier.
pub async fn send_supply_request(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path(params): Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let supply_request_id = params.get("supply_request_id")
        .ok_or((StatusCode::BAD_REQUEST, "missing supply_request_id".to_string()))?
        .parse::<Uuid>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid supply_request_id".to_string()))?;

    let supply_request_id = SupplyRequestId::new(supply_request_id);
    let branch_id_typed = BranchId::new(branch_id);
    let event = DomainEvent::SupplyRequestSent { supply_request_id, branch_id: branch_id_typed };

    let seq = state.supply_request_events.current_sequence(supply_request_id)
        .await.map_err(internal)?;
    state.event_sourcing.append(branch_id_typed, seq + 1, &event).await.map_err(internal)?;

    crate::event_handler::fan_out(&state, &event).await;

    let signal = state.projection_worker.project_supply_request(supply_request_id)
        .await.map_err(internal)?;
    state.publish_sse(signal).await;

    Ok(StatusCode::NO_CONTENT)
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
