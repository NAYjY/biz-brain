//! API crate (T05 / T20): Axum HTTP surface for the Owner dashboard, webhooks, SSE.
//! T20: AdminClaims and AuthedAdmin added.
//! F05: alert_worker module added.

#![warn(clippy::all)]

pub mod alert_worker;  // F05
pub mod app;
pub mod event_handler;
pub mod extractors;
pub mod inbox_worker;
pub mod routes;
pub mod security_headers;
pub mod state;

pub use app::build_router;
pub use extractors::{AuthedOwner, AuthedOwnerOnly, AuthorizedBranch, Claims};
pub use state::AppState;
