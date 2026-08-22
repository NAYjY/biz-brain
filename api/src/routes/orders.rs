//! GET reads from T02's projection table.
//! POST creates the Order directly.
//! P16: list_orders filters soft-deleted orders via JOIN.
//! F04: unread_message_count, ai_routed_low_confidence exposed; thread endpoint added.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use domain::OrderId;

use crate::{extractors::AuthorizedBranch, state::AppState};

#[derive(Debug, Serialize)]
pub struct OrderView {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub description: String,
    pub state: String,
    pub worker_id: Option<Uuid>,
    pub worker_name: Option<String>,
    /// F04: replaced inline bubble; used for thread button badge.
    pub unread_message_count: i32,
    /// F04: true when AI routed with low confidence (F02 hook).
    pub ai_routed_low_confidence: bool,
    // Keep for backwards compat (used by SSR worker-message row removal check).
    pub last_worker_message: Option<String>,
    pub last_worker_message_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn list_orders(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<OrderView>>, (StatusCode, String)> {
    let rows = state.projections.orders_by_branch(branch_id).await.map_err(internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|r| OrderView {
                id: r.id,
                customer_id: r.customer_id,
                description: r.description,
                state: r.state,
                worker_id: r.worker_id,
                worker_name: r.worker_name,
                unread_message_count: r.unread_message_count,
                ai_routed_low_confidence: r.ai_routed_low_confidence,
                last_worker_message: r.last_worker_message,
                last_worker_message_at: r.last_worker_message_at,
            })
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
    sqlx::query(
        "INSERT INTO orders (id, branch_id, customer_id, description) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(branch_id)
    .bind(req.customer_id)
    .bind(&req.description)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    state
        .projections
        .upsert_order_state(
            domain::OrderId::new(id),
            branch_id,
            req.customer_id,
            &req.description,
            domain::OrderState::Unassigned,
            None,
        )
        .await
        .map_err(internal)?;

    Ok((StatusCode::CREATED, Json(CreateOrderResponse { id })))
}

// ── F04: Thread endpoint ──────────────────────────────────────────────────── //

/// A single turn in the worker ↔ bot conversation for this order.
#[derive(Debug, Serialize)]
pub struct ThreadMessage {
    pub role: String,
    pub content: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// GET /api/v1/branches/:branch_id/orders/:order_id/thread
///
/// Returns the conversation_history window for the Worker assigned to this
/// order, restricted to messages on or after the order's first event minus
/// one hour (prevents bleed from earlier orders on the same sender key).
///
/// Opening the thread also resets the unread count.
pub async fn get_order_thread(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<Json<Vec<ThreadMessage>>, (StatusCode, String)> {
    // Verify order belongs to this branch.
    let exists: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM orders WHERE id = $1 AND branch_id = $2 AND deleted_at IS NULL",
    )
    .bind(order_id)
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    if exists.is_none() {
        return Err((StatusCode::NOT_FOUND, "order not found".to_string()));
    }

    // Fetch conversation history scoped to this order's timeline.
    let rows: Vec<(String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT ch.role, ch.content, ch.created_at
         FROM conversation_history ch
         JOIN order_current_state ocs ON ocs.order_id = $1
         JOIN actor_directory ad
             ON ad.actor_id = ocs.worker_id
             AND ad.actor_type = 'worker'
             AND ad.owner_confirmed = TRUE
         WHERE ch.sender_key = (ad.channel || ':' || ad.external_id)
           AND ch.created_at >= (
               SELECT COALESCE(MIN(occurred_at), NOW()) - INTERVAL '1 hour'
               FROM order_events WHERE aggregate_id = $1
           )
         ORDER BY ch.created_at ASC
         LIMIT 100",
    )
    .bind(order_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    // Clear unread count now that Owner has seen the thread.
    let _ = state.projections.clear_unread(OrderId::new(order_id)).await;

    // Publish SSE so the badge on the dashboard clears in real-time.
    let meta: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT branch_id FROM orders WHERE id = $1")
            .bind(order_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    if let Some((bid,)) = meta {
        state
            .publish_sse(domain::SseSignal::OrderChanged {
                order_id: domain::OrderId::new(order_id),
                branch_id: domain::BranchId::new(bid),
            })
            .await;
    }

    Ok(Json(
        rows.into_iter()
            .map(|(role, content, created_at)| ThreadMessage { role, content, created_at })
            .collect(),
    ))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
