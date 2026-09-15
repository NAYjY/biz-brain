//! Pure order state transition commands — no direct worker messaging.
//! All routes live under /branches/:branch_id/orders/:order_id/...
//!
//! F04: message_worker and resolve_clarification clear unread count on reply
//!      (those live in worker.rs; this file only handles state transitions).
//! F01: set_short_name and edit_description endpoints.
//! P16: force-state and reassign-worker endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use domain::{BranchId, DomainEvent, OrderId, WorkerId};

use crate::{extractors::AuthorizedBranch, state::AppState};
use crate::routes::orders::{is_unique_violation, validate_short_name};

use super::{append_and_project_order, bad_request, internal, send_message_to_worker};

// ── Assign worker ─────────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct AssignWorkerRequest {
    pub worker_id: Uuid,
}

/// POST /branches/:branch_id/orders/:order_id/assign-worker
pub async fn assign_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<AssignWorkerRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let binding: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM actor_directory \
         WHERE actor_id = $1 AND actor_type = 'worker' AND owner_confirmed = TRUE",
    )
    .bind(req.worker_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    if binding.is_none() {
        return Err(bad_request(
            "Worker has no confirmed channel binding. \
             Confirm it on the Workers & Suppliers page first.",
        ));
    }

    let event = DomainEvent::WorkerAssigned {
        worker_id: WorkerId::new(req.worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── Close order ───────────────────────────────────────────────────────────── //

/// POST /branches/:branch_id/orders/:order_id/close
pub async fn close_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let event = DomainEvent::OrderDone { order_id: OrderId::new(order_id) };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── Cancel order ──────────────────────────────────────────────────────────── //

/// POST /branches/:branch_id/orders/:order_id/cancel
pub async fn cancel_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let event = DomainEvent::OwnerCancelled { order_id: OrderId::new(order_id) };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── Reset order ───────────────────────────────────────────────────────────── //

/// POST /branches/:branch_id/orders/:order_id/reset
pub async fn reset_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let event = DomainEvent::OrderReset { order_id: OrderId::new(order_id) };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P16: Force-state ──────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct ForceStateRequest {
    pub note: Option<String>,
}

/// POST /branches/:branch_id/orders/:order_id/force-accepted
pub async fn force_accepted(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(_req): Json<ForceStateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id)
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let worker_id = row
        .and_then(|(id,)| id)
        .ok_or_else(|| bad_request("no worker assigned to this order"))?;

    let event = DomainEvent::OwnerForceAccepted {
        worker_id: WorkerId::new(worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

/// POST /branches/:branch_id/orders/:order_id/force-unavailable
pub async fn force_unavailable(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(_req): Json<ForceStateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id)
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let worker_id = row
        .and_then(|(id,)| id)
        .ok_or_else(|| bad_request("no worker assigned — assign a worker first"))?;

    let event = DomainEvent::OwnerForceUnavailable {
        worker_id: WorkerId::new(worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

/// POST /branches/:branch_id/orders/:order_id/force-clarification
pub async fn force_clarification(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(_req): Json<ForceStateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id)
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let worker_id = row
        .and_then(|(id,)| id)
        .ok_or_else(|| bad_request("no worker assigned to this order"))?;

    let event = DomainEvent::OwnerForceClarification {
        worker_id: WorkerId::new(worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

/// POST /branches/:branch_id/orders/:order_id/force-ready
pub async fn force_ready(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(_req): Json<ForceStateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id)
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let worker_id = row
        .and_then(|(id,)| id)
        .ok_or_else(|| bad_request("no worker assigned to this order"))?;

    let event = DomainEvent::OwnerForceReady {
        worker_id: WorkerId::new(worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P16: Reassign worker ──────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct ReassignWorkerRequest {
    pub worker_id: Uuid,
}

/// POST /branches/:branch_id/orders/:order_id/reassign-worker
pub async fn reassign_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<ReassignWorkerRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let binding: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM actor_directory \
         WHERE actor_id = $1 AND actor_type = 'worker' AND owner_confirmed = TRUE",
    )
    .bind(req.worker_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    if binding.is_none() {
        return Err(bad_request(
            "Worker has no confirmed channel binding. Confirm it on the Workers & Suppliers page first.",
        ));
    }

    let event = DomainEvent::OwnerReassignWorker {
        new_worker_id: WorkerId::new(req.worker_id),
        order_id: OrderId::new(order_id),
    };
    append_and_project_order(&state, BranchId::new(branch_id), OrderId::new(order_id), event).await
}

// ── P16: Edit description ─────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct EditDescriptionRequest {
    pub description: String,
}

/// PATCH /branches/:branch_id/orders/:order_id/description
pub async fn edit_description(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<EditDescriptionRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let new_desc = req.description.trim().to_string();
    if new_desc.is_empty() {
        return Err(bad_request("description cannot be empty"));
    }
    if new_desc.len() > 1000 {
        return Err(bad_request("description max 1000 characters"));
    }

    let row: Option<(String,)> = sqlx::query_as(
        "SELECT COALESCE( \
            (SELECT new_description FROM order_description_edits \
             WHERE order_id = $1 ORDER BY id DESC LIMIT 1), \
            o.description \
         ) \
         FROM orders o \
         WHERE o.id = $1 AND o.branch_id = $2 AND o.deleted_at IS NULL",
    )
    .bind(order_id)
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let old_desc = match row {
        Some((d,)) => d,
        None => return Err((StatusCode::NOT_FOUND, "order not found".to_string())),
    };

    sqlx::query(
        "INSERT INTO order_description_edits (order_id, new_description) VALUES ($1, $2)",
    )
    .bind(order_id)
    .bind(&new_desc)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    sqlx::query(
        "UPDATE order_current_state SET description = $1, updated_at = NOW() WHERE order_id = $2",
    )
    .bind(&new_desc)
    .bind(order_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    let worker_row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1",
    )
    .bind(order_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    if let Some((Some(worker_id),)) = worker_row {
        let msg = format!(
            "ℹ️ Order description updated by Owner.\n\
             Before: {old_desc}\n\
             After:  {new_desc}"
        );
        let _ = send_message_to_worker(&state, worker_id, &msg).await;
    }

    let meta: Option<(uuid::Uuid,)> =
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

// ── F01: Set short name ───────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct SetShortNameRequest {
    pub short_name: Option<String>,
}

/// PATCH /branches/:branch_id/orders/:order_id/short-name
pub async fn set_short_name(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(req): Json<SetShortNameRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let short_name = validate_short_name(req.short_name.as_deref())?;

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

    sqlx::query(
        "UPDATE orders SET short_name = $1 WHERE id = $2 AND branch_id = $3",
    )
    .bind(short_name.as_deref())
    .bind(order_id)
    .bind(branch_id)
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
        .set_short_name(OrderId::new(order_id), short_name.as_deref())
        .await
        .map_err(internal)?;

    let meta: Option<(uuid::Uuid,)> =
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

// ── P16: Delete order (soft) ──────────────────────────────────────────────── //

/// DELETE /branches/:branch_id/orders/:order_id
pub async fn delete_order(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, order_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT state FROM order_current_state WHERE order_id = $1 AND branch_id = $2",
    )
    .bind(order_id)
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    if let Some((state_str,)) = row {
        let blocked = matches!(
            state_str.as_str(),
            "ASSIGNED" | "ACCEPTED" | "PENDING_CLARIFICATION" | "READY_FOR_PICKUP"
        );
        if blocked {
            return Err((
                StatusCode::CONFLICT,
                format!("Cannot delete an order in state '{state_str}'. Cancel or close it first."),
            ));
        }
    }

    let result = sqlx::query(
        "UPDATE orders SET deleted_at = NOW() WHERE id = $1 AND branch_id = $2 AND deleted_at IS NULL",
    )
    .bind(order_id)
    .bind(branch_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "order not found".to_string()));
    }

    sqlx::query("DELETE FROM order_current_state WHERE order_id = $1")
        .bind(order_id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;

    let meta: Option<(uuid::Uuid,)> = sqlx::query_as("SELECT branch_id FROM orders WHERE id = $1")
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