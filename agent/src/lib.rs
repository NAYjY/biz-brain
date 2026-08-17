//! Agent crate (T03 / P01 / P13): turns an inbound Worker/Supplier message
//! into zero or more `DomainEvent`s.
//!
//! P01: prompt harness with Thai/English few-shot examples, OrderState
//! injection, structured output validation, Owner alert on unexpected variant.
//! P13: conversation history window, `{ variant, order_id }` output.
//! P06: SupplierConfirmed in both prefilter and classifier.

#![warn(clippy::all)]

pub mod classify;
pub mod outcome;
pub mod prefilter;
pub mod thread_context;

pub use classify::{ActiveOrderContext, ClaudeClassifier, HistoryMessage};
pub use outcome::{InterpretationError, InterpretationOutcome, OwnerAlert};
pub use thread_context::ThreadContextStore;

use domain::{ChannelIdentity, DomainEvent, DomainEventVariant, OrderId, WorkerId};
use prefilter::Prefilter;

/// Agent for Worker messages (LINE / Telegram).
pub struct WorkerAgent {
    prefilter: Prefilter,
    classifier: ClaudeClassifier,
}

impl WorkerAgent {
    pub fn new(classifier: ClaudeClassifier) -> Self {
        Self { prefilter: Prefilter::worker_events(), classifier }
    }

    /// Attempt prefilter first; fall through to Claude classify on miss.
    ///
    /// Returns `Ok(Some((variant, order_id)))` on success,
    /// `Ok(None)` when the message is unrecognized,
    /// `Err` on API/parse/unexpected-variant failure (triggers Owner alert).
    pub async fn classify(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_orders: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<uuid::Uuid>)>, InterpretationError> {
        // Cheap prefilter — no Claude call if a keyword matches.
        if let Some(variant) = self.prefilter.classify(message) {
            return Ok(Some((variant, None)));
        }
        self.classifier.classify_worker_message(message, history, active_orders).await
    }
}

/// Agent for Supplier messages (WhatsApp).
pub struct SupplierAgent {
    prefilter: Prefilter,
    classifier: ClaudeClassifier,
}

impl SupplierAgent {
    pub fn new(classifier: ClaudeClassifier) -> Self {
        Self { prefilter: Prefilter::supplier_events(), classifier }
    }

    pub async fn classify(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_supply_requests: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<uuid::Uuid>)>, InterpretationError> {
        if let Some(variant) = self.prefilter.classify(message) {
            return Ok(Some((variant, None)));
        }
        self.classifier.classify_supplier_message(message, history, active_supply_requests).await
    }
}

/// Construct the concrete `DomainEvent` from a Worker-side variant.
/// Panics in debug if a Supplier-only variant is passed (unreachable in prod).
pub fn construct_worker_event(
    variant: DomainEventVariant,
    worker_id: WorkerId,
    order_id: OrderId,
) -> DomainEvent {
    match variant {
        DomainEventVariant::WorkerAssigned        => DomainEvent::WorkerAssigned { worker_id, order_id },
        DomainEventVariant::WorkerAccepted        => DomainEvent::WorkerAccepted { worker_id, order_id },
        DomainEventVariant::WorkerUnavailable     => DomainEvent::WorkerUnavailable { worker_id, order_id },
        DomainEventVariant::WorkerCancelled       => DomainEvent::WorkerCancelled { worker_id, order_id },
        DomainEventVariant::ClarificationRequested=> DomainEvent::ClarificationRequested { worker_id, order_id },
        DomainEventVariant::WorkerReadyForPickup  => DomainEvent::WorkerReadyForPickup { worker_id, order_id },
        DomainEventVariant::OrderDone             => DomainEvent::OrderDone { order_id },
        other => unreachable!("Supplier-only variant {other:?} routed into WorkerAgent"),
    }
}
