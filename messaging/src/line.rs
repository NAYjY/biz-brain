//! LINE adapter. R01: `events[]` array (can be empty), verify via
//! `x-line-signature` HMAC-SHA256 of the raw body against the channel secret,
//! reply tokens are one-time-use and expire fast.

use domain::{Channel, ChannelIdentity};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

use crate::channel_trait::{http_like::Headers, ChannelAdapter, ChannelError, InboundMessage};

type HmacSha256 = Hmac<Sha256>;

pub struct LineAdapter {
    channel_secret: String,
    channel_access_token: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct WebhookBody {
    events: Vec<LineEvent>,
}

#[derive(Deserialize)]
struct LineEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "webhookEventId")]
    webhook_event_id: String,
    source: LineSource,
    message: Option<LineMessage>,
    #[serde(rename = "replyToken")]
    reply_token: Option<String>,
}

#[derive(Deserialize)]
struct LineSource {
    #[serde(rename = "userId")]
    user_id: String,
}

#[derive(Deserialize)]
struct LineMessage {
    text: Option<String>,
}

impl LineAdapter {
    pub fn new(channel_secret: impl Into<String>, channel_access_token: impl Into<String>) -> Self {
        Self { channel_secret: channel_secret.into(), channel_access_token: channel_access_token.into(), http: reqwest::Client::new() }
    }

    /// Synchronous in-handler ack (T04) — the only legitimate use of a reply
    /// token, since it's still guaranteed valid at this point in the request.
    pub async fn reply(&self, reply_token: &str, text: &str) -> Result<(), ChannelError> {
        self.http
            .post("https://api.line.me/v2/bot/message/reply")
            .bearer_auth(&self.channel_access_token)
            .json(&serde_json::json!({
                "replyToken": reply_token,
                "messages": [{ "type": "text", "text": text }],
            }))
            .send()
            .await
            .map_err(|e| ChannelError::SendFailed(e.to_string()))?;
        Ok(())
    }
}

impl ChannelAdapter for LineAdapter {
    fn verify(&self, headers: &Headers, raw_body: &[u8]) -> Result<(), ChannelError> {
        let signature = headers.get("x-line-signature").ok_or(ChannelError::VerificationFailed)?;

        let mut mac = HmacSha256::new_from_slice(self.channel_secret.as_bytes())
            .map_err(|_| ChannelError::VerificationFailed)?;
        mac.update(raw_body);
        let expected = mac.finalize().into_bytes();
        let expected_b64 = base64_encode(&expected);

        if expected_b64 != signature {
            return Err(ChannelError::VerificationFailed);
        }
        Ok(())
    }

    fn parse_events(&self, raw_body: &[u8]) -> Result<Vec<InboundMessage>, ChannelError> {
        let body: WebhookBody = serde_json::from_slice(raw_body).map_err(|e| ChannelError::ParseFailed(e.to_string()))?;

        Ok(body
            .events
            .into_iter()
            .filter(|e| e.kind == "message")
            .filter_map(|e| {
                let text = e.message.as_ref()?.text.clone()?;
                Some(InboundMessage {
                    sender: ChannelIdentity { channel: Channel::Line, external_id: e.source.user_id },
                    text,
                    external_event_id: e.webhook_event_id,
                    reply_token: e.reply_token,
                })
            })
            .collect())
    }

    fn send_push(&self, recipient: &ChannelIdentity, text: &str) -> impl std::future::Future<Output = Result<(), ChannelError>> + Send {
        async move {
            self.http
                .post("https://api.line.me/v2/bot/message/push")
                .bearer_auth(&self.channel_access_token)
                .json(&serde_json::json!({
                    "to": recipient.external_id,
                    "messages": [{ "type": "text", "text": text }],
                }))
                .send()
                .await
                .map_err(|e| ChannelError::SendFailed(e.to_string()))?;
            Ok(())
        }
    }
}

/// Minimal base64 (standard, with padding) — avoids pulling in the `base64` crate for one call.
fn base64_encode(bytes: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(CHARS[(b0 >> 2) as usize] as char);
        out.push(CHARS[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 { CHARS[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[(b2 & 0x3f) as usize] as char } else { '=' });
    }
    out
}
