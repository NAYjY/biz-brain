//! T13: Account settings API.
//! POST /api/v1/account/change-password — Owner or Manager changes their own password.
//! Bumps token_version after success, invalidating all other active sessions.

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;

use crate::{extractors::AuthedOwner, state::AppState};

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// POST /api/v1/account/change-password
pub async fn change_password(
    AuthedOwner(claims): AuthedOwner,
    State(state): State<AppState>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if req.new_password.len() < 8 {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "New password must be at least 8 characters.".to_string(),
        ));
    }

    // Fetch current password hash.
    let row: Option<(String,)> =
        sqlx::query_as("SELECT password_hash FROM users WHERE id = $1")
            .bind(claims.sub)
            .fetch_optional(&state.pool)
            .await
            .map_err(internal)?;

    let (current_hash,) =
        row.ok_or_else(|| (StatusCode::NOT_FOUND, "User not found.".to_string()))?;

    // Verify current password.
    let valid = bcrypt::verify(&req.current_password, &current_hash)
        .map_err(|e| internal(e))?;

    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Current password is incorrect.".to_string(),
        ));
    }

    // Hash new password and save, bumping token_version to sign out all other sessions.
    let new_hash = bcrypt::hash(&req.new_password, 12).map_err(|e| internal(e))?;

    sqlx::query(
        "UPDATE users \
         SET password_hash = $1, token_version = token_version + 1 \
         WHERE id = $2",
    )
    .bind(&new_hash)
    .bind(claims.sub)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    Ok(StatusCode::NO_CONTENT)
}
