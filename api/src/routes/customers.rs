//! D04: Customer endpoints.
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{extractors::AuthorizedBranch, state::AppState};

#[derive(Debug, Serialize)]
pub struct CustomerView { pub id: Uuid, pub name: String }

pub async fn list_customers(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Result<Json<Vec<CustomerView>>, (StatusCode, String)> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, name FROM customers WHERE branch_id = $1 ORDER BY name ASC",
    ).bind(branch_id).fetch_all(&state.pool).await.map_err(internal)?;
    Ok(Json(rows.into_iter().map(|(id, name)| CustomerView { id, name }).collect()))
}

#[derive(Debug, Deserialize)]
pub struct CreateCustomerRequest { pub name: String }

#[derive(Debug, Serialize)]
pub struct CreateCustomerResponse { pub id: Uuid, pub name: String }

pub async fn create_customer(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
    Json(req): Json<CreateCustomerRequest>,
) -> Result<(StatusCode, Json<CreateCustomerResponse>), (StatusCode, String)> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO customers (id, branch_id, name) VALUES ($1, $2, $3)")
        .bind(id).bind(branch_id).bind(&req.name)
        .execute(&state.pool).await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(CreateCustomerResponse { id, name: req.name })))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
