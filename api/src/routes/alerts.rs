//! F05: Order date + follow-up alert endpoints.
//!
//! PATCH /branches/:branch_id/orders/:order_id/dates
//!   — set/clear start_date and due_date
//!
//! GET   /branches/:branch_id/orders/:order_id/alerts
//!   — list active alerts for an order
//!
//! POST  /branches/:branch_id/orders/:order_id/alerts
//!   — create a follow-up alert
//!
//! DELETE /branches/:branch_id/orders/:order_id/alerts/:alert_id
//!   — soft-delete an alert

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use store::alerts::{AlertMode, AlertRepository, AlertView};

use crate::{extractors::AuthorizedBranch, state::AppState};

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ── Dates ─────────────────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct SetDatesRequest {
    /// ISO-8601 datetime or null to clear.
    pub start_date: Option<DateTime<Utc>>,
    pub due_date: Option<DateTime<Utc>>,
}

/// PATCH /branches/:branch_id/orders/:order_id/dates
pub async fn set_dates(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<SetDatesRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Verify order belongs to branch and is not deleted.
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

    if let (Some(s), Some(d)) = (req.start_date, req.due_date) {
        if s > d {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                "start_date must be before due_date".to_string(),
            ));
        }
    }

    // Update source table.
    sqlx::query(
        "UPDATE orders SET start_date = $1, due_date = $2 WHERE id = $3",
    )
    .bind(req.start_date)
    .bind(req.due_date)
    .bind(order_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    // Mirror to projection.
    sqlx::query(
        "UPDATE order_current_state \
         SET start_date = $1, due_date = $2, updated_at = NOW() \
         WHERE order_id = $3",
    )
    .bind(req.start_date)
    .bind(req.due_date)
    .bind(order_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    // Fire SSE so dashboard refreshes immediately.
    let meta: Option<(Uuid,)> =
        sqlx::query_as("SELECT branch_id FROM orders WHERE id = $1")
            .bind(order_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(internal)?;
    if let Some((bid,)) = meta {
        state
            .publish_sse(domain::SseSignal::OrderChanged {
                order_id: domain::OrderId::new(order_id),
                branch_id: domain::BranchId::new(bid),
            })
            .await;
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── List alerts ───────────────────────────────────────────────────────────── //

pub async fn list_alerts(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<Json<Vec<AlertView>>, (StatusCode, String)> {
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

    let repo = AlertRepository::new(state.pool.clone());
    let alerts = repo.list_for_order(order_id).await.map_err(internal)?;
    Ok(Json(alerts))
}

// ── Create alert ──────────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct CreateAlertRequest {
    /// "once" | "hourly" | "daily" | "every3d"
    pub alert_mode: String,
    /// When to fire (first fire for recurring modes).
    pub alert_at: DateTime<Utc>,
    /// Optional custom message text.
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateAlertResponse {
    pub id: Uuid,
}

pub async fn create_alert(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<CreateAlertRequest>,
) -> Result<(StatusCode, Json<CreateAlertResponse>), (StatusCode, String)> {
    let mode = AlertMode::from_sql(&req.alert_mode).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "alert_mode must be one of: once, hourly, daily, every3d".to_string(),
        )
    })?;

    if req.alert_at <= Utc::now() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "alert_at must be in the future".to_string(),
        ));
    }

    // Verify order belongs to branch.
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

    let repo = AlertRepository::new(state.pool.clone());
    let id = repo
        .create(order_id, branch_id, &mode, req.alert_at, req.message.as_deref())
        .await
        .map_err(internal)?;

    Ok((StatusCode::CREATED, Json(CreateAlertResponse { id })))
}

// ── Delete alert ──────────────────────────────────────────────────────────── //

/// DELETE /branches/:branch_id/orders/:order_id/alerts/:alert_id
pub async fn delete_alert(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id, alert_id)): Path<(Uuid, Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let repo = AlertRepository::new(state.pool.clone());
    let deleted = repo
        .delete_for_order(alert_id, order_id)
        .await
        .map_err(internal)?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "alert not found".to_string()))
    }
}