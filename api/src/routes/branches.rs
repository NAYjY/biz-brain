//! D02 / P01 / T01 / T20: Branch management.
//! T20: `owner_id` → `created_by_user_id`. Creating branches is Owner-only.
//!      Manager management endpoints (create, grant, revoke) are Owner-only.

use axum::{extract::State, http::StatusCode, Json};
use axum::extract::Path;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use agent::AiProvider;
use store::ReplyTemplateRepository;

use crate::{extractors::{AuthedOwner, AuthedOwnerOnly}, state::AppState};

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ── List branches ─────────────────────────────────────────────────────────── //

#[derive(Debug, Serialize)]
pub struct BranchView {
    pub id: Uuid,
    pub name: String,
    pub ai_provider: String,
}

/// GET /api/v1/branches — returns all branches the caller can access.
/// Owners see all; Managers see their granted subset (already in JWT branch_ids).
pub async fn list_branches(
    AuthedOwner(claims): AuthedOwner,
    State(state): State<AppState>,
) -> Result<Json<Vec<BranchView>>, (StatusCode, String)> {
    // branch_ids is embedded in the JWT — filter to just those.
    let rows: Vec<(Uuid, String, String)> = if claims.is_owner() {
        sqlx::query_as(
            "SELECT id, name, ai_provider FROM branches ORDER BY created_at ASC",
        )
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?
    } else {
        // Manager: only granted branches.
        sqlx::query_as(
            "SELECT b.id, b.name, b.ai_provider \
             FROM branches b \
             JOIN user_branch_access uba ON uba.branch_id = b.id \
             WHERE uba.user_id = $1 \
             ORDER BY uba.granted_at ASC",
        )
        .bind(claims.sub)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?
    };

    Ok(Json(
        rows.into_iter()
            .map(|(id, name, ai_provider)| BranchView { id, name, ai_provider })
            .collect(),
    ))
}

// ── Create branch (Owner-only) ────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct CreateBranchRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CreateBranchResponse {
    pub id: Uuid,
}

pub async fn create_branch(
    AuthedOwnerOnly(claims): AuthedOwnerOnly,
    State(state): State<AppState>,
    Json(req): Json<CreateBranchRequest>,
) -> Result<(StatusCode, Json<CreateBranchResponse>), (StatusCode, String)> {
    let id = Uuid::new_v4();

    // T20: created_by_user_id (was owner_id).
    sqlx::query("INSERT INTO branches (id, created_by_user_id, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(claims.sub)
        .bind(&req.name)
        .execute(&state.pool)
        .await
        .map_err(internal)?;

    ReplyTemplateRepository::new(state.pool.clone())
        .seed_defaults(id)
        .await
        .map_err(internal)?;

    Ok((StatusCode::CREATED, Json(CreateBranchResponse { id })))
}

// ── T01: Set AI provider (Owner-only) ─────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct SetAiProviderRequest {
    pub provider: String,
}

#[derive(Debug, Serialize)]
pub struct SetAiProviderResponse {
    pub id: Uuid,
    pub ai_provider: String,
}

pub async fn set_ai_provider(
    AuthedOwnerOnly(claims): AuthedOwnerOnly,
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(req): Json<SetAiProviderRequest>,
) -> Result<Json<SetAiProviderResponse>, (StatusCode, String)> {
    let provider = match req.provider.as_str() {
        "claude" => AiProvider::Claude,
        "gemini" => AiProvider::Gemini,
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unknown provider '{other}': must be 'claude' or 'gemini'"),
            ))
        }
    };

    // Verify the branch is accessible to this Owner.
    let owns: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM branches WHERE id = $1",
    )
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    if owns.is_none() {
        return Err((StatusCode::NOT_FOUND, "branch not found".to_string()));
    }

    state
        .branch_config
        .set_ai_provider(branch_id, provider.as_sql())
        .await
        .map_err(internal)?;

    Ok(Json(SetAiProviderResponse {
        id: branch_id,
        ai_provider: provider.as_sql().to_string(),
    }))
}

// ── T20: Manager management (Owner-only) ──────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct CreateManagerRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct ManagerView {
    pub id: Uuid,
    pub email: String,
}

/// POST /api/v1/managers — Owner creates a Manager account.
pub async fn create_manager(
    AuthedOwnerOnly(_claims): AuthedOwnerOnly,
    State(state): State<AppState>,
    Json(req): Json<CreateManagerRequest>,
) -> Result<(StatusCode, Json<ManagerView>), (StatusCode, String)> {
    let name = req.email.trim().to_string();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "email required".to_string()));
    }
    let hash = bcrypt::hash(&req.password, 12)
        .map_err(|e| internal(e))?;

    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role) VALUES ($1, $2, $3, 'manager')",
    )
    .bind(id)
    .bind(&name)
    .bind(&hash)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        if matches!(&e, sqlx::Error::Database(db) if db.is_unique_violation()) {
            (StatusCode::CONFLICT, "email already in use".to_string())
        } else {
            internal(e)
        }
    })?;

    Ok((StatusCode::CREATED, Json(ManagerView { id, email: name })))
}

/// GET /api/v1/branches/:branch_id/managers — list Managers with access to this branch.
pub async fn list_branch_managers(
    AuthedOwnerOnly(_claims): AuthedOwnerOnly,
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<ManagerView>>, (StatusCode, String)> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT u.id, u.email \
         FROM users u \
         JOIN user_branch_access uba ON uba.user_id = u.id \
         WHERE uba.branch_id = $1 AND u.role = 'manager' \
         ORDER BY u.email ASC",
    )
    .bind(branch_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    Ok(Json(rows.into_iter().map(|(id, email)| ManagerView { id, email }).collect()))
}

#[derive(Debug, Deserialize)]
pub struct GrantAccessRequest {
    pub manager_id: Uuid,
}

/// POST /api/v1/branches/:branch_id/managers — grant a Manager access to this branch.
pub async fn grant_branch_access(
    AuthedOwnerOnly(claims): AuthedOwnerOnly,
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(req): Json<GrantAccessRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Verify target is a manager.
    let target: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM users WHERE id = $1",
    )
    .bind(req.manager_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    match target {
        None => return Err((StatusCode::NOT_FOUND, "user not found".to_string())),
        Some((role,)) if role != "manager" => {
            return Err((StatusCode::BAD_REQUEST, "target user is not a Manager".to_string()))
        }
        _ => {}
    }

    sqlx::query(
        "INSERT INTO user_branch_access (user_id, branch_id, granted_by) \
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(req.manager_id)
    .bind(branch_id)
    .bind(claims.sub)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/branches/:branch_id/managers/:manager_id — revoke access.
pub async fn revoke_branch_access(
    AuthedOwnerOnly(_claims): AuthedOwnerOnly,
    Path((branch_id, manager_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let result = sqlx::query(
        "DELETE FROM user_branch_access WHERE user_id = $1 AND branch_id = $2",
    )
    .bind(manager_id)
    .bind(branch_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    if result.rows_affected() > 0 {
        // Force the Manager to re-login so their JWT refreshes.
        let _ = sqlx::query(
            "UPDATE users SET token_version = token_version + 1 WHERE id = $1",
        )
        .bind(manager_id)
        .execute(&state.pool)
        .await;
    }

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "access record not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}
