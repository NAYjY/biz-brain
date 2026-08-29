//! D01 / T20 / T13: Owner/Manager login — form POST, JWT issuance, httpOnly cookie.
//! T20: queries `users` table (replaces `owners`). JWT now carries `role`.
//! T13 fix: render_login validates token_version before redirecting away,
//!           preventing redirect loops after password change.

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

const COOKIE_LIFETIME_DAYS: i64 = 7;

pub async fn render_login(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    // Only skip the login page if the cookie is present AND the token_version
    // is still valid in the DB. A stale cookie (after password change / logout)
    // must show the login page, not redirect — that's what causes the loop.
    if is_session_valid(&state, &jar).await {
        return Redirect::to("/").into_response();
    }
    login_page_html(None).into_response()
}

/// Returns true only when the cookie exists, the JWT is valid, AND the
/// token_version in the DB still matches. Any failure → false (show login).
async fn is_session_valid(state: &AppState, jar: &CookieJar) -> bool {
    use jsonwebtoken::{decode, DecodingKey, Validation};

    let Some(token) = jar.get("auth").map(|c| c.value().to_string()) else {
        return false;
    };

    let secret = std::env::var("JWT_SECRET").unwrap_or_default();
    let Ok(data) = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    ) else {
        return false;
    };

    let claims = data.claims;

    // DB check — must match current token_version.
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT token_version FROM users WHERE id = $1")
            .bind(claims.sub)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    row.map_or(false, |(v,)| v == claims.token_version)
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
    let row: Option<(uuid::Uuid, String, String, i32)> = sqlx::query_as(
        "SELECT id, password_hash, role, token_version \
         FROM users \
         WHERE email = $1",
    )
    .bind(&form.email)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("DB error: {e}"))?;

    let (user_id, hash, role, token_version) =
        row.ok_or_else(|| "Invalid email or password.".to_string())?;

    let valid = bcrypt::verify(&form.password, &hash)
        .map_err(|_| "Invalid email or password.".to_string())?;

    if !valid {
        return Err("Invalid email or password.".to_string());
    }

    // Populate branch_ids based on role.
    let branch_ids: Vec<uuid::Uuid> = if role == "owner" {
        sqlx::query_scalar("SELECT id FROM branches ORDER BY created_at ASC")
            .fetch_all(&state.pool)
            .await
            .map_err(|e| format!("DB error: {e}"))?
    } else {
        sqlx::query_scalar(
            "SELECT branch_id FROM user_branch_access \
             WHERE user_id = $1 ORDER BY granted_at ASC",
        )
        .bind(user_id)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| format!("DB error: {e}"))?
    };

    let jar = issue_jwt_cookie(CookieJar::new(), user_id, role, branch_ids, token_version)?;

    Ok((jar, Redirect::to("/")))
}

fn issue_jwt_cookie(
    jar: CookieJar,
    user_id: uuid::Uuid,
    role: String,
    branch_ids: Vec<uuid::Uuid>,
    token_version: i32,
) -> Result<CookieJar, String> {
    let secret = std::env::var("JWT_SECRET").map_err(|_| "JWT_SECRET unset".to_string())?;

    let exp = (chrono::Utc::now() + chrono::Duration::days(COOKIE_LIFETIME_DAYS)).timestamp()
        as usize;

    let claims = Claims {
        sub: user_id,
        role,
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
        .secure(std::env::var("APP_ENV").as_deref() == Ok("production"))
        .same_site(SameSite::Lax)
        .max_age(Duration::days(COOKIE_LIFETIME_DAYS))
        .build();

    Ok(jar.add(cookie))
}

fn login_page_html(error: Option<&str>) -> Html<String> {
    let error_html = error.map_or(String::new(), |msg| {
        format!(r#"<div class="error-banner">{}</div>"#, html_escape(msg))
    });

    Html(format!(
        r#"<!DOCTYPE html>
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
</html>"#
    ))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}