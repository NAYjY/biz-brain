//! D03/D07: shared HTML shell fragments used by all SSR page handlers.

use axum::response::{Html, IntoResponse, Response};
use uuid::Uuid;

pub fn shell_open(title: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <link rel="stylesheet" href="/static/css/base.css">
</head>
<body>
<div class="app-layout">"#,
        title = html_escape(title)
    )
}

pub fn shell_close() -> &'static str {
    "</div></body></html>"
}

pub fn topbar_html(branch_id: Uuid, active_page: &str) -> String {
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

    format!(
        r#"<header class="topbar">
  <span class="topbar__wordmark">Biz<span>·</span>Brain</span>
  <nav>
    <ul class="topbar__nav">
      {orders}
      {supply}
      {workers}
      {actors}
    </ul>
  </nav>
  <div class="topbar__actions">
    <form method="POST" action="/logout" style="margin:0;">
      <button class="btn btn--ghost btn--sm" type="submit">Sign out</button>
    </form>
  </div>
</header>"#,
        orders  = nav_item("/orders",          "Orders",           "orders"),
        supply  = nav_item("/supply-requests", "Supply",           "supply-requests"),
        workers = nav_item("/workers",         "Workers",          "workers"),
        actors  = nav_item("/actors",          "Pending Bindings", "actors"),
    )
}

pub fn page_not_found() -> Response {
    Html("<h1>Not found</h1>").into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}