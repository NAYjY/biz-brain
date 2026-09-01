//! T08: In-process rate limiting via the `governor` crate.
//!
//! Two independent limiters, each keyed on the client IP extracted from
//! `X-Forwarded-For` (Railway sets this) or the direct peer address:
//!
//! - `LOGIN_LIMITER`   — 5 requests / 15 minutes per IP.
//!   Applied to `POST /login` (web crate) and `POST /api/v1/*` auth-less paths
//!   that a brute-force attacker would target.
//!
//! - `WEBHOOK_LIMITER` — 200 requests / minute per IP.
//!   Applied to `/webhooks/line`, `/webhooks/whatsapp`, `/webhooks/telegram`.
//!   Legitimate webhook traffic from LINE/WhatsApp/Telegram infra is well under
//!   this ceiling; the cap only blocks abuse.
//!
//! Both limiters use a `DashMap`-backed keyed governor so each IP gets an
//! independent bucket. State is entirely in-process (no Redis); single-binary
//! deployment (T06) makes this sufficient.
//!
//! On rate-limit breach the middleware returns `429 Too Many Requests` with a
//! `Retry-After: <seconds>` header computed from the governor's wait time.

use std::{
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    sync::Arc,
    time::Duration,
};

use axum::{
    extract::{ConnectInfo, Request},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use governor::{
    clock::{Clock, DefaultClock},
    middleware::NoOpMiddleware,
    state::keyed::DefaultKeyedStateStore,
    Quota, RateLimiter,
};

/// A per-IP keyed rate limiter.
pub type KeyedLimiter =
    RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock, NoOpMiddleware>;

/// Build the login limiter: 5 requests per 15 minutes per IP.
pub fn login_limiter() -> Arc<KeyedLimiter> {
    // 15 minutes = 900 seconds; burst of 5 from an empty bucket.
    let quota = Quota::with_period(Duration::from_secs(180))
        .expect("valid period")
        .allow_burst(NonZeroU32::new(5).unwrap());
    Arc::new(RateLimiter::keyed(quota))
}

/// Build the webhook limiter: 200 requests per minute per IP.
pub fn webhook_limiter() -> Arc<KeyedLimiter> {
    let quota = Quota::per_minute(NonZeroU32::new(200).unwrap());
    Arc::new(RateLimiter::keyed(quota))
}

/// Extract the best client IP from the request.
/// Prefers the leftmost address in `X-Forwarded-For` (set by Railway's proxy)
/// and falls back to the direct peer address injected by Axum's `ConnectInfo`.
pub fn client_ip(req: &Request) -> IpAddr {
    // X-Forwarded-For: <client>, <proxy1>, <proxy2>
    if let Some(xff) = req.headers().get("x-forwarded-for") {
        if let Ok(val) = xff.to_str() {
            if let Some(first) = val.split(',').next() {
                if let Ok(ip) = first.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }
    // ConnectInfo fallback (local dev without a proxy).
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]))
}

/// Axum middleware that applies `limiter` keyed on the client IP.
/// Returns `429` with a `Retry-After` header on breach.
pub async fn rate_limit_layer(
    limiter: Arc<KeyedLimiter>,
    req: Request,
    next: Next,
) -> Response {
    let ip = client_ip(&req);
    match limiter.check_key(&ip) {
        Ok(_) => next.run(req).await,
        Err(not_until) => {
            let clock = DefaultClock::default();
            let wait_secs = not_until
                .wait_time_from(clock.now())
                .as_secs()
                .max(1);
            (
                StatusCode::TOO_MANY_REQUESTS,
                [
                    (header::RETRY_AFTER, wait_secs.to_string()),
                    (header::CONTENT_TYPE, "text/plain".to_string()),
                ],
                format!("Rate limit exceeded. Retry after {wait_secs}s."),
            )
                .into_response()
        }
    }
}