//! D01: Owner login — form POST, JWT issuance, httpOnly cookie.
//! Lives in `web` (not `api`): web owns all cookie/browser-facing flows.
//!
//! Cookie: httpOnly + Secure + SameSite=Lax, 7-day sliding window.
//! Cookie renewal on every authenticated request where remaining life < 6 days
//! is handled separately in `api::extractors::auth` (T05/D01 resolution).

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::Deserialize;
use time::Duration;

use api::{extractors::Claims, AppState};
use crate::routes::logout::current_token_version;

const COOKIE_LIFETIME_DAYS: i64 = 7;

pub async fn render_login(jar: CookieJar) -> Response {
    // Already logged in? Bounce to dashboard.
    if jar.get("auth").is_some() {
        return Redirect::to("/").into_response();
    }
    login_page_html(None).into_response()
}

#[derive(Deserialize)]
pub struct LoginForm {
    email: String,
    password: String,
}

pub async fn handle_login(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> Response {
    match authenticate(&state, &form).await {
        Ok((jar, redirect)) => (jar, redirect).into_response(),
        Err(msg) => (StatusCode::UNAUTHORIZED, login_page_html(Some(&msg))).into_response(),
    }
}

async fn authenticate(
    state: &AppState,
    form: &LoginForm,
) -> Result<(CookieJar, Redirect), String> {
    let row: Option<(uuid::Uuid, String, Vec<uuid::Uuid>, i32)> = sqlx::query_as(
        "SELECT o.id, o.password_hash,
                COALESCE(ARRAY_AGG(b.id) FILTER (WHERE b.id IS NOT NULL), '{}'),
                o.token_version
         FROM owners o
         LEFT JOIN branches b ON b.owner_id = o.id
         WHERE o.email = $1
         GROUP BY o.id",
    )
    .bind(&form.email)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("DB error: {e}"))?;

    let (owner_id, hash, branch_ids, token_version) =
        row.ok_or_else(|| "Invalid email or password.".to_string())?;

    let valid = bcrypt::verify(&form.password, &hash)
        .map_err(|_| "Invalid email or password.".to_string())?;

    if !valid {
        return Err("Invalid email or password.".to_string());
    }

    let jar = issue_jwt_cookie(jar_from_jar(), owner_id, branch_ids, token_version)?;
    Ok((jar, Redirect::to("/")))
}

fn jar_from_jar() -> CookieJar {
    CookieJar::new()
}

fn issue_jwt_cookie(
    jar: CookieJar,
    owner_id: uuid::Uuid,
    branch_ids: Vec<uuid::Uuid>,
    token_version: i32,
) -> Result<CookieJar, String> {
    let secret = std::env::var("JWT_SECRET").map_err(|_| "JWT_SECRET unset".to_string())?;

    let exp = (chrono::Utc::now() + chrono::Duration::days(COOKIE_LIFETIME_DAYS)).timestamp() as usize;

    let claims = Claims {
        sub: owner_id,
        branch_ids,
        exp,
        token_version,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| format!("JWT encode error: {e}"))?;

    let cookie = Cookie::build(("auth", token))
        .path("/")
        .http_only(true)
        .secure(std::env::var("APP_ENV").as_deref() == Ok("production")) // only Secure in prod
        .same_site(SameSite::Lax)
        .max_age(Duration::days(COOKIE_LIFETIME_DAYS))
        .build();

    Ok(jar.add(cookie))
}

/// Re-issues a fresh JWT cookie for sliding window renewal.
/// Called by auth extractor when remaining life < 6 days (D01 resolution).
pub fn renew_cookie(
    jar: CookieJar,
    claims: &Claims,
) -> Result<CookieJar, String> {
    issue_jwt_cookie(jar, claims.sub, claims.branch_ids.clone(), claims.token_version)
}

fn login_page_html(error: Option<&str>) -> Html<String> {
    let error_html = error.map_or(String::new(), |msg| {
        format!(r#"<div class="error-banner">{}</div>"#, html_escape(msg))
    });

    Html(format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Sign in — Biz-Brain</title>
  <link rel="stylesheet" href="/static/css/base.css">
</head>
<body>
<div class="login-page">
  <div class="login-card">
    <div class="login-card__wordmark">Biz<span>·</span>Brain</div>
    {error_html}
    <form class="login-form" method="POST" action="/login">
      <div class="form-group">
        <label class="form-label" for="email">Email</label>
        <input class="form-input" id="email" name="email" type="email"
               autocomplete="email" required autofocus>
      </div>
      <div class="form-group">
        <label class="form-label" for="password">Password</label>
        <input class="form-input" id="password" name="password" type="password"
               autocomplete="current-password" required>
      </div>
      <button class="btn btn--primary" type="submit">Sign in</button>
    </form>
  </div>
</div>
</body>
</html>"#))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
