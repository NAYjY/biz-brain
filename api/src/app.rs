//! T05's endpoint shape: REST resource nouns for reads, one command endpoint
//! per Owner-triggerable `DomainEvent` variant for writes, all Branch-scoped
//! under `/branches/:branch_id/...`. Plus T04's webhook routes and T07's
//! per-Branch SSE stream.

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{routes, state::AppState};

/// Returns an unmounted `Router<AppState>` — the `server` binary crate merges
/// this with `web`'s routes and calls `.with_state(...)` exactly once, since
/// both crates' routes share the same `AppState` (T06: single process).
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

    Router::new()
        .nest("/api/v1", api_v1)
        .merge(webhooks)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive()) // TODO: tighten before deploy — permissive is a placeholder, not a decision
}
