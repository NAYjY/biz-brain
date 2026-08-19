//! GET reads from T02's projection table (P16: filters soft-deleted orders).
//! POST creates the Order directly (Order creation is not a DomainEvent).
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
    pub last_worker_message: Option<String>,
    pub last_worker_message_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn list_orders(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<OrderView>>, (StatusCode, String)> {
    // P16: JOIN with orders table to exclude soft-deleted rows.
    let rows: Vec<(Uuid, Uuid, String, String, Option<Uuid>, Option<String>, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as(
            "SELECT ocs.order_id, ocs.customer_id, \
                    COALESCE(ocs.description, o.description), ocs.state, \
                    ocs.worker_id, ocs.last_worker_message, ocs.last_worker_message_at \
             FROM order_current_state ocs \
             JOIN orders o ON o.id = ocs.order_id \
             WHERE ocs.branch_id = $1 \
               AND o.deleted_at IS NULL \
             ORDER BY ocs.updated_at DESC",
        )
        .bind(branch_id)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;

    Ok(Json(rows.into_iter().map(|(id, customer_id, description, st, worker_id, lwm, lwma)| {
        OrderView { id, customer_id, description, state: st, worker_id, last_worker_message: lwm, last_worker_message_at: lwma }
    }).collect()))
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
