//! T03 / P01 output contract: two lanes.
//! `DomainEvent` JSON is system truth; outbound NL text is separate.
//!
//! P01: `InterpretationError::UnexpectedVariant` added — triggers Owner alert,
//! never silently dropped.

use domain::{DomainEvent, OrderId};

#[derive(Debug)]
pub enum InterpretationOutcome {
    /// A DomainEvent was produced — hand to `store::EventSourcing::append`.
    Event(DomainEvent),
    /// Sender has multiple active Orders and the message didn't disambiguate.
    /// Agent sends a ranked clarifying question back to the Worker (P02).
    NeedsOrderDisambiguation { candidates: Vec<OrderId>, question: String },
    /// Pre-filter + classify both returned nothing, or a timeout elapsed.
    /// Not itself a DomainEvent; routes to ClarificationRequested at the caller.
    Unprocessed { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum InterpretationError {
    #[error("Claude API error: {0}")]
    ClaudeApi(String),
    #[error("Claude response failed to parse: {0}")]
    ParseFailed(String),
    /// P01: Claude returned a variant not in the allowed set.
    /// This is a signal integrity issue that must alert the Owner.
    #[error("unexpected variant '{received}' (allowed: {allowed})")]
    UnexpectedVariant { received: String, allowed: String },
    #[error("request timed out")]
    Timeout,
}

/// T03 / P01: any Claude API error/timeout/parse-fail/unexpected-variant drops
/// the message and raises an urgent Owner alert.
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
