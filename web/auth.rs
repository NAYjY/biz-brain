//! D03: Branch-ownership auth check shared between `api`'s route extractors
//! and `web`'s SSR handlers. Both crates call this directly rather than
//! duplicating the cookie-decode + token_version + branch-ownership logic.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use jsonwebtoken::{decode, DecodingKey, Validation};
use sqlx::PgPool;
use uuid::Uuid;

use api::extractors::Claims;

pub enum BranchAuthOutcome {
    Authorized { claims: Claims, branch_id: Uuid },
    /// Redirect to login (no cookie / invalid token).
    Unauthenticated,
    /// Valid session but branch not owned by this Owner.
    Forbidden,
}

/// Validates cookie -> Claims -> token_version -> branch ownership.
/// Used by SSR handlers (web crate) to guard per-branch pages.
pub async fn authorize_branch(
    jar: &CookieJar,
    pool: &PgPool,
    branch_id: Uuid,
) -> BranchAuthOutcome {
    let Some(claims) = decode_claims(jar) else {
        return BranchAuthOutcome::Unauthenticated;
    };

    if !token_version_valid(pool, &claims).await {
        return BranchAuthOutcome::Unauthenticated;
    }

    if !claims.owns(branch_id) {
        return BranchAuthOutcome::Forbidden;
    }

    BranchAuthOutcome::Authorized { claims, branch_id }
}

fn decode_claims(jar: &CookieJar) -> Option<Claims> {
    let token = jar.get("auth")?.value().to_string();
    let secret = std::env::var("JWT_SECRET").ok()?;
    decode::<Claims>(&token, &DecodingKey::from_secret(secret.as_bytes()), &Validation::default())
        .ok()
        .map(|d| d.claims)
}

async fn token_version_valid(pool: &PgPool, claims: &Claims) -> bool {
    let Ok(row) = sqlx::query_as::<_, (i32,)>(
        "SELECT token_version FROM owners WHERE id = $1"
    )
    .bind(claims.sub)
    .fetch_optional(pool)
    .await
    else {
        return false;
    };

    row.map_or(false, |(v,)| v == claims.token_version)
}

/// Converts a BranchAuthOutcome into an error Response when not Authorized.
/// Returns None when authorized (caller proceeds).
pub fn auth_error_response(outcome: BranchAuthOutcome) -> Option<Response> {
    match outcome {
        BranchAuthOutcome::Authorized { .. } => None,
        BranchAuthOutcome::Unauthenticated => Some(Redirect::to("/login").into_response()),
        BranchAuthOutcome::Forbidden => Some(StatusCode::FORBIDDEN.into_response()),
    }
}
