//! GET reads from T02's projection table. POST creates the Order directly —
//! Order creation isn't in T01's DomainEvent enum (Owner creates Orders
//! directly; the Agent never originates them), so this writes straight to
//! the `orders` metadata table, not the event stream.

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
}

pub async fn list_orders(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<OrderView>>, (StatusCode, String)> {
    let rows = state.projections.orders_by_branch(branch_id).await.map_err(internal)?;
    Ok(Json(
        rows.into_iter()
            .map(|r| OrderView { id: r.id, customer_id: r.customer_id, description: r.description, state: r.state })
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct CreateOrderRequest {
    pub customer_id: Uuid,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct CreateOrderResponse {
    pub id: Uuid,
}

pub async fn create_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
    Json(req): Json<CreateOrderRequest>,
) -> Result<(StatusCode, Json<CreateOrderResponse>), (StatusCode, String)> {
    let id = Uuid::new_v4();

    sqlx::query("INSERT INTO orders (id, branch_id, customer_id, description) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(branch_id)
        .bind(req.customer_id)
        .bind(&req.description)
        .execute(&state.pool)
        .await
        .map_err(|e| { eprintln!("INSERT orders failed: {e}"); internal(e) })?;

    state.projections
        .upsert_order_state(
            domain::OrderId::new(id),
            branch_id,
            req.customer_id,
            &req.description,
            domain::OrderState::Unassigned,
        )
        .await
        .map_err(|e| { eprintln!("upsert_order_state failed: {e}"); internal(e) })?;

    Ok((StatusCode::CREATED, Json(CreateOrderResponse { id })))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    eprintln!("INTERNAL ERROR: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
