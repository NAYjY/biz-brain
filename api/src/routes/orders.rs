//! GET reads from T02's projection table.
//! POST creates the Order directly.
//! P16: list_orders filters soft-deleted orders via JOIN.
//! F04: unread_message_count, ai_routed_low_confidence exposed; thread endpoint added.
//! F01: short_name in CreateOrderRequest, OrderView, create_order, list_orders.
//! T09: list_orders is now paginated — returns { items, next_cursor }.
//!      Supports ?after=<cursor>&limit=50&state=ASSIGNED,ACCEPTED&worker_id=<uuid>&q=text

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use domain::OrderId;
use store::{OrderCursor, OrderFilter, PaginatedProjections, PAGE_SIZE};

use crate::{extractors::AuthorizedBranch, state::AppState};

// ── List query params ─────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize, Default)]
pub struct ListOrdersQuery {
    /// Opaque cursor string from previous response's `next_cursor`.
    pub after: Option<String>,
    /// Max rows to return (server clamps to PAGE_SIZE = 50).
    pub limit: Option<i64>,
    /// Comma-separated state filter: e.g. `ASSIGNED,ACCEPTED`
    pub state: Option<String>,
    /// Worker UUID filter
    pub worker_id: Option<Uuid>,
    /// Free-text search against description and short_name
    pub q: Option<String>,
}

// ── Response types ────────────────────────────────────────────────────────── //

#[derive(Debug, Serialize)]
pub struct OrderView {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub description: String,
    pub state: String,
    pub worker_id: Option<Uuid>,
    pub worker_name: Option<String>,
    /// F01: optional short human-readable job name.
    pub short_name: Option<String>,
    /// F04: replaced inline bubble; used for thread button badge.
    pub unread_message_count: i32,
    /// F04: true when AI routed with low confidence (F02 hook).
    pub ai_routed_low_confidence: bool,
    // Keep for backwards compat (used by SSR worker-message row removal check).
    pub last_worker_message: Option<String>,
    pub last_worker_message_at: Option<chrono::DateTime<chrono::Utc>>,
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    pub due_date:   Option<chrono::DateTime<chrono::Utc>>,
    pub alert_count: i64,
}

/// T09: paginated response envelope.
#[derive(Debug, Serialize)]
pub struct OrdersPage {
    pub items: Vec<OrderView>,
    /// Base64-encoded cursor for the next page. `null` when on the last page.
    pub next_cursor: Option<String>,
}

// ── Handlers ──────────────────────────────────────────────────────────────── //

pub async fn list_orders(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Query(params): Query<ListOrdersQuery>,
    State(state): State<AppState>,
) -> Result<Json<OrdersPage>, (StatusCode, String)> {
    // Decode opaque cursor (if provided).
    let after_cursor: Option<OrderCursor> = params
        .after
        .as_deref()
        .and_then(OrderCursor::decode);

    // Build filter.
    let filter = OrderFilter {
        states: params
            .state
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_uppercase)
            .collect(),
        worker_id: params.worker_id,
        q: params.q.filter(|s| !s.trim().is_empty()),
    };

    let limit = params.limit.unwrap_or(PAGE_SIZE);

    let paginator = PaginatedProjections::new(state.pool.clone());
    let (rows, next_cursor) = paginator
        .orders_page(branch_id, after_cursor.as_ref(), &filter, limit)
        .await
        .map_err(internal)?;

    let items = rows
        .into_iter()
        .map(|r| OrderView {
            id: r.id,
            customer_id: r.customer_id,
            description: r.description,
            state: r.state,
            worker_id: r.worker_id,
            worker_name: r.worker_name,
            short_name: r.short_name,
            unread_message_count: r.unread_message_count,
            ai_routed_low_confidence: r.ai_routed_low_confidence,
            last_worker_message: r.last_worker_message,
            last_worker_message_at: r.last_worker_message_at,
            start_date: r.start_date,
            due_date: r.due_date,
            alert_count: r.alert_count,
        })
        .collect();

    Ok(Json(OrdersPage {
        items,
        next_cursor: next_cursor.map(|c| c.encode()),
    }))
}

// ── Create Order ──────────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct CreateOrderRequest {
    pub customer_id: Uuid,
    pub description: String,
    /// F01: optional ≤20-char job name set at create time.
    pub short_name: Option<String>,
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
    // F01: validate and normalise short_name.
    let short_name = validate_short_name(req.short_name.as_deref())?;

    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orders (id, branch_id, customer_id, description, short_name) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(branch_id)
    .bind(req.customer_id)
    .bind(&req.description)
    .bind(short_name.as_deref())
    .execute(&state.pool)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            (StatusCode::CONFLICT, "A job with that short name already exists in this branch.".to_string())
        } else {
            internal(e)
        }
    })?;

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

    // F01: write short_name into projection immediately.
    if short_name.is_some() {
        state
            .projections
            .set_short_name(domain::OrderId::new(id), short_name.as_deref())
            .await
            .map_err(internal)?;
    }

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
pub async fn get_order_thread(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<Json<Vec<ThreadMessage>>, (StatusCode, String)> {
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

    let _ = state.projections.clear_unread(OrderId::new(order_id)).await;

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

// ── F01: shared validation ────────────────────────────────────────────────── //

pub fn validate_short_name(raw: Option<&str>) -> Result<Option<String>, (StatusCode, String)> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) if s.len() > 20 => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "short_name must be 20 characters or fewer".to_string(),
        )),
        Some(s) => Ok(Some(s.to_string())),
    }
}

pub fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}