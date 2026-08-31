//! D03/D07/T11: shared HTML shell fragments used by all SSR page handlers.
//! T11: all chrome strings now go through `&Translations` — never hardcoded.

use axum::response::{Html, IntoResponse, Response};
use axum::http::HeaderMap;
use axum_extra::extract::CookieJar;
use sqlx::PgPool;
use uuid::Uuid;

use api::extractors::Claims;
use crate::i18n::{locale_from_request, locale_switcher_html, Translations};

pub fn shell_open(title: &str, lang: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="{lang}">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <link rel="stylesheet" href="/static/css/base.css">
  <link rel="stylesheet" href="/static/css/f04_thread.css">
  <link rel="stylesheet" href="/static/css/f01_order_tag.css">
  <link rel="stylesheet" href="/static/css/f05_dates_alerts.css">
  <link rel="stylesheet" href="/static/css/mobile-cards.css">
</head>
<body>
<div class="app-layout">"#,
        title = html_escape(title),
        lang = lang,
    )
}

pub fn shell_close() -> &'static str {
    "</div></body></html>"
}

/// Resolve translations for a request. Call once at the top of every handler.
pub fn translations_for(jar: &CookieJar, headers: &HeaderMap) -> Translations {
    locale_from_request(jar, headers)
}

/// Load the current branch name and full branch list for the topbar switcher.
pub async fn load_topbar_data(
    pool: &PgPool,
    branch_id: Uuid,
    claims: &Claims,
) -> (String, Vec<(Uuid, String)>) {
    let name: String = sqlx::query_scalar("SELECT name FROM branches WHERE id = $1")
        .bind(branch_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "Branch".to_string());

    let branches: Vec<(Uuid, String)> = if claims.is_owner() {
        sqlx::query_as("SELECT id, name FROM branches ORDER BY created_at ASC")
            .fetch_all(pool)
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
        .fetch_all(pool)
        .await
        .unwrap_or_default()
    };

    (name, branches)
}

/// Render the shared topbar with branch switcher, nav, account link, and locale switcher.
pub fn topbar_html(
    branch_id: Uuid,
    branch_name: &str,
    all_branches: &[(Uuid, String)],
    active_page: &str,
    t: &Translations,
    current_path: &str,
) -> String {
    let nav_item = |href: &str, label: &str, page: &str| {
        let active = if active_page == page { " active" } else { "" };
        format!(
            r#"<li><a href="/branches/{bid}{href}" class="{active}">{label}</a></li>"#,
            bid = branch_id,
            href = href,
            label = label,
            active = active,
        )
    };

    let switcher_html = if all_branches.len() <= 1 {
        format!(
            r#"<span class="branch-switcher__name">{}</span>"#,
            html_escape(branch_name)
        )
    } else {
        let options: String = all_branches
            .iter()
            .map(|(id, name)| {
                let selected = if *id == branch_id { " selected" } else { "" };
                format!(
                    r#"<option value="{id}"{selected}>{name}</option>"#,
                    id = id,
                    name = html_escape(name),
                    selected = selected,
                )
            })
            .collect();

        format!(
            r#"<div class="branch-switcher">
  <select class="branch-switcher__select"
          onchange="window.location='/branches/'+this.value+'/orders'">
    {options}
  </select>
</div>"#
        )
    };

    let locale_switcher = locale_switcher_html(t.locale, current_path);

    format!(
        r#"<header class="topbar">
  <a href="/branches" class="topbar__wordmark" style="text-decoration:none;">Biz<span>·</span>Brain</a>
  {switcher_html}
  <nav>
    <ul class="topbar__nav">
      {orders}
      {supply}
      {workers}
      {suppliers}
      {actors}
    </ul>
  </nav>
  <div class="topbar__actions">
    {locale_switcher}
    <a href="/account/settings" class="btn btn--ghost btn--sm">{account}</a>
    <form method="POST" action="/logout" style="margin:0;">
      <button class="btn btn--ghost btn--sm" type="submit">{sign_out}</button>
    </form>
  </div>
</header>

<style>
.branch-switcher {{
  display: flex;
  align-items: center;
}}
.branch-switcher__select {{
  background: var(--color-surface-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text);
  font-family: var(--font-body);
  font-size: var(--text-sm);
  font-weight: 500;
  padding: var(--space-1) var(--space-3);
  cursor: pointer;
  transition: border-color .15s;
}}
.branch-switcher__select:focus {{
  outline: none;
  border-color: var(--color-accent);
}}
.branch-switcher__name {{
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--color-text-muted);
  padding: var(--space-1) var(--space-2);
}}
</style>
<script>
        // T16-09: scroll active nav link into view on mobile load
        (function () {{
          var active = document.querySelector('.topbar__nav a.active');
          if (active) {{
            active.scrollIntoView({{ inline: 'center', block: 'nearest' }});
          }}
        }})();
      </script>"#,
        switcher_html = switcher_html,
        locale_switcher = locale_switcher,
        orders    = nav_item("/orders",          t.get("nav.orders"),           "orders"),
        supply    = nav_item("/supply-requests", t.get("nav.supply"),           "supply-requests"),
        workers   = nav_item("/workers",         t.get("nav.workers"),          "workers"),
        suppliers = nav_item("/suppliers",       t.get("nav.suppliers"),        "suppliers"),
        actors    = nav_item("/actors",          t.get("nav.pending_bindings"), "actors"),
        account   = t.get("nav.account"),
        sign_out  = t.get("nav.sign_out"),
    )
}

pub fn page_not_found() -> Response {
    Html("<h1>Not found</h1>").into_response()
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
