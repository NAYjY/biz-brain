//! T12: /branches — branch list + create form.
//!
//! GET  /branches — SSR page (list + create form)
//! POST /branches — create branch, reissue JWT with updated branch_ids, redirect into new branch
//!
//! The POST is handled here (not via /api/v1/branches JS fetch) so we can
//! reissue the httpOnly JWT cookie with the new branch_id included.
//! Without this the Owner would get a 403 immediately after creation because
//! the old JWT doesn't contain the new branch.

use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::Deserialize;
use time::Duration as CookieDuration;
use uuid::Uuid;

use api::extractors::Claims;
use api::AppState;
use store::ReplyTemplateRepository;

use crate::auth::decode_claims_from_jar;
use crate::templates::{html_escape, shell_close, shell_open};

const COOKIE_LIFETIME_DAYS: i64 = 7;

// ── GET /branches ─────────────────────────────────────────────────────────── //

pub async fn render_branches(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    let Some(claims) = decode_claims_from_jar(&jar) else {
        return Redirect::to("/login").into_response();
    };

    let rows: Vec<(Uuid, String)> = if claims.is_owner() {
        sqlx::query_as("SELECT id, name FROM branches ORDER BY created_at ASC")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
    } else {
        sqlx::query_as(
            "SELECT b.id, b.name \
             FROM branches b \
             JOIN user_branch_access uba ON uba.branch_id = b.id \
             WHERE uba.user_id = $1 \
             ORDER BY uba.granted_at ASC",
        )
        .bind(claims.sub)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default()
    };

    render_page(&rows, claims.is_owner(), None).into_response()
}

// ── POST /branches ────────────────────────────────────────────────────────── //

#[derive(Deserialize)]
pub struct CreateBranchForm {
    name: String,
}

