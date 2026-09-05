//! T05b: K8s liveness and readiness probes.
//!
//! GET /health — liveness probe.
//!   Always returns 200. Proves the process is alive and the HTTP server
//!   is accepting connections. K8s restarts the pod if this fails.
//!   No DB check — if Postgres is down the pod is still alive; it will
//!   return errors on real requests but should not be restarted.
//!
//! GET /ready — readiness probe.
//!   Returns 200 only when the DB is reachable. K8s removes the pod from
//!   the load balancer if this fails (stops new requests) but does NOT
//!   restart it. Used for:
//!   - Startup: pod is not added to LB until DB is reachable.
//!   - Rolling deploy: old pod stays in rotation while new pod waits for DB.
//!   - DB transient blip: pod removed from LB, returns when DB recovers.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct ReadyResponse {
    status: &'static str,
    db: &'static str,
}

/// GET /health — liveness probe. Always 200 if the process is running.
pub async fn health() -> impl IntoResponse {
    Json(HealthResponse { status: "ok" })
}

/// GET /ready — readiness probe. 200 only when DB ping succeeds.
pub async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => (
            StatusCode::OK,
            Json(ReadyResponse { status: "ok", db: "ok" }),
        )
            .into_response(),
        Err(e) => {
            tracing::warn!("readiness check failed: {e}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ReadyResponse {
                    status: "not_ready",
                    db: "unreachable",
                }),
            )
                .into_response()
        }
    }
}