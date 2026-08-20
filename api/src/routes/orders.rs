//! GET reads from T02's projection table.
//! POST creates the Order directly.
//! P16: list_orders filters soft-deleted orders via JOIN.
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{extractors::AuthorizedBranch, state::AppState};

#[derive(Debug, Serialize)]
pub struct OrderView {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub description: String,
    pub state: String,
    pub worker_id: Option<Uuid>,
    pub worker_name: Option<String>,
    pub last_worker_message: Option<String>,
    pub last_worker_message_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn list_orders(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<OrderView>>, (StatusCode, String)> {
    // orders_by_branch already excludes soft-deleted via JOIN orders o + deleted_at IS NULL
    let rows = state.projections.orders_by_branch(branch_id).await.map_err(internal)?;

    Ok(Json(rows.into_iter()
        .map(|r| OrderView {
            id: r.id,
            customer_id: r.customer_id,
            description: r.description,
            state: r.state,
            worker_id: r.worker_id,
            worker_name: r.worker_name,
            last_worker_message: r.last_worker_message,
            last_worker_message_at: r.last_worker_message_at,
        })
        .collect()))
}

#[derive(Debug, Deserialize)]
pub struct CreateOrderRequest { pub customer_id: Uuid, pub description: String }

#[derive(Debug, Serialize)]
pub struct CreateOrderResponse { pub id: Uuid }

pub async fn create_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
    Json(req): Json<CreateOrderRequest>,
) -> Result<(StatusCode, Json<CreateOrderResponse>), (StatusCode, String)> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orders (id, branch_id, customer_id, description) VALUES ($1, $2, $3, $4)",
    )
    .bind(id).bind(branch_id).bind(req.customer_id).bind(&req.description)
    .execute(&state.pool).await.map_err(internal)?;

    state.projections.upsert_order_state(
        domain::OrderId::new(id), branch_id, req.customer_id,
        &req.description, domain::OrderState::Unassigned, None,
    ).await.map_err(internal)?;

    Ok((StatusCode::CREATED, Json(CreateOrderResponse { id })))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}