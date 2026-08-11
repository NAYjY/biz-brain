//! T05 + D02-D05: REST endpoints, webhooks, SSE, all Branch-scoped.
//! D08-4: /supply-requests/:id/send wired.
//! S05: security headers on every response.

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{routes, security_headers, state::AppState};

pub fn build_router() -> Router<AppState> {
    let branch_routes = Router::new()
        // D04: Orders + Customers + Workers
        .route("/orders", get(routes::orders::list_orders).post(routes::orders::create_order))
        .route("/customers", get(routes::customers::list_customers).post(routes::customers::create_customer))
        .route("/workers", get(routes::workers::list_workers))
        // D05: Supply Requests + Invoices
        .route(
            "/supply-requests",
            get(routes::supply_requests::list_supply_requests)
                .post(routes::supply_requests::create_supply_request),
        )
        .route("/invoices", get(routes::invoices::list_invoices))
        // T05: Owner commands
        .route("/orders/:order_id/assign-worker", post(routes::commands::assign_worker))
        .route("/orders/:order_id/close", post(routes::commands::close_order))
        .route(
            "/supply-requests/:supply_request_id/approve-invoice",
            post(routes::commands::approve_invoice),
        )
        // D08-4: Send supply request (Draft -> Sent)
        .route(
            "/supply-requests/:supply_request_id/send",
            post(routes::supply_requests::send_supply_request),
        )
        // T07: SSE
        .route("/events", get(routes::sse::stream_branch_events))
        // S06: actor binding management
        .route("/actors/pending", get(routes::actors::list_pending_bindings))
        .route("/actors/:actor_id/confirm", post(routes::actors::confirm_binding))
        .route("/actors/:actor_id/reject", post(routes::actors::reject_binding));

    let api_v1 = Router::new()
        .nest("/branches/:branch_id", branch_routes)
        // D02: Branch create/list
        .route("/branches", get(routes::branches::list_branches).post(routes::branches::create_branch));

    let webhooks = Router::new()
        .route("/webhooks/line", post(routes::webhooks::line_webhook))
        .route(
            "/webhooks/whatsapp",
            get(routes::webhooks::whatsapp_verify).post(routes::webhooks::whatsapp_webhook),
        );

    // D08-6: tighten CORS — same-origin only
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::exact(
            "http://localhost:8080".parse().unwrap(),
        ))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
        ])
        .allow_headers([axum::http::header::CONTENT_TYPE]);

    Router::new()
        .nest("/api/v1", api_v1)
        .merge(webhooks)
        .layer(security_headers::csp_layer())
        .layer(security_headers::x_frame_options_layer())
        .layer(security_headers::x_content_type_options_layer())
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}