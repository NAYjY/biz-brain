//! Web crate (T06 / D01-D07 / T11 / T12 / T13 / T20 / T10 / T21): SSR dashboard.
//! T11: i18n module + /locale switcher endpoint.
//! T12: /branches route added (branch list + create).
//! T13: /account/settings added.
//! T10: /branches/:branch_id/suppliers added.
//! T21: /branches/:branch_id/settings added (Owner-only).

#![warn(clippy::all)]

pub mod auth;
pub mod i18n;
pub mod routes;
pub mod templates;

use axum::{
    routing::{get, post},
    Router,
};

use api::AppState;

pub fn build_router() -> Router<AppState> {
    Router::new()
        // Auth
        .route("/login",  get(routes::login::render_login).post(routes::login::handle_login))
        .route("/logout", post(routes::logout::logout))
        // T11: locale switcher
        .route("/locale", post(routes::locale::set_locale))
        // Root → first branch or /branches
        .route("/", get(routes::dashboard::render_dashboard))
        // T12: branch list (GET) + create with JWT reissue (POST)
        .route(
            "/branches",
            get(routes::branches::render_branches)
                .post(routes::branches::handle_create_branch),
        )
        // T13: account settings
        .route("/account/settings", get(routes::account::render_account_settings))
        // Branch-scoped pages
        .route("/branches/:branch_id/orders",          get(routes::orders::render_orders))
        .route("/branches/:branch_id/supply-requests", get(routes::supply_requests::render_supply_requests))
        .route("/branches/:branch_id/workers",         get(routes::workers::render_workers))
        // T10: Suppliers page
        .route("/branches/:branch_id/suppliers",       get(routes::suppliers::render_suppliers))
        .route("/branches/:branch_id/actors",          get(routes::actors::render_actors))
        // T21: Branch settings (Owner-only)
        .route("/branches/:branch_id/settings",        get(routes::settings::render_settings))
        // T07: browser-facing SSE relay
        .route("/branches/:branch_id/events", get(routes::sse_relay::relay_branch_events))
}