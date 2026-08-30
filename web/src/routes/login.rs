//! D01 / T20 / T11 / T13: Owner/Manager login.
//! T11: login page rendered in the locale detected from Accept-Language header.
//!      No cookie exists yet so browser language is the only signal.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
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
use crate::i18n::{locale_from_request, Translations};

const COOKIE_LIFETIME_DAYS: i64 = 7;

pub async fn render_login(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Response {
    if is_session_valid(&state, &jar).await {
        return Redirect::to("/").into_response();
    }
    let t = locale_from_request(&jar, &headers);
    login_page_html(&t, None).into_response()
}

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
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let t = locale_from_request(&jar, &headers);
    match authenticate(&state, &form).await {
        Ok((jar, redirect)) => (jar, redirect).into_response(),
        Err(msg) => (StatusCode::UNAUTHORIZED, login_page_html(&t, Some(&msg))).into_response(),
    }
}

async fn authenticate(
    state: &AppState,
    form: &LoginForm,
) -> Result<(CookieJar, Redirect), String> {
    let row: Option<(uuid::Uuid, String, String, i32)> = sqlx::query_as(
        "SELECT id, password_hash, role, token_version FROM users WHERE email = $1",
    )
    .bind(&form.email)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("DB error: {e}"))?;

    let (user_id, hash, role, token_version) =
        row.ok_or_else(|| "invalid".to_string())?;

    let valid = bcrypt::verify(&form.password, &hash)
        .map_err(|_| "invalid".to_string())?;
    if !valid {
        return Err("invalid".to_string());
    }

    let branch_ids: Vec<uuid::Uuid> = if role == "owner" {
        sqlx::query_scalar("SELECT id FROM branches ORDER BY created_at ASC")
            .fetch_all(&state.pool)
            .await
            .map_err(|e| format!("DB error: {e}"))?
    } else {
        sqlx::query_scalar(
            "SELECT branch_id FROM user_branch_access WHERE user_id = $1 ORDER BY granted_at ASC",
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
    let exp = (chrono::Utc::now() + chrono::Duration::days(COOKIE_LIFETIME_DAYS)).timestamp() as usize;
    let claims = Claims { sub: user_id, role, branch_ids, exp, token_version };
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

fn login_page_html(t: &Translations, error: Option<&str>) -> Html<String> {
    // Map the sentinel "invalid" to the translated string
    let error_msg = error.map(|e| {
        if e == "invalid" { t.get("login.error.invalid") } else { e }
    });

    let error_html = error_msg.map_or(String::new(), |msg| {
        format!(r#"<div class="error-banner">{}</div>"#, html_escape(msg))
    });

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="{lang}">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <link rel="stylesheet" href="/static/css/base.css">
</head>
<body>
<div class="login-page">
  <div class="login-card">
    <div class="login-card__wordmark">Biz<span>·</span>Brain</div>
    {error_html}
    <form class="login-form" method="POST" action="/login">
      <div class="form-group">
        <label class="form-label" for="email">{email_label}</label>
        <input class="form-input" id="email" name="email" type="email"
               autocomplete="email" required autofocus>
      </div>
      <div class="form-group">
        <label class="form-label" for="password">{pw_label}</label>
        <input class="form-input" id="password" name="password" type="password"
               autocomplete="current-password" required>
      </div>
      <button class="btn btn--primary" type="submit">{sign_in}</button>
    </form>
  </div>
</div>
</body>
</html>"#,
        lang       = t.lang(),
        title      = t.get("login.title"),
        email_label= t.get("login.email"),
        pw_label   = t.get("login.password"),
        sign_in    = t.get("btn.sign_in"),
        error_html = error_html,
    ))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
