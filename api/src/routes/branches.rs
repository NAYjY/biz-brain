//! D02 / P01 / T01: Branch management.
//! POST  /api/v1/branches         — create branch, seed reply templates.
//! GET   /api/v1/branches         — list owned branches.
//! PATCH /api/v1/branches/:id/ai-provider — switch AI provider (claude | gemini).

use axum::{extract::State, http::StatusCode, Json};
use axum::extract::Path;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use agent::AiProvider;
use store::ReplyTemplateRepository;

use crate::{extractors::AuthedOwner, state::AppState};

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

pub async fn list_branches(
    AuthedOwner(claims): AuthedOwner,
    State(state): State<AppState>,
) -> Result<Json<Vec<BranchView>>, (StatusCode, String)> {
    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, name, ai_provider FROM branches WHERE owner_id = $1 ORDER BY created_at ASC",
    )
    .bind(claims.sub)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, name, ai_provider)| BranchView { id, name, ai_provider })
            .collect(),
    ))
}

// ── Create branch ─────────────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct CreateBranchRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CreateBranchResponse {
    pub id: Uuid,
}

pub async fn create_branch(
    AuthedOwner(claims): AuthedOwner,
    State(state): State<AppState>,
    Json(req): Json<CreateBranchRequest>,
) -> Result<(StatusCode, Json<CreateBranchResponse>), (StatusCode, String)> {
    let id = Uuid::new_v4();

    sqlx::query("INSERT INTO branches (id, owner_id, name) VALUES ($1, $2, $3)")
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

// ── T01: Set AI provider ──────────────────────────────────────────────────── //

#[derive(Debug, Deserialize)]
pub struct SetAiProviderRequest {
    /// "claude" or "gemini"
    pub provider: String,
}

#[derive(Debug, Serialize)]
pub struct SetAiProviderResponse {
    pub id: Uuid,
    pub ai_provider: String,
}

/// PATCH /api/v1/branches/:branch_id/ai-provider
///
/// Owner selects which AI backend classifies messages for this branch.
/// Takes effect on the next inbound message — no restart required.
pub async fn set_ai_provider(
    AuthedOwner(claims): AuthedOwner,
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(req): Json<SetAiProviderRequest>,
) -> Result<Json<SetAiProviderResponse>, (StatusCode, String)> {
    // Validate provider value.
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

    // Verify ownership.
    let owns: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM branches WHERE id = $1 AND owner_id = $2",
    )
    .bind(branch_id)
    .bind(claims.sub)
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