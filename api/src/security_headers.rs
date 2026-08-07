//! S05: security headers middleware. Applied to every response via
//! `tower_http::set_header`. CSP is the second-layer defence against XSS
//! (first layer: Leptos view! macro escapes by default). No inline scripts
//! or styles allowed — all JS/CSS is served as static files from `web`.
//!
//! `script-src 'self'` — no CDN scripts, no inline. If a third-party script
//! is ever needed, add it explicitly here rather than loosening to unsafe-inline.

use axum::http::{header, HeaderValue};
use tower_http::set_header::SetResponseHeaderLayer;

/// CSP value. Strict — adjust only with explicit justification.
const CSP: &str = concat!(
    "default-src 'self'; ",
    "script-src 'self'; ",        // no inline, no eval
    "style-src 'self'; ",         // no inline styles
    "img-src 'self' data:; ",     // data: for any inline SVG icons
    "connect-src 'self'; ",       // fetch() to same origin only (api/v1 + SSE)
    "frame-ancestors 'none'; ",   // clickjacking protection
    "base-uri 'self'; ",
    "form-action 'self'",
);

pub fn csp_layer() -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    )
}

/// X-Frame-Options redundant with frame-ancestors CSP but kept for older
/// browsers/proxies that don't parse CSP.
pub fn x_frame_options_layer() -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static("DENY"),
    )
}

pub fn x_content_type_options_layer() -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    )
}