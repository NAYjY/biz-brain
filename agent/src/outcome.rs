//! T03 output contract: two lanes. `DomainEvent` JSON is system truth;
//! outbound NL text (disambiguation, confirmations) is separate, never the
//! raw JSON.

use domain::{DomainEvent, OrderId};

#[derive(Debug)]
pub enum InterpretationOutcome {
    /// A DomainEvent was produced — hand to `store::EventSourcing::append`.
    Event(DomainEvent),
    /// Sender has multiple active Orders and the message didn't disambiguate.
    /// Agent sends this NL question back to the Worker (channel-level
    /// mechanics, not a domain decision — doesn't violate the
    /// Agent-never-resolves-ambiguity rule).
    NeedsOrderDisambiguation { candidates: Vec<OrderId>, question: String },
    /// Pre-filter + fallback classify both failed, or a timeout window on a
    /// disambiguation question elapsed unanswered. Not itself a DomainEvent —
    /// routes to `ClarificationRequested` at the caller (needs a resolved OrderId).
    Unprocessed { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum InterpretationError {
    #[error("Claude API error: {0}")]
    ClaudeApi(String),
    #[error("Claude response failed to parse into a DomainEvent: {0}")]
    ParseFailed(String),
    #[error("request timed out")]
    Timeout,
}

/// T03: any Claude API error/timeout/parse-fail drops the message — no
/// retry queue — and raises an urgent Owner alert. Uniform across LINE/WhatsApp.
#[derive(Debug)]
pub struct OwnerAlert {
    pub urgent: bool,
    pub message: String,
}

impl From<InterpretationError> for OwnerAlert {
    fn from(err: InterpretationError) -> Self {
        OwnerAlert { urgent: true, message: format!("Message could not be interpreted and was dropped: {err}") }
    }
}
