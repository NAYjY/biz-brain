//! D03/D07: shared HTML shell fragments used by all SSR page handlers.
//! F04: adds f04_thread.css to shell_open.
//! F01: adds f01_order_tag.css to shell_open.
//! T12/T13: topbar_html gains branch switcher dropdown + Account link.
//!           load_topbar_data() fetches branch name + list for the switcher.
//! T10: Suppliers nav item added.

use axum::response::{Html, IntoResponse, Response};
use sqlx::PgPool;
use uuid::Uuid;

use api::extractors::Claims;

pub fn shell_open(title: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <link rel="stylesheet" href="/static/css/base.css">
  <link rel="stylesheet" href="/static/css/f04_thread.css">
  <link rel="stylesheet" href="/static/css/f01_order_tag.css">
  <link rel="stylesheet" href="/static/css/f05_dates_alerts.css">
</head>
<body>
<div class="app-layout">"#,
        title = html_escape(title)
    )
}

pub fn shell_close() -> &'static str {
    "</div></body></html>"
}

/// Load the current branch name and full branch list for the topbar switcher.
/// Should be called once per SSR page handler.
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

/// Render the shared topbar with branch switcher and account link.
///
/// `branch_id`    — current branch (used for nav links)
/// `branch_name`  — display name shown in the switcher
/// `all_branches` — full list of accessible branches (id, name)
/// `active_page`  — which nav link to highlight ("orders", "supply-requests",
///                  "workers", "suppliers", "actors")
pub fn topbar_html(
    branch_id: Uuid,
    branch_name: &str,
    all_branches: &[(Uuid, String)],
    active_page: &str,
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

    // Branch switcher — dropdown when multiple branches, plain label when one.
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
    <a href="/account/settings" class="btn btn--ghost btn--sm">Account</a>
    <form method="POST" action="/logout" style="margin:0;">
      <button class="btn btn--ghost btn--sm" type="submit">Sign out</button>
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
</style>"#,
        switcher_html = switcher_html,
        orders    = nav_item("/orders",          "Orders",           "orders"),
        supply    = nav_item("/supply-requests", "Supply",           "supply-requests"),
        workers   = nav_item("/workers",         "Workers",          "workers"),
        suppliers = nav_item("/suppliers",       "Suppliers",        "suppliers"),
        actors    = nav_item("/actors",          "Pending Bindings", "actors"),
    )
}

pub fn page_not_found() -> Response {
    Html("<h1>Not found</h1>").into_response()
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}