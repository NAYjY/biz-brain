//! T05 / P04: REST endpoints, webhooks, SSE.
//! P04: cancel, reset, resolve-clarification endpoints added.
//! P05: invoice media endpoint added.
//! S05: security headers on every response.

use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::{routes, security_headers, state::AppState};

pub fn build_router() -> Router<AppState> {
    let branch_routes = Router::new()
        // Orders
        .route("/orders", get(routes::orders::list_orders).post(routes::orders::create_order))
        // Customers
        .route("/customers", get(routes::customers::list_customers).post(routes::customers::create_customer))
        // Workers
        .route("/workers", get(routes::workers::list_workers).post(routes::workers::create_worker))
        .route("/workers/:worker_id", delete(routes::workers::delete_worker))
        // Supply requests
        .route("/supply-requests",
            get(routes::supply_requests::list_supply_requests)
                .post(routes::supply_requests::create_supply_request),
        )
        .route("/supply-requests/:supply_request_id/send", post(routes::supply_requests::send_supply_request))
        .route("/supply-requests/:supply_request_id/approve-invoice", post(routes::commands::approve_invoice))
        // Invoices
        .route("/invoices", get(routes::invoices::list_invoices))
        // P05: invoice media download
        .route("/invoices/:invoice_id/media", get(routes::invoices::get_invoice_media))
        // Order commands
        .route("/orders/:order_id/assign-worker",          post(routes::commands::assign_worker))
        .route("/orders/:order_id/close",                  post(routes::commands::close_order))
        .route("/orders/:order_id/message-worker",         post(routes::commands::message_worker))
        // P04: new Owner command endpoints
        .route("/orders/:order_id/cancel",                 post(routes::commands::cancel_order))
        .route("/orders/:order_id/reset",                  post(routes::commands::reset_order))
        .route("/orders/:order_id/resolve-clarification",  post(routes::commands::resolve_clarification))
        // SSE
        .route("/events", get(routes::sse::stream_branch_events))
        // Actor bindings (S06)
        .route("/actors/pending",           get(routes::actors::list_pending_bindings))
        .route("/actors/:actor_id/confirm", post(routes::actors::confirm_binding))
        .route("/actors/:actor_id/reject",  post(routes::actors::reject_binding));

    let api_v1 = Router::new()
        .nest("/branches/:branch_id", branch_routes)
        .route("/branches", get(routes::branches::list_branches).post(routes::branches::create_branch));

    let webhooks = Router::new()
        .route("/webhooks/line", post(routes::webhooks::line_webhook))
        .route("/webhooks/whatsapp",
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
