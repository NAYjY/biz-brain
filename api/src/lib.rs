//! API crate (T05 / T20 / T13 / T08): Axum HTTP surface for the Owner dashboard, webhooks, SSE.
//! T08: rate_limit module added; login_rate_limiter() re-exported for web crate.
//! T20: AdminClaims and AuthedAdmin added.
//! T13: account module added (change-password endpoint).
//! F05: alert_worker module added.

#![warn(clippy::all)]

pub mod alert_worker;  // F05
pub mod app;
pub mod event_handler;
pub mod extractors;
pub mod inbox_worker;
pub mod inbox_worker_harness;
pub mod rate_limit;    // T08
pub mod routes;
pub mod security_headers;
pub mod state;

pub use app::{build_router, login_rate_limiter};
pub use extractors::{AuthedOwner, AuthedOwnerOnly, AuthorizedBranch, Claims};
pub use rate_limit::KeyedLimiter;
pub use state::AppState;