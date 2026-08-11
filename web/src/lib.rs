//! Web crate (T06 / D01-D07): Leptos-thin SSR dashboard.
//! JWT stays server-side in httpOnly cookie; browser calls /api/v1/* for data.
//! D03: initial page renders query projection tables directly (no api loopback).

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
        // D02: root redirects to first branch (dashboard picks up from there)
        .route("/", get(routes::dashboard::render_dashboard))
        // D04: Orders page
        .route("/branches/:branch_id/orders", get(routes::orders::render_orders))
        // D05: Supply Requests page
        .route("/branches/:branch_id/supply-requests", get(routes::supply_requests::render_supply_requests))
        // D08-5: Actors (pending Worker/Supplier bindings) page
        .route("/branches/:branch_id/actors", get(routes::actors::render_actors))
        // T07: browser-facing SSE relay
        .route("/branches/:branch_id/events", get(routes::sse_relay::relay_branch_events))
}