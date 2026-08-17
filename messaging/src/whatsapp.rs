//! WhatsApp adapter. R02: nested `entry[].changes[].value.messages[]`, GET
//! handshake verification (not per-request HMAC), no reply-token concept.
//!
//! P05: `fetch_media` implemented — downloads the image/document bytes from
//! the Graph API using the message's media id.

use domain::{Channel, ChannelIdentity};
use serde::Deserialize;

use crate::channel_trait::{http_like::Headers, ChannelAdapter, ChannelError, InboundMessage, MediaBlob};

pub struct WhatsAppAdapter {
    verify_token: String,
    access_token: String,
    phone_number_id: String,
    http: reqwest::Client,
}

// ── Webhook payload structures ───────────────────────────────────────────── //

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
    #[serde(rename = "type")]
    kind: String,
    text: Option<WaText>,
    image: Option<WaMedia>,
    document: Option<WaMedia>,
}

#[derive(Deserialize)]
struct WaText {
    body: String,
}

#[derive(Deserialize)]
struct WaMedia {
    id: String,
    mime_type: Option<String>,
}

// ── Graph API media response ─────────────────────────────────────────────── //

#[derive(Deserialize)]
struct MediaUrlResponse {
    url: String,
    mime_type: Option<String>,
}

impl WhatsAppAdapter {
    pub fn new(
        verify_token: impl Into<String>,
        access_token: impl Into<String>,
        phone_number_id: impl Into<String>,
    ) -> Self {
        Self {
            verify_token: verify_token.into(),
            access_token: access_token.into(),
            phone_number_id: phone_number_id.into(),
            http: reqwest::Client::new(),
        }
    }

    /// GET-handshake webhook registration (R02).
    pub fn verify_handshake(&self, mode: &str, token: &str, challenge: &str) -> Option<String> {
        (mode == "subscribe" && token == self.verify_token).then(|| challenge.to_string())
    }
}

impl ChannelAdapter for WhatsAppAdapter {
    fn verify(&self, _headers: &Headers, _raw_body: &[u8]) -> Result<(), ChannelError> {
        // R02: no per-request signature on the message webhook itself.
        Ok(())
    }

    fn parse_events(&self, raw_body: &[u8]) -> Result<Vec<InboundMessage>, ChannelError> {
        let body: WebhookBody = serde_json::from_slice(raw_body)
            .map_err(|e| ChannelError::ParseFailed(e.to_string()))?;

        let messages = body
            .entry
            .into_iter()
            .flat_map(|e| e.changes)
            .flat_map(|c| c.value.messages)
            .filter_map(|m| {
                // P05: extract media_id from image or document messages.
                let (text, media_id) = match m.kind.as_str() {
                    "text" => (m.text?.body, None),
                    "image" => {
                        let img = m.image?;
                        // Use a placeholder caption for classification
                        (format!("invoice {}", img.mime_type.as_deref().unwrap_or("")), Some(img.id))
                    }
                    "document" => {
                        let doc = m.document?;
                        (format!("invoice {}", doc.mime_type.as_deref().unwrap_or("")), Some(doc.id))
                    }
                    _ => return None,
                };

                Some(InboundMessage {
                    sender: ChannelIdentity { channel: Channel::WhatsApp, external_id: m.from },
                    text,
                    external_event_id: m.id,
                    reply_token: None,
                    media_id,
                })
            })
            .collect();

        Ok(messages)
    }

    fn send_push(
        &self,
        recipient: &ChannelIdentity,
        text: &str,
    ) -> impl std::future::Future<Output = Result<(), ChannelError>> + Send {
        let url = format!(
            "https://graph.facebook.com/v20.0/{}/messages",
            self.phone_number_id
        );
        let payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": recipient.external_id,
            "type": "text",
            "text": { "body": text },
        });
        let req = self.http.post(url).bearer_auth(&self.access_token).json(&payload);

        async move {
            req.send()
                .await
                .map_err(|e| ChannelError::SendFailed(e.to_string()))?;
            Ok(())
        }
    }

    /// P05: two-step Graph API media fetch.
    /// Step 1: GET /v20.0/{media_id} → URL + mime_type.
    /// Step 2: GET <url> → raw bytes.
    fn fetch_media(
        &self,
        media_id: &str,
    ) -> impl std::future::Future<Output = Result<MediaBlob, ChannelError>> + Send {
        let url = format!("https://graph.facebook.com/v20.0/{media_id}");
        let meta_req = self.http.get(&url).bearer_auth(&self.access_token);
        let http = self.http.clone();
        let token = self.access_token.clone();

        async move {
            let meta: MediaUrlResponse = meta_req
                .send()
                .await
                .map_err(|e| ChannelError::SendFailed(e.to_string()))?
                .json()
                .await
                .map_err(|e| ChannelError::ParseFailed(e.to_string()))?;

            let bytes = http
                .get(&meta.url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| ChannelError::SendFailed(e.to_string()))?
                .bytes()
                .await
                .map_err(|e| ChannelError::SendFailed(e.to_string()))?;

            Ok(MediaBlob {
                data: bytes.to_vec(),
                mime_type: meta.mime_type.unwrap_or_else(|| "application/octet-stream".to_string()),
            })
        }
    }
}
