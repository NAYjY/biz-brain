//! API crate (T05): Axum HTTP surface for the Owner dashboard, plus T04's
//! webhook ingestion and T07's SSE stream (all Branch-scoped, same service).
//!
//! S05: security_headers module added.

#![warn(clippy::all)]

pub mod app;
pub mod extractors;
pub mod inbox_worker;
pub mod routes;
pub mod security_headers;
pub mod state;

pub use app::build_router;
pub use extractors::Claims;
pub use state::AppState;