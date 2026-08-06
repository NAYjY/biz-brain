//! T05: JWT in an httpOnly cookie. Carries Owner identity + owned Branch ids;
//! no "active Branch" claim — Branch selection happens per-request via the
//! URL, not session state (an Owner may have several Branches open in
//! different tabs).

use async_trait::async_trait;
use axum::{
    extract::{FromRequestParts, Path},
    http::{request::Parts, StatusCode},
    RequestPartsExt,
};
use jsonwebtoken::{Algorithm, decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,            // Owner id
    pub branch_ids: Vec<Uuid>, // owned Branches
    pub exp: usize,
}

impl Claims {
    pub fn owns(&self, branch_id: Uuid) -> bool {
        self.branch_ids.contains(&branch_id)
    }
}

pub struct AuthedOwner(pub Claims);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AuthedOwner {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
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

        let secret = std::env::var("JWT_SECRET").map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "JWT_SECRET unset"))?;

        let mut validation = Validation::new(Algorithm::HS256);
        // S02: Enforce signature checking on all requests;
        // do not relax these conditions — they are the trust boundary.
        validation.leeway = 0; // No clock skew tolerance for security-sensitive tokens
        // By default, JWT requires nbf and exp claims if present,
        // so we don't need to explicitly enable those checks here.
        // Do NOT set iat_required = true — that would cause valid tokens
        // to be rejected when a future is pushed to the database and then
        // fetched again (see R03 in CODE_REVIEW_REPORT.md).
        let data = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation)
            .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid or expired token"))?;

        Ok(AuthedOwner(data.claims))
    }
}

/// Per-request Branch-ownership check (T05: URL-scoped, not cookie-scoped).
/// Use as an extractor alongside `Path<Uuid>` for `:branch_id`.
pub struct AuthorizedBranch {
    pub branch_id: Uuid,
    pub claims: Claims,
}

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AuthorizedBranch {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthedOwner(claims) = parts.extract_with_state::<AuthedOwner, S>(state).await?;

        // Works for any route shape (`/branches/:branch_id/...`, with or
        // without further path segments after it) rather than assuming
        // `:branch_id` is the only path param.
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
