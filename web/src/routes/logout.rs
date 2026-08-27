//! S04 / T20: logout bumps token_version in `users` table (was `owners`),
//! invalidating all outstanding JWTs for this account — Owner or Manager.

use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;

use api::AppState;

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    if let Some(cookie) = jar.get("auth") {
        let secret = std::env::var("JWT_SECRET").unwrap_or_default();
        if let Ok(data) = jsonwebtoken::decode::<api::extractors::Claims>(
            cookie.value(),
            &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
            &jsonwebtoken::Validation::default(),
        ) {
            // T20: users table (was owners).
            let _ = sqlx::query(
                "UPDATE users SET token_version = token_version + 1 WHERE id = $1"
            )
            .bind(data.claims.sub)
            .execute(&state.pool)
            .await;
        }
    }

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

/// Helper used by login route when issuing a fresh JWT.
pub async fn current_token_version(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
) -> Result<i32, sqlx::Error> {
    // T20: users table.
    let row: (i32,) =
        sqlx::query_as("SELECT token_version FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}
