use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{extractors::AuthorizedBranch, state::AppState};

#[derive(Debug, Serialize)]
pub struct SupplyRequestView {
    pub id: Uuid,
    pub description: String,
    pub state: String,
}

pub async fn list_supply_requests(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<SupplyRequestView>>, (StatusCode, String)> {
    let rows = state.projections.supply_requests_by_branch(branch_id).await.map_err(internal)?;
    Ok(Json(rows.into_iter().map(|r| SupplyRequestView { id: r.id, description: r.description, state: r.state }).collect()))
}

#[derive(Debug, Deserialize)]
pub struct CreateSupplyRequestRequest {
    pub description: String,
    pub order_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct CreateSupplyRequestResponse {
    pub id: Uuid,
}

pub async fn create_supply_request(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
    Json(req): Json<CreateSupplyRequestRequest>,
) -> Result<(StatusCode, Json<CreateSupplyRequestResponse>), (StatusCode, String)> {
    let id = Uuid::new_v4();

    sqlx::query("INSERT INTO supply_requests (id, branch_id, description, order_ids) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(branch_id)
        .bind(&req.description)
        .bind(serde_json::to_value(&req.order_ids).unwrap_or_default())
        .execute(&state.pool)
        .await
        .map_err(internal)?;

    state
        .projections
        .upsert_supply_request_state(domain::SupplyRequestId::new(id), branch_id, &req.description, domain::SupplyRequestState::Draft)
        .await
        .map_err(internal)?;

    Ok((StatusCode::CREATED, Json(CreateSupplyRequestResponse { id })))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
