//! S04: logout bumps token_version in DB, invalidating all outstanding JWTs
//! for this Owner — not just the current browser's cookie. Any tab/device
//! holding a prior token gets a 401 on next authenticated request.
//!
//! Lives in `web`, not `api` — consistent with D01/T06: `web` owns all
//! cookie/browser-facing flows; `api` stays pure JSON and never issues or
//! clears cookies.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;

use api::AppState;

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    // Extract owner id from cookie — best-effort; if token is already invalid
    // we still clear the cookie.
    if let Some(cookie) = jar.get("auth") {
        let secret = std::env::var("JWT_SECRET").unwrap_or_default();
        if let Ok(data) = jsonwebtoken::decode::<api::extractors::Claims>(
            cookie.value(),
            &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
            &jsonwebtoken::Validation::default(),
        ) {
            // S04: bump token_version -> all prior JWTs for this owner now stale.
            let _ = sqlx::query(
                "UPDATE owners SET token_version = token_version + 1 WHERE id = $1"
            )
            .bind(data.claims.sub)
            .execute(&state.pool)
            .await;
        }
    }

    // Clear the cookie regardless of whether we could decode it.
    let cleared = jar.remove(
        axum_extra::extract::cookie::Cookie::build("auth")
            .path("/")
            .http_only(true)
            .secure(true)
            .same_site(axum_extra::extract::cookie::SameSite::Lax)
            .max_age(time::Duration::ZERO)
            .build(),
    );

    (cleared, Redirect::to("/login")).into_response()
}

/// Helper used by the login route (web crate) when issuing a fresh JWT.
/// Fetches the current token_version so it can be embedded in the new claim.
pub async fn current_token_version(
    pool: &sqlx::PgPool,
    owner_id: uuid::Uuid,
) -> Result<i32, sqlx::Error> {
    let row: (i32,) =
        sqlx::query_as("SELECT token_version FROM owners WHERE id = $1")
            .bind(owner_id)
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}