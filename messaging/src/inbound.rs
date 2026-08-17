//! T04 / P05: channel-agnostic webhook-ingestion flow.
//! Verify → parse → insert each message into `webhook_inbox` (dedup) → return.
//!
//! P05: `media_id` is persisted in the raw_payload JSON so the inbox_worker
//! can pass it to `fetch_media` later without needing the original request.

use domain::Channel;
use store::WebhookInbox;

use crate::channel_trait::{http_like::Headers, ChannelAdapter, ChannelError};

pub async fn receive<A: ChannelAdapter>(
    adapter: &A,
    channel: Channel,
    inbox: &WebhookInbox,
    headers: &Headers,
    raw_body: &[u8],
) -> Result<Vec<crate::channel_trait::InboundMessage>, ChannelError> {
    adapter.verify(headers, raw_body)?;
    let messages = adapter.parse_events(raw_body)?;

    for msg in &messages {
        // P05: include media_id in the persisted payload so inbox_worker can
        // fetch the bytes asynchronously without the original HTTP request.
        let payload = serde_json::json!({
            "sender": msg.sender,
            "text":   msg.text,
            "media_id": msg.media_id,
        });

        // Duplicate is not an error — LINE/WhatsApp/Telegram can all redeliver.
        let _ = inbox.record(channel, &msg.external_event_id, payload).await;
    }

    Ok(messages)
}
