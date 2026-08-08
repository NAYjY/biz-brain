//! D02/D03: Root `/` handler. Decodes cookie and redirects to the Owner's
//! first Branch orders page. No standalone "dashboard" view — Orders and
//! SupplyRequests are the dashboard (separate pages per D03).

use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use jsonwebtoken::{decode, DecodingKey, Validation};

use api::{extractors::Claims, AppState};

pub async fn render_dashboard(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    // No cookie -> login.
    let Some(token) = jar.get("auth").map(|c| c.value().to_string()) else {
        return Redirect::to("/login").into_response();
    };

    let secret = std::env::var("JWT_SECRET").unwrap_or_default();
    let Ok(data) = decode::<Claims>(&token, &DecodingKey::from_secret(secret.as_bytes()), &Validation::default()) else {
        return Redirect::to("/login").into_response();
    };

    let claims = data.claims;

    // First owned Branch -> orders. If none, need to create one first.
    if let Some(&branch_id) = claims.branch_ids.first() {
        Redirect::to(&format!("/branches/{branch_id}/orders")).into_response()
    } else {
        // Owner has no branches yet — show a minimal setup page.
        Redirect::to("/setup").into_response()
    }
}
