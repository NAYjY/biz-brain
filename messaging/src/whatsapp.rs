//! WhatsApp adapter. R02: nested `entry[].changes[].value.messages[]`, GET
//! handshake verification (not per-request HMAC), no reply-token concept —
//! WhatsApp only ever pushes.

use domain::{Channel, ChannelIdentity};
use serde::Deserialize;

use crate::channel_trait::{http_like::Headers, ChannelAdapter, ChannelError, InboundMessage};

pub struct WhatsAppAdapter {
    verify_token: String,
    access_token: String,
    phone_number_id: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct WebhookBody {
    entry: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    changes: Vec<Change>,
}

#[derive(Deserialize)]
struct Change {
    value: ChangeValue,
}

#[derive(Deserialize)]
struct ChangeValue {
    #[serde(default)]
    messages: Vec<WaMessage>,
}

#[derive(Deserialize)]
struct WaMessage {
    id: String,
    from: String,
    text: Option<WaText>,
}

#[derive(Deserialize)]
struct WaText {
    body: String,
}

impl WhatsAppAdapter {
    pub fn new(verify_token: impl Into<String>, access_token: impl Into<String>, phone_number_id: impl Into<String>) -> Self {
        Self {
            verify_token: verify_token.into(),
            access_token: access_token.into(),
            phone_number_id: phone_number_id.into(),
            http: reqwest::Client::new(),
        }
    }

    /// GET-handshake webhook registration (R02) — not part of `ChannelAdapter`
    /// since it's a one-time setup flow, not per-message verification.
    pub fn verify_handshake(&self, mode: &str, token: &str, challenge: &str) -> Option<String> {
        (mode == "subscribe" && token == self.verify_token).then(|| challenge.to_string())
    }
}

impl ChannelAdapter for WhatsAppAdapter {
    fn verify(&self, _headers: &Headers, _raw_body: &[u8]) -> Result<(), ChannelError> {
        // R02: no per-request signature scheme on the message webhook itself —
        // trust boundary is the one-time GET handshake above.
        Ok(())
    }

    fn parse_events(&self, raw_body: &[u8]) -> Result<Vec<InboundMessage>, ChannelError> {
        let body: WebhookBody = serde_json::from_slice(raw_body).map_err(|e| ChannelError::ParseFailed(e.to_string()))?;

        Ok(body
            .entry
            .into_iter()
            .flat_map(|e| e.changes)
            .flat_map(|c| c.value.messages)
            .filter_map(|m| {
                let text = m.text?.body;
                Some(InboundMessage {
                    sender: ChannelIdentity { channel: Channel::WhatsApp, external_id: m.from },
                    text,
                    external_event_id: m.id,
                    reply_token: None,
                })
            })
            .collect())
    }

    fn send_push(&self, recipient: &ChannelIdentity, text: &str) -> impl std::future::Future<Output = Result<(), ChannelError>> + Send {
        async move {
            let url = format!("https://graph.facebook.com/v20.0/{}/messages", self.phone_number_id);
            self.http
                .post(url)
                .bearer_auth(&self.access_token)
                .json(&serde_json::json!({
                    "messaging_product": "whatsapp",
                    "to": recipient.external_id,
                    "type": "text",
                    "text": { "body": text },
                }))
                .send()
                .await
                .map_err(|e| ChannelError::SendFailed(e.to_string()))?;
            Ok(())
        }
    }
}
