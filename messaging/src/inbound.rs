//! T04: the channel-agnostic webhook-ingestion flow. Verify -> parse ->
//! insert each message into `webhook_inbox` (dedup) -> return. Callers (api
//! crate's axum handler) invoke this then respond 200 immediately —
//! processing happens later, async, in `process_inbox`.

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
        let payload = serde_json::json!({
            "sender": msg.sender,
            "text": msg.text,
        });
        // Duplicate is not an error at this layer — LINE/Telegram/WhatsApp can
        // all redeliver (R01/R02/R03); the webhook handler still acks 200.
        let _ = inbox.record(channel, &msg.external_event_id, payload).await;
    }

    Ok(messages)
}
