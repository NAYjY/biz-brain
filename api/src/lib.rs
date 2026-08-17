//! API crate (T05): Axum HTTP surface for the Owner dashboard, webhooks, SSE.
//! P01/P04/P13/P14: classify prompt harness, new command endpoints, unified routing.

#![warn(clippy::all)]

pub mod app;
pub mod event_handler;
pub mod extractors;
pub mod inbox_worker;
pub mod routes;
pub mod security_headers;
pub mod state;

pub use app::build_router;
pub use extractors::Claims;
pub use state::AppState;
