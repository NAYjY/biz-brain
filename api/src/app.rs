//! T05 + D02-D05: REST endpoints, webhooks, SSE, all Branch-scoped.
//! D08-4: /supply-requests/:id/send wired.
//! Worker onboarding: POST + DELETE /workers wired.
//! message-worker: Owner sends free-text to Worker on any order state.
//! S05: security headers on every response.

use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::{routes, security_headers, state::AppState};

pub fn build_router() -> Router<AppState> {
    let branch_routes = Router::new()
        .route("/orders", get(routes::orders::list_orders).post(routes::orders::create_order))
        .route("/customers", get(routes::customers::list_customers).post(routes::customers::create_customer))
        .route("/workers", get(routes::workers::list_workers).post(routes::workers::create_worker))
        .route("/workers/:worker_id", delete(routes::workers::delete_worker))
        .route(
            "/supply-requests",
            get(routes::supply_requests::list_supply_requests)
                .post(routes::supply_requests::create_supply_request),
        )
        .route("/invoices", get(routes::invoices::list_invoices))
        .route("/orders/:order_id/assign-worker",  post(routes::commands::assign_worker))
        .route("/orders/:order_id/close",          post(routes::commands::close_order))
        .route("/orders/:order_id/message-worker", post(routes::commands::message_worker))
        .route(
            "/supply-requests/:supply_request_id/approve-invoice",
            post(routes::commands::approve_invoice),
        )
        .route(
            "/supply-requests/:supply_request_id/send",
            post(routes::supply_requests::send_supply_request),
        )
        .route("/events", get(routes::sse::stream_branch_events))
        .route("/actors/pending",             get(routes::actors::list_pending_bindings))
        .route("/actors/:actor_id/confirm",   post(routes::actors::confirm_binding))
        .route("/actors/:actor_id/reject",    post(routes::actors::reject_binding));

    let api_v1 = Router::new()
        .nest("/branches/:branch_id", branch_routes)
        .route("/branches", get(routes::branches::list_branches).post(routes::branches::create_branch));

    let webhooks = Router::new()
        .route("/webhooks/line", post(routes::webhooks::line_webhook))
        .route(
            "/webhooks/whatsapp",
            get(routes::webhooks::whatsapp_verify).post(routes::webhooks::whatsapp_webhook),
        )
        .route("/webhooks/telegram", post(routes::webhooks::telegram_webhook));

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::exact(
            "http://localhost:8080".parse().unwrap(),
        ))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::DELETE,
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