pub async fn handle_create_branch(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<CreateBranchForm>,
) -> Response {
    let Some(claims) = decode_claims_from_jar(&jar) else {
        return Redirect::to("/login").into_response();
    };
    if !claims.is_owner() {
        return (axum::http::StatusCode::FORBIDDEN, "Owner role required").into_response();
    }

    let name = form.name.trim().to_string();
    if name.is_empty() {
        let rows = fetch_all_branches(&state, &claims).await;
        return render_page(&rows, true, Some("Branch name is required.")).into_response();
    }

    let branch_id = Uuid::new_v4();
    if let Err(e) = sqlx::query(
        "INSERT INTO branches (id, created_by_user_id, name) VALUES ($1, $2, $3)",
    )
    .bind(branch_id)
    .bind(claims.sub)
    .bind(&name)
    .execute(&state.pool)
    .await
    {
        let rows = fetch_all_branches(&state, &claims).await;
        return render_page(&rows, true, Some(&format!("Failed to create branch: {e}"))).into_response();
    }

    // Seed default reply templates.
    let _ = ReplyTemplateRepository::new(state.pool.clone())
        .seed_defaults(branch_id)
        .await;

    // Reissue JWT with the new branch_id appended — without this the redirect
    // into the new branch hits 403 because the old cookie lacks the new id.
    let mut new_branch_ids = claims.branch_ids.clone();
    new_branch_ids.push(branch_id);

    match issue_jwt_cookie(jar, &claims, new_branch_ids) {
        Ok(new_jar) => {
            (new_jar, Redirect::to(&format!("/branches/{branch_id}/orders"))).into_response()
        }
        Err(_) => {
            // Fallback: send to /branches; user can log out+in to pick up new branch.
            Redirect::to("/branches").into_response()
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────── //

async fn fetch_all_branches(state: &AppState, claims: &Claims) -> Vec<(Uuid, String)> {
    if claims.is_owner() {
        sqlx::query_as("SELECT id, name FROM branches ORDER BY created_at ASC")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
    } else {
        sqlx::query_as(
            "SELECT b.id, b.name FROM branches b \
             JOIN user_branch_access uba ON uba.branch_id = b.id \
             WHERE uba.user_id = $1 ORDER BY uba.granted_at ASC",
        )
        .bind(claims.sub)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default()
    }
}

fn issue_jwt_cookie(
    jar: CookieJar,
    claims: &Claims,
    branch_ids: Vec<Uuid>,
) -> Result<CookieJar, String> {
    let secret = std::env::var("JWT_SECRET").map_err(|_| "JWT_SECRET unset")?;

    let exp = (chrono::Utc::now() + chrono::Duration::days(COOKIE_LIFETIME_DAYS)).timestamp()
        as usize;

    let new_claims = Claims {
        sub: claims.sub,
        role: claims.role.clone(),
        branch_ids,
        exp,
        token_version: claims.token_version,
    };

    let token = encode(
        &Header::default(),
        &new_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| e.to_string())?;

    let cookie = Cookie::build(("auth", token))
        .path("/")
        .http_only(true)
        .secure(std::env::var("APP_ENV").as_deref() == Ok("production"))
        .same_site(SameSite::Lax)
        .max_age(CookieDuration::days(COOKIE_LIFETIME_DAYS))
        .build();

    Ok(jar.add(cookie))
}

// ── Page renderer ─────────────────────────────────────────────────────────── //

fn render_page(rows: &[(Uuid, String)], is_owner: bool, error: Option<&str>) -> Html<String> {
    let branch_list_html = if rows.is_empty() {
        if is_owner {
            r#"<p class="text-muted text-sm" style="padding:var(--space-2) 0;">
                 No branches yet — create your first one below.
               </p>"#
                .to_string()
        } else {
            r#"<p class="text-muted text-sm" style="padding:var(--space-2) 0;">
                 You have no branch access yet. Ask the Owner to grant you access.
               </p>"#
                .to_string()
        }
    } else {
        rows.iter()
            .map(|(id, name)| {
                format!(
                    r#"<a href="/branches/{id}/orders" class="branch-card">
  <span class="branch-card__name">{name}</span>
  <span class="branch-card__arrow">→</span>
</a>"#,
                    id = id,
                    name = html_escape(name),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let error_html = error.map_or(String::new(), |msg| {
        format!(
            r#"<div class="error-banner" style="margin-bottom:var(--space-4);">{}</div>"#,
            html_escape(msg)
        )
    });

    let create_section = if is_owner {
        format!(
            r#"<div class="card" style="padding:var(--space-6);margin-top:var(--space-6);">
  <h2 style="font-size:var(--text-base);font-weight:600;margin-bottom:var(--space-4);">
    Create a branch
  </h2>
  <p class="text-sm text-muted" style="margin-bottom:var(--space-4);">
    Each branch has its own Workers, Suppliers, Orders, and AI agent.
    Use branches for separate locations, franchises, or business units.
  </p>
  {error_html}
  <form method="POST" action="/branches"
        style="display:flex;gap:var(--space-3);align-items:flex-end;">
    <div class="form-group" style="flex:1;margin-bottom:0;">
      <label class="form-label" for="branch-name">Branch name</label>
      <input class="form-input" id="branch-name" name="name" type="text"
             placeholder="e.g. Bangkok North, Warehouse 2, Main Office"
             autocomplete="off" required autofocus>
    </div>
    <button class="btn btn--primary" type="submit">Create</button>
  </form>
</div>"#,
            error_html = error_html,
        )
    } else {
        String::new()
    };

    Html(format!(
        r#"{shell_open}
<header class="topbar">
  <a href="/branches" class="topbar__wordmark" style="text-decoration:none;">Biz<span>·</span>Brain</a>
  <div class="topbar__actions">
    <a href="/account/settings" class="btn btn--ghost btn--sm">Account</a>
    <form method="POST" action="/logout" style="margin:0;">
      <button class="btn btn--ghost btn--sm" type="submit">Sign out</button>
    </form>
  </div>
</header>

<div class="page" style="max-width:600px;">
  <div class="page-header"><h1>Branches</h1></div>

  <div class="card" style="padding:var(--space-6);">
    <div style="display:flex;flex-direction:column;gap:var(--space-3);">
      {branch_list_html}
    </div>
  </div>

  {create_section}
</div>

<style>
.branch-card {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-4);
  background: var(--color-surface-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  text-decoration: none;
  color: var(--color-text);
  transition: border-color .15s, background .15s;
}}
.branch-card:hover {{
  border-color: var(--color-accent);
  background: var(--color-surface);
  text-decoration: none;
}}
.branch-card__name {{ font-weight: 500; font-size: var(--text-base); }}
.branch-card__arrow {{ color: var(--color-accent); font-size: var(--text-lg); }}
</style>
{shell_close}"#,
        shell_open = shell_open("Branches — Biz-Brain"),
        branch_list_html = branch_list_html,
        create_section = create_section,
        shell_close = shell_close(),
    ))
}