//! T04 / P05: one abstraction shared across LINE / WhatsApp / Telegram.
//! Verification and payload shape genuinely differ (R01/R02/R03), so this
//! trait is deliberately thin — each implementation owns its own parsing.
//!
//! P05: `fetch_media` added for WhatsApp invoice image download.

use domain::ChannelIdentity;

/// One parsed inbound message, independent of transport.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub sender: ChannelIdentity,
    pub text: String,
    /// Channel-native id for this specific event — used for `webhook_inbox` dedup.
    pub external_event_id: String,
    /// LINE only (R01/T04): still-valid reply token for the synchronous
    /// in-handler ack.  `None` on WhatsApp/Telegram.
    pub reply_token: Option<String>,
    /// P05: channel-native media id if the message includes an image/document.
    /// Callers in the Supplier path call `fetch_media` when this is Some.
    pub media_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("signature/verification failed")]
    VerificationFailed,
    #[error("failed to parse webhook payload: {0}")]
    ParseFailed(String),
    #[error("send failed: {0}")]
    SendFailed(String),
    /// P05: channel does not support media fetch (LINE, Telegram stub).
    #[error("media fetch not supported on this channel")]
    MediaFetchUnsupported,
}

/// Fetched media blob from a channel (P05).
#[derive(Debug)]
pub struct MediaBlob {
    pub data: Vec<u8>,
    pub mime_type: String,
}

pub trait ChannelAdapter {
    /// Verify the inbound request before any processing (T04's trust boundary).
    fn verify(&self, headers: &http_like::Headers, raw_body: &[u8]) -> Result<(), ChannelError>;

    /// Parse a verified payload into zero or more inbound messages.
    fn parse_events(&self, raw_body: &[u8]) -> Result<Vec<InboundMessage>, ChannelError>;

    /// Push message — the *only* path Agent/Owner-triggered content takes.
    fn send_push(
        &self,
        recipient: &ChannelIdentity,
        text: &str,
    ) -> impl std::future::Future<Output = Result<(), ChannelError>> + Send;

    /// P05: fetch a media attachment by its channel-native id.
    /// LINE and Telegram return `Err(MediaFetchUnsupported)` — they are never
    /// called on the Supplier path in practice.
    fn fetch_media(
        &self,
        media_id: &str,
    ) -> impl std::future::Future<Output = Result<MediaBlob, ChannelError>> + Send;
}

pub mod http_like {
    use std::collections::HashMap;

    #[derive(Debug, Default, Clone)]
    pub struct Headers(pub HashMap<String, String>);

    impl Headers {
        pub fn get(&self, name: &str) -> Option<&str> {
            self.0.get(&name.to_ascii_lowercase()).map(String::as_str)
        }
    }
}
