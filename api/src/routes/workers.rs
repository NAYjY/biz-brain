//! Worker management endpoints.
//!
//! GET  /api/v1/branches/:branch_id/workers        — list workers + binding status
//! POST /api/v1/branches/:branch_id/workers        — create worker row (name only)
//!
//! Worker onboarding flow (S06):
//!   1. Owner creates Worker here (gets a WorkerId)
//!   2. Owner tells Worker to message the LINE bot
//!   3. inbox_worker sees unknown sender -> pending binding appears in /actors/pending
//!   4. Owner goes to /actors page, confirms the binding linking sender -> WorkerId
//!   5. Worker's messages now route correctly

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{extractors::AuthorizedBranch, state::AppState};

#[derive(Debug, Serialize)]
pub struct WorkerView {
    pub id: Uuid,
    pub name: String,
    /// Whether this worker has a confirmed channel binding (can receive messages).
    pub bound: bool,
    /// Channel they're bound to, if any.
    pub channel: Option<String>,
    /// Their external_id on that channel, if bound.
    pub external_id: Option<String>,
}

/// List all Workers for the Branch with their binding status.
pub async fn list_workers(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<WorkerView>>, (StatusCode, String)> {
    // Left join workers against actor_directory to get binding status in one query.
    let rows: Vec<(Uuid, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT w.id, w.name, ad.channel, ad.external_id \
         FROM workers w \
         LEFT JOIN actor_directory ad \
           ON ad.actor_id = w.id \
           AND ad.actor_type = 'worker' \
           AND ad.owner_confirmed = TRUE \
         WHERE w.branch_id = $1 \
         ORDER BY w.name ASC",
    )
    .bind(branch_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, name, channel, external_id)| WorkerView {
                id,
                name,
                bound: channel.is_some(),
                channel,
                external_id,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkerRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CreateWorkerResponse {
    pub id: Uuid,
    pub name: String,
}

/// Create a Worker row. Does NOT create a binding — Worker must message
/// the bot first, then Owner confirms the pending binding on the actors page.
pub async fn create_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
    Json(req): Json<CreateWorkerRequest>,
) -> Result<(StatusCode, Json<CreateWorkerResponse>), (StatusCode, String)> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name required".to_string()));
    }

    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO workers (id, branch_id, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(branch_id)
        .bind(&name)
        .execute(&state.pool)
        .await
        .map_err(internal)?;

    Ok((StatusCode::CREATED, Json(CreateWorkerResponse { id, name })))
}

/// Delete a Worker. Also removes any actor_directory bindings for this worker.
/// Only safe if worker has no active orders.
pub async fn delete_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    axum::extract::Path(params): axum::extract::Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let worker_id = params
        .get("worker_id")
        .ok_or((StatusCode::BAD_REQUEST, "missing worker_id".to_string()))?
        .parse::<Uuid>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid worker_id".to_string()))?;

    // Check no active orders
    let active: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM order_current_state \
         WHERE worker_id = $1 AND state NOT IN ('done', 'cancelled', 'unavailable')",
    )
    .bind(worker_id)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;

    if active.0 > 0 {
        return Err((
            StatusCode::CONFLICT,
            format!("Worker has {} active order(s) — cannot delete", active.0),
        ));
    }

    // Remove bindings first (FK constraint)
    sqlx::query(
        "DELETE FROM actor_directory WHERE actor_id = $1 AND actor_type = 'worker'",
    )
    .bind(worker_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    let result = sqlx::query("DELETE FROM workers WHERE id = $1 AND branch_id = $2")
        .bind(worker_id)
        .bind(branch_id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "worker not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}