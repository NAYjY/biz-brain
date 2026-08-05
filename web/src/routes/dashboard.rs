//! T06: Leptos SSR. JWT stays server-side in the httpOnly cookie (checked by
//! `api`'s extractors when the browser calls `/api/v1/...`); this route only
//! renders the page shell — data fetching happens client-side against the
//! same-origin `/api/v1` routes, cookie attached automatically by the browser.

use axum::response::Html;
use leptos::*;

#[component]
fn DashboardShell() -> impl IntoView {
    view! {
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <title>"Biz-Brain"</title>
            </head>
            <body>
                <div id="app">"Biz-Brain Dashboard"</div>
                // Client-side JS (not built here) hydrates this shell, calling
                // /api/v1/branches/:id/orders etc. directly — same-origin, so
                // the httpOnly cookie rides along without any JS ever reading it.
            </body>
        </html>
    }
}

pub async fn render_dashboard() -> Html<String> {
    let html = leptos::ssr::render_to_string(DashboardShell);
    Html(format!("<!DOCTYPE html>{html}"))
}
