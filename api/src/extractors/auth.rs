//! T05 / S04: JWT in an httpOnly cookie.
//! Carries Owner identity + owned Branch ids.
//! S04: token_version claim checked against DB on every authenticated request.

use async_trait::async_trait;
use axum::{
    extract::{FromRef, FromRequestParts, Path},
    http::{request::Parts, StatusCode},
    RequestPartsExt,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub branch_ids: Vec<Uuid>,
    pub exp: usize,
    /// S04: must match owners.token_version in DB.
    pub token_version: i32,
}

impl Claims {
    pub fn owns(&self, branch_id: Uuid) -> bool {
        self.branch_ids.contains(&branch_id)
    }
}

/// Extractor: validates JWT sig + exp + token_version.
pub struct AuthedOwner(pub Claims);

#[async_trait]
impl<S> FromRequestParts<S> for AuthedOwner
where
    S: Send + Sync,
    PgPool: FromRef<S>,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let cookie_header = parts
            .headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "missing auth cookie"))?;

        let token = cookie_header
            .split(';')
            .map(str::trim)
            .find_map(|kv| kv.strip_prefix("auth="))
            .ok_or((StatusCode::UNAUTHORIZED, "missing auth cookie"))?;

        let secret = std::env::var("JWT_SECRET")
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "JWT_SECRET unset"))?;

        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid or expired token"))?;

        let claims = data.claims;

        // S04: token_version check.
        let pool = PgPool::from_ref(state);
        let row: Option<(i32,)> =
            sqlx::query_as("SELECT token_version FROM owners WHERE id = $1")
                .bind(claims.sub)
                .fetch_optional(&pool)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "DB error during auth"))?;

        let current = row
            .map(|(v,)| v)
            .ok_or((StatusCode::UNAUTHORIZED, "owner not found"))?;

        if claims.token_version != current {
            return Err((StatusCode::UNAUTHORIZED, "session revoked"));
        }

        Ok(AuthedOwner(claims))
    }
}

/// Per-request Branch-ownership check (T05: URL-scoped, not cookie-scoped).
pub struct AuthorizedBranch {
    pub branch_id: Uuid,
    pub claims: Claims,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthorizedBranch
where
    S: Send + Sync,
    PgPool: FromRef<S>,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthedOwner(claims) =
            parts.extract_with_state::<AuthedOwner, S>(state).await?;

        let Path(params) = parts
            .extract::<Path<std::collections::HashMap<String, String>>>()
            .await
            .map_err(|_| (StatusCode::BAD_REQUEST, "missing path params"))?;

        let branch_id: Uuid = params
            .get("branch_id")
            .ok_or((StatusCode::BAD_REQUEST, "missing branch_id in path"))?
            .parse()
            .map_err(|_| (StatusCode::BAD_REQUEST, "branch_id is not a valid UUID"))?;

        if !claims.owns(branch_id) {
            return Err((StatusCode::FORBIDDEN, "Branch not owned by authenticated Owner"));
        }

        Ok(AuthorizedBranch { branch_id, claims })
    }
}
