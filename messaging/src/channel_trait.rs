//! T04: the one abstraction shared across LINE/WhatsApp. Verification and
//! payload shape genuinely differ (R01/R02), so this trait is deliberately
//! thin — each implementation owns its own parsing.

use domain::ChannelIdentity;

/// One parsed inbound message, independent of transport.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub sender: ChannelIdentity,
    pub text: String,
    /// Channel-native id for this specific event, for `webhook_inbox` dedup.
    pub external_event_id: String,
    /// LINE only (R01/T04): still-valid reply token for the synchronous
    /// in-handler ack. `None` on WhatsApp, which has no reply-token concept.
    pub reply_token: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("signature/verification failed")]
    VerificationFailed,
    #[error("failed to parse webhook payload: {0}")]
    ParseFailed(String),
    #[error("send failed: {0}")]
    SendFailed(String),
}

pub trait ChannelAdapter {
    /// Verify the inbound request before any processing (T04's trust boundary).
    fn verify(&self, headers: &http_like::Headers, raw_body: &[u8]) -> Result<(), ChannelError>;

    /// Parse a verified payload into zero or more inbound messages. A single
    /// webhook call can carry multiple events (both LINE's `events[]` and
    /// WhatsApp's nested `entry[].changes[].value.messages[]`).
    fn parse_events(&self, raw_body: &[u8]) -> Result<Vec<InboundMessage>, ChannelError>;

    /// Push message — the *only* path Agent/Owner-triggered content takes
    /// (T04: reply tokens are reserved for the synchronous in-handler ack).
    fn send_push(&self, recipient: &ChannelIdentity, text: &str) -> impl std::future::Future<Output = Result<(), ChannelError>> + Send;
}

/// Minimal header lookup so `verify` doesn't depend on a specific HTTP framework.
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
