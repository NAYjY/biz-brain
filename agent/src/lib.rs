//! Agent crate (T01 / T03 / P01 / P13).
//!
//! T01: `WorkerAgent` and `SupplierAgent` now hold `Arc<dyn ClassifyClient>`
//! so the caller can inject either `ClaudeClassifier` or `GeminiClassifier`
//! based on per-branch config. No other behaviour changes.

#![warn(clippy::all)]

pub mod classify;
pub mod outcome;
pub mod prefilter;
pub mod provider;
pub mod thread_context;

pub use classify::{ActiveOrderContext, ClaudeClassifier, GeminiClassifier, HistoryMessage};
pub use outcome::{InterpretationError, InterpretationOutcome, OwnerAlert};
pub use provider::{AiProvider, ClassifyClient};
pub use thread_context::ThreadContextStore;

use std::sync::Arc;

use domain::{DomainEvent, DomainEventVariant, OrderId, WorkerId};
use prefilter::Prefilter;

// ── Worker agent ──────────────────────────────────────────────────────────── //

pub struct WorkerAgent {
    prefilter: Prefilter,
    classifier: Arc<dyn ClassifyClient>,
}

impl WorkerAgent {
    pub fn new(classifier: Arc<dyn ClassifyClient>) -> Self {
        Self { prefilter: Prefilter::worker_events(), classifier }
    }

    /// Returns `true` if the prefilter matched (no AI call was made).
    /// F04: used by inbox_worker to decide whether to set ai_routed_low_confidence.
    pub fn prefilter_hit(&self, message: &str) -> bool {
        self.prefilter.classify(message).is_some()
    }

    pub async fn classify(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_orders: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<uuid::Uuid>)>, InterpretationError> {
        if let Some(variant) = self.prefilter.classify(message) {
            return Ok(Some((variant, None)));
        }
        self.classifier.classify_worker(message, history, active_orders).await
    }
}

// ── Supplier agent ────────────────────────────────────────────────────────── //

pub struct SupplierAgent {
    prefilter: Prefilter,
    classifier: Arc<dyn ClassifyClient>,
}

impl SupplierAgent {
    pub fn new(classifier: Arc<dyn ClassifyClient>) -> Self {
        Self { prefilter: Prefilter::supplier_events(), classifier }
    }

    pub fn prefilter_hit(&self, message: &str) -> bool {
        self.prefilter.classify(message).is_some()
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
        self.classifier.classify_supplier(message, history, active_supply_requests).await
    }
}

// ── Event construction ────────────────────────────────────────────────────── //

pub fn construct_worker_event(
    variant: DomainEventVariant,
    worker_id: WorkerId,
    order_id: OrderId,
) -> DomainEvent {
    match variant {
        DomainEventVariant::WorkerAssigned         => DomainEvent::WorkerAssigned { worker_id, order_id },
        DomainEventVariant::WorkerAccepted         => DomainEvent::WorkerAccepted { worker_id, order_id },
        DomainEventVariant::WorkerUnavailable      => DomainEvent::WorkerUnavailable { worker_id, order_id },
        DomainEventVariant::WorkerCancelled        => DomainEvent::WorkerCancelled { worker_id, order_id },
        DomainEventVariant::ClarificationRequested => DomainEvent::ClarificationRequested { worker_id, order_id },
        DomainEventVariant::WorkerReadyForPickup   => DomainEvent::WorkerReadyForPickup { worker_id, order_id },
        DomainEventVariant::OrderDone              => DomainEvent::OrderDone { order_id },
        other => unreachable!("Supplier-only variant {other:?} routed into WorkerAgent"),
    }
}