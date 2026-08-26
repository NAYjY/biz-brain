//! T01 / P01 output contract.
//!
//! `InterpretationError` now carries an internal `retryable` flag used by
//! the classify retry loop. The flag is never exposed to callers outside
//! the agent crate — they only see the error variant.

use domain::{DomainEvent, OrderId};

#[derive(Debug)]
pub enum InterpretationOutcome {
    Event(DomainEvent),
    NeedsOrderDisambiguation { candidates: Vec<OrderId>, question: String },
    Unprocessed { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum InterpretationError {
    #[error("AI API error: {0}")]
    ClaudeApi(String),
    #[error("AI response failed to parse: {0}")]
    ParseFailed(String),
    /// P01: returned a variant not in the allowed set.
    #[error("unexpected variant '{received}' (allowed: {allowed})")]
    UnexpectedVariant { received: String, allowed: String },
    #[error("request timed out")]
    Timeout,
}

impl InterpretationError {
    /// Whether the classify retry loop should try again after this error.
    /// Network timeouts and 429/5xx are transient. Parse failures and
    /// unexpected variants are not — retrying won't help.
    pub(crate) fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout | Self::ClaudeApi(_) if self.retryable_flag())
    }

    // Internal: retryable flag is stored as a tag in the message string to
    // avoid a separate enum variant. Only ClaudeApi errors carry it.
    pub(crate) fn retryable_flag(&self) -> bool {
        match self {
            // Timeout is always retryable.
            Self::Timeout => true,
            // ClaudeApi: retryable iff message starts with the sentinel.
            Self::ClaudeApi(msg) => msg.starts_with("[transient]"),
            _ => false,
        }
    }

    /// Shorthand called from classify.rs on network errors and 429/5xx.
    pub(crate) fn mark_transient(self) -> Self {
        self.with_retryable(true)
    }

    /// Tag this error as transient (retryable). Used in `classify.rs` when
    /// an HTTP 429 or 5xx is returned.
    pub(crate) fn with_retryable(self, retryable: bool) -> Self {
        if !retryable {
            return self;
        }
        match self {
            Self::ClaudeApi(msg) if !msg.starts_with("[transient]") => {
                Self::ClaudeApi(format!("[transient]{msg}"))
            }
            other => other,
        }
    }
}

#[derive(Debug)]
pub struct OwnerAlert {
    pub urgent: bool,
    pub message: String,
}

impl From<InterpretationError> for OwnerAlert {
    fn from(err: InterpretationError) -> Self {
        // Strip the internal sentinel before surfacing to Owner.
        let message = err.to_string().replace("[transient]", "");
        OwnerAlert {
            urgent: true,
            message: format!("Message could not be interpreted and was dropped: {message}"),
        }
    }
}