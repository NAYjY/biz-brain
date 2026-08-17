//! Worker management endpoints.
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{extractors::AuthorizedBranch, state::AppState};

#[derive(Debug, Serialize)]
pub struct WorkerView {
    pub id: Uuid,
    pub name: String,
    pub bound: bool,
    pub channel: Option<String>,
    pub external_id: Option<String>,
}

pub async fn list_workers(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<WorkerView>>, (StatusCode, String)> {
    let rows: Vec<(Uuid, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT w.id, w.name, ad.channel, ad.external_id \
         FROM workers w \
         LEFT JOIN actor_directory ad \
           ON ad.actor_id = w.id AND ad.actor_type = 'worker' AND ad.owner_confirmed = TRUE \
         WHERE w.branch_id = $1 ORDER BY w.name ASC",
    ).bind(branch_id).fetch_all(&state.pool).await.map_err(internal)?;

    Ok(Json(rows.into_iter().map(|(id, name, channel, external_id)| WorkerView {
        id, name, bound: channel.is_some(), channel, external_id,
    }).collect()))
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkerRequest { pub name: String }

#[derive(Debug, Serialize)]
pub struct CreateWorkerResponse { pub id: Uuid, pub name: String }

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
        .bind(id).bind(branch_id).bind(&name)
        .execute(&state.pool).await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(CreateWorkerResponse { id, name })))
}

pub async fn delete_worker(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    axum::extract::Path(params): axum::extract::Path<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let worker_id = params.get("worker_id")
        .ok_or((StatusCode::BAD_REQUEST, "missing worker_id".to_string()))?
        .parse::<Uuid>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid worker_id".to_string()))?;

    let (active,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM order_current_state \
         WHERE worker_id = $1 AND state NOT IN ('done','cancelled','unavailable')",
    ).bind(worker_id).fetch_one(&state.pool).await.map_err(internal)?;

    if active > 0 {
        return Err((StatusCode::CONFLICT, format!("Worker has {active} active order(s)")));
    }

    sqlx::query("DELETE FROM actor_directory WHERE actor_id = $1 AND actor_type = 'worker'")
        .bind(worker_id).execute(&state.pool).await.map_err(internal)?;

    let result = sqlx::query("DELETE FROM workers WHERE id = $1 AND branch_id = $2")
        .bind(worker_id).bind(branch_id)
        .execute(&state.pool).await.map_err(internal)?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "worker not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
