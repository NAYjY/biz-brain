//! Web crate (T06 / D01-D07 / T20 / T13): SSR dashboard.
//! T20: Admin routes added (/admin/login, /admin/owners).
//! T13: /account/settings and /branches routes added.
//!      /setup redirect removed — zero-branch state handled by /branches (T12).

#![warn(clippy::all)]

pub mod auth;
pub mod routes;
pub mod templates;

use axum::{
    routing::{get, post},
    Router,
};

use api::AppState;

pub fn build_router() -> Router<AppState> {
    Router::new()
        // D01: login / logout
        .route("/login", get(routes::login::render_login).post(routes::login::handle_login))
        .route("/logout", post(routes::logout::logout))
        // D02: root redirects to first branch (or /branches if none)
        .route("/", get(routes::dashboard::render_dashboard))
        // T12/T13: branch list + create (zero-branch landing)
        .route("/branches", get(routes::branches::render_branches))
        // T13: account settings (change password)
        .route("/account/settings", get(routes::account::render_account_settings))
        // D04: Orders page
        .route("/branches/:branch_id/orders", get(routes::orders::render_orders))
        // D05: Supply Requests page
        .route(
            "/branches/:branch_id/supply-requests",
            get(routes::supply_requests::render_supply_requests),
        )
        // Worker onboarding page
        .route("/branches/:branch_id/workers", get(routes::workers::render_workers))
        // D08-5: Actors (pending bindings) page
        .route("/branches/:branch_id/actors", get(routes::actors::render_actors))
        // T07: browser-facing SSE relay
        .route("/branches/:branch_id/events", get(routes::sse_relay::relay_branch_events))
}