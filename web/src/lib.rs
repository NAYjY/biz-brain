//! Web crate (T06): Leptos SSR dashboard, plus T07's browser-facing SSE
//! relay. Returns `Router<AppState>` for the same reason `api::build_router`
//! does — the `server` crate merges both and calls `.with_state` once.

#![warn(clippy::all)]

pub mod routes;

use axum::{routing::get, Router};

use api::AppState;

pub fn build_router() -> Router<AppState> {
    Router::new()
        .route("/", get(routes::dashboard::render_dashboard))
        .route("/branches/:branch_id/events", get(routes::sse_relay::relay_branch_events))
}
