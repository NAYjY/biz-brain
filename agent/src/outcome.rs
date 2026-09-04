//! Output contracts for the Gemini harness and the Claude classifier.
//!
//! Two separate outputs:
//!   HarnessOutput  — Gemini harness path (rich, bidirectional)
//!   InterpretationOutcome — Claude path (unchanged, kept for compatibility)

use domain::{DomainEvent, OrderId};
use uuid::Uuid;

// ── Gemini Harness Output ─────────────────────────────────────────────────── //

/// Full output of one Gemini harness call — covers both fresh messages and
/// continuation turns inside an active disambiguation flow.
///
/// The harness always produces a `reply` — worker always gets a response
/// regardless of whether resolution happened. This is the core design
/// principle: never leave the worker waiting for the system to decide.
#[derive(Debug, Clone)]
pub struct HarnessOutput {
    // ── Resolution ──────────────────────────────────────────────── //

    /// True if the harness resolved which order this message is about.
    pub resolved: bool,

    /// The resolved order UUID. None if needs_disambiguation is true.
    pub order_id: Option<Uuid>,

    /// The domain event to emit. None if not resolved or message is
    /// informational only (e.g. worker just sharing a note).
    pub event_variant: Option<domain::DomainEventVariant>,

    // ── Communication ────────────────────────────────────────────── //

    /// Always present. Natural Thai reply to send to the worker immediately.
    /// On disambiguation turns: acknowledges their message + asks the question.
    /// On resolution: confirms what was recorded.
    /// On escalation: tells worker owner has been notified.
    pub reply: String,

    // ── Secondary extraction ─────────────────────────────────────── //

    /// Operational notes extracted from the message regardless of resolution.
    /// e.g. ["น้ำยาหมด", "ลูกค้าร้องเรื่องเสียงดัง"]
    /// On resolution: flushed to owner_alert.
    /// On disambiguation: accumulated in disambiguation_pending.extracted_notes.
    pub extracted_notes: Vec<String>,

    /// Owner alert to surface on dashboard. Set when:
    ///   - Resolution happens AND extracted_notes is non-empty
    ///   - Message contains urgent operational info regardless of resolution
    pub owner_alert: Option<OwnerAlert>,

    // ── Disambiguation state ─────────────────────────────────────── //

    /// True when the harness cannot resolve and needs the worker to declare.
    pub needs_disambiguation: bool,

    /// If harness narrowed from N candidates to 1 (not yet confirmed),
    /// stored here so next turn asks confirmation for this specific order.
    pub narrowed_to: Option<Uuid>,

    /// The question to store as last_question in disambiguation_pending
    /// so the next turn can rephrase it.
    pub disambiguation_question: Option<String>,

    // ── Escalation ───────────────────────────────────────────────── //

    /// True when turns_elapsed has reached 3 without declaration.
    /// inbox_worker: sets escalated_at, sets ai_routed_low_confidence = TRUE
    /// so the ⚠️ badge appears on the owner's dashboard.
    pub escalate_to_owner: bool,

    /// English summary for the owner escalation alert.
    /// Includes all accumulated notes and candidates.
    pub owner_escalation_summary: Option<String>,

    // ── Meta ─────────────────────────────────────────────────────── //

    /// 0.0–1.0. Used to decide whether to set ai_routed_low_confidence
    /// even on successful resolution (below 0.75 → flag anyway).
    pub confidence: f32,
}

impl HarnessOutput {
    /// Whether this output should trigger the ⚠️ badge on the dashboard.
    /// True on escalation OR low-confidence resolution.
    pub fn should_flag_low_confidence(&self) -> bool {
        self.escalate_to_owner || (self.resolved && self.confidence < 0.75)
    }
}

// ── Owner Alert ───────────────────────────────────────────────────────────── //

#[derive(Debug, Clone)]
pub struct OwnerAlert {
    pub urgent: bool,
    /// English summary shown in the dashboard alert / thread header.
    pub message: String,
}

// ── Claude path (unchanged) ───────────────────────────────────────────────── //

/// Kept for the Claude classifier path. Not used by the Gemini harness.
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
    #[error("unexpected variant '{received}' (allowed: {allowed})")]
    UnexpectedVariant { received: String, allowed: String },
    #[error("request timed out")]
    Timeout,
}

impl InterpretationError {
    pub(crate) fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout | Self::ClaudeApi(_) if self.retryable_flag())
    }

    pub(crate) fn retryable_flag(&self) -> bool {
        match self {
            Self::Timeout => true,
            Self::ClaudeApi(msg) => msg.starts_with("[transient]"),
            _ => false,
        }
    }

    pub(crate) fn mark_transient(self) -> Self {
        self.with_retryable(true)
    }

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

impl From<InterpretationError> for OwnerAlert {
    fn from(err: InterpretationError) -> Self {
        let message = err.to_string().replace("[transient]", "");
        OwnerAlert {
            urgent: true,
            message: format!("Message could not be interpreted and was dropped: {message}"),
        }
    }
}