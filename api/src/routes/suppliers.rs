//! T10: Supplier management endpoints.
//! POST   /api/v1/branches/:branch_id/suppliers
//! GET    /api/v1/branches/:branch_id/suppliers
//! DELETE /api/v1/branches/:branch_id/suppliers/:supplier_id

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{extractors::AuthorizedBranch, state::AppState};

#[derive(Debug, Serialize)]
pub struct SupplierView {
    pub id: Uuid,
    pub name: String,
    pub bound: bool,
    pub channel: Option<String>,
    pub external_id: Option<String>,
}

pub async fn list_suppliers(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<SupplierView>>, (StatusCode, String)> {
    let rows: Vec<(Uuid, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT s.id, s.name, ad.channel, ad.external_id \
         FROM suppliers s \
         LEFT JOIN actor_directory ad \
           ON ad.actor_id = s.id AND ad.actor_type = 'supplier' AND ad.owner_confirmed = TRUE \
         WHERE s.branch_id = $1 ORDER BY s.name ASC",
    )
    .bind(branch_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    Ok(Json(rows.into_iter().map(|(id, name, channel, external_id)| SupplierView {
        id, name, bound: channel.is_some(), channel, external_id,
    }).collect()))
}

#[derive(Debug, Deserialize)]
pub struct CreateSupplierRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CreateSupplierResponse {
    pub id: Uuid,
    pub name: String,
}

pub async fn create_supplier(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
    Json(req): Json<CreateSupplierRequest>,
) -> Result<(StatusCode, Json<CreateSupplierResponse>), (StatusCode, String)> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name required".to_string()));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO suppliers (id, branch_id, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(branch_id)
        .bind(&name)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok((StatusCode::CREATED, Json(CreateSupplierResponse { id, name })))
}

/// DELETE /branches/:branch_id/suppliers/:supplier_id
/// Blocked (409) if the supplier has an active confirmed binding linked to any
/// non-terminal supply request in this branch.
pub async fn delete_supplier(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    Path((_branch_id, supplier_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let (active,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint \
         FROM supply_request_current_state srcs \
         JOIN actor_directory ad \
           ON ad.actor_id = $1 \
           AND ad.actor_type = 'supplier' \
           AND ad.owner_confirmed = TRUE \
           AND ad.branch_id = srcs.branch_id \
         WHERE srcs.branch_id = $2 \
           AND srcs.state NOT IN ('SUPPLIER_CONFIRMED')",
    )
    .bind(supplier_id)
    .bind(branch_id)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;

    if active > 0 {
        return Err((StatusCode::CONFLICT, format!("Supplier has {active} active supply request(s)")));
    }

    sqlx::query("DELETE FROM actor_directory WHERE actor_id = $1 AND actor_type = 'supplier'")
        .bind(supplier_id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;

    let result = sqlx::query("DELETE FROM suppliers WHERE id = $1 AND branch_id = $2")
        .bind(supplier_id)
        .bind(branch_id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "supplier not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}