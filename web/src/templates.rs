//! D03/D07: shared HTML shell fragments used by all SSR page handlers.
//! Keeps the page shell (head, topbar, static asset links) consistent.

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
    let orders_active = if active_page == "orders" { " active" } else { "" };
    let sr_active = if active_page == "supply-requests" { " active" } else { "" };
    let actors_active = if active_page == "actors" { " active" } else { "" };

    format!(
        r#"<header class="topbar">
  <span class="topbar__wordmark">Biz<span>·</span>Brain</span>
  <nav>
    <ul class="topbar__nav">
      <li><a href="/branches/{bid}/orders" class="{o}">Orders</a></li>
      <li><a href="/branches/{bid}/supply-requests" class="{sr}">Supply</a></li>
      <li><a href="/branches/{bid}/actors" class="{ac}">Workers</a></li>
    </ul>
  </nav>
  <div class="topbar__actions">
    <form method="POST" action="/logout" style="margin:0;">
      <button class="btn btn--ghost btn--sm" type="submit">Sign out</button>
    </form>
  </div>
</header>"#,
        bid = branch_id,
        o = orders_active,
        sr = sr_active,
        ac = actors_active,
    )
}

pub fn page_not_found() -> Response {
    Html("<h1>Not found</h1>").into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
