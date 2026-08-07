//! T05's endpoint shape: REST resource nouns for reads, one command endpoint
//! per Owner-triggerable `DomainEvent` variant for writes, all Branch-scoped
//! under `/branches/:branch_id/...`. Plus T04's webhook routes and T07's
//! per-Branch SSE stream.
//!
//! S05: security headers (CSP, X-Frame-Options, X-Content-Type-Options)
//! applied to every response via tower-http middleware.

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{routes, security_headers, state::AppState};

pub fn build_router() -> Router<AppState> {
    let api_v1 = Router::new()
        .route("/branches/:branch_id/orders", get(routes::orders::list_orders).post(routes::orders::create_order))
        .route(
            "/branches/:branch_id/supply-requests",
            get(routes::supply_requests::list_supply_requests).post(routes::supply_requests::create_supply_request),
        )
        .route("/branches/:branch_id/orders/:order_id/assign-worker", post(routes::commands::assign_worker))
        .route("/branches/:branch_id/orders/:order_id/close", post(routes::commands::close_order))
        .route(
            "/branches/:branch_id/supply-requests/:supply_request_id/approve-invoice",
            post(routes::commands::approve_invoice),
        )
        .route("/branches/:branch_id/events", get(routes::sse::stream_branch_events));

    let webhooks = Router::new()
        .route("/webhooks/line", post(routes::webhooks::line_webhook))
        .route("/webhooks/whatsapp", get(routes::webhooks::whatsapp_verify).post(routes::webhooks::whatsapp_webhook));

    // S06: Owner-facing actor binding management endpoints
    let actor_mgmt = Router::new()
        .route(
            "/branches/:branch_id/actors/pending",
            get(routes::actors::list_pending_bindings),
        )
        .route(
            "/branches/:branch_id/actors/:actor_id/confirm",
            post(routes::actors::confirm_binding),
        )
        .route(
            "/branches/:branch_id/actors/:actor_id/reject",
            post(routes::actors::reject_binding),
        );

    Router::new()
        .nest("/api/v1", api_v1.merge(actor_mgmt))
        .merge(webhooks)
        // S05: security headers on every response
        .layer(security_headers::csp_layer())
        .layer(security_headers::x_frame_options_layer())
        .layer(security_headers::x_content_type_options_layer())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive()) // TODO: tighten before deploy
}