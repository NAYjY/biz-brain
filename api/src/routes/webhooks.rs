//! T04: webhook ingestion routes.
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use std::collections::HashMap;
use domain::Channel;
use messaging::Headers;
use crate::state::AppState;

fn to_messaging_headers(headers: &HeaderMap) -> Headers {
    let mut map = HashMap::new();
    for (name, value) in headers {
        if let Ok(v) = value.to_str() {
            map.insert(name.as_str().to_ascii_lowercase(), v.to_string());
        }
    }
    Headers(map)
}

pub async fn line_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let h = to_messaging_headers(&headers);
    match messaging::inbound::receive(state.line.as_ref(), Channel::Line, &state.inbox, &h, &body).await {
        Ok(messages) => {
            for msg in messages {
                if let Some(token) = msg.reply_token {
                    let _ = state.line.reply(&token, "Got it — one sec.").await;
                }
            }
            StatusCode::OK
        }
        Err(messaging::ChannelError::VerificationFailed) => StatusCode::UNAUTHORIZED,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

pub async fn whatsapp_verify(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let mode = params.get("hub.mode").cloned().unwrap_or_default();
    let token = params.get("hub.verify_token").cloned().unwrap_or_default();
    let challenge = params.get("hub.challenge").cloned().unwrap_or_default();
    match state.whatsapp.verify_handshake(&mode, &token, &challenge) {
        Some(echoed) => (StatusCode::OK, echoed).into_response(),
        None => StatusCode::FORBIDDEN.into_response(),
    }
}

pub async fn whatsapp_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let h = to_messaging_headers(&headers);
    match messaging::inbound::receive(state.whatsapp.as_ref(), Channel::WhatsApp, &state.inbox, &h, &body).await {
        Ok(_) => StatusCode::OK,
        Err(messaging::ChannelError::VerificationFailed) => StatusCode::UNAUTHORIZED,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

pub async fn telegram_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let h = to_messaging_headers(&headers);
    match messaging::inbound::receive(state.telegram.as_ref(), Channel::Telegram, &state.inbox, &h, &body).await {
        Ok(_) => StatusCode::OK,
        Err(messaging::ChannelError::VerificationFailed) => StatusCode::UNAUTHORIZED,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}
