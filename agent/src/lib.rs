//! Agent crate (T03): turns an inbound Worker/Supplier message into zero or
//! more `DomainEvent`s. Never called from the webhook handler synchronously
//! (T04) — always invoked from the async worker draining `webhook_inbox`.

#![warn(clippy::all)]

mod classify;
mod outcome;
mod prefilter;
mod thread_context;

pub use classify::ClaudeClassifier;
pub use outcome::{InterpretationError, InterpretationOutcome, OwnerAlert};
pub use thread_context::ThreadContextStore;

use domain::{ChannelIdentity, DomainEvent, DomainEventVariant, OrderId, WorkerId};
use prefilter::Prefilter;

pub struct WorkerAgent {
    prefilter: Prefilter,
    classifier: ClaudeClassifier,
}

impl WorkerAgent {
    pub fn new(classifier: ClaudeClassifier) -> Self {
        Self { prefilter: Prefilter::worker_events(), classifier }
    }

    /// Interpret a Worker's message. `sender` resolves to `worker_id` by the
    /// caller (messaging crate owns that lookup, T04); `threads` supplies the
    /// set of Orders currently active for this sender (T03: set-valued, not
    /// single "current Order").
    pub async fn interpret(
        &self,
        message: &str,
        worker_id: WorkerId,
        sender: &ChannelIdentity,
        threads: &ThreadContextStore,
    ) -> Result<InterpretationOutcome, InterpretationError> {
        let variant = match self.prefilter.classify(message) {
            Some(v) => Some(v),
            None => self.classifier.classify_worker_message(message).await?,
        };

        let Some(variant) = variant else {
            return Ok(InterpretationOutcome::Unprocessed {
                reason: "message did not match any known Worker action".into(),
            });
        };

        let active = threads.active_orders(sender);
        let order_id = match active {
            [] => {
                return Ok(InterpretationOutcome::Unprocessed {
                    reason: "sender has no active Order to apply this action to".into(),
                })
            }
            [only] => *only,
            many => {
                return Ok(InterpretationOutcome::NeedsOrderDisambiguation {
                    candidates: many.to_vec(),
                    question: "You have more than one active order — which one is this about?".into(),
                })
            }
        };

        Ok(InterpretationOutcome::Event(construct_worker_event(variant, worker_id, order_id)))
    }
}

pub struct SupplierAgent {
    prefilter: Prefilter,
    classifier: ClaudeClassifier,
}

impl SupplierAgent {
    pub fn new(classifier: ClaudeClassifier) -> Self {
        Self { prefilter: Prefilter::supplier_events(), classifier }
    }

    /// Interpret a Supplier message. Only `InvoiceReceived` originates here —
    /// `InvoiceApproved`/`SupplierConfirmed` are Owner-dashboard commands (T05),
    /// never Agent-interpreted.
    pub async fn interpret(&self, message: &str) -> Result<InterpretationOutcome, InterpretationError> {
        let variant = match self.prefilter.classify(message) {
            Some(v) => Some(v),
            None => self.classifier.classify_supplier_message(message).await?,
        };

        match variant {
            Some(DomainEventVariant::InvoiceReceived) => {
                // Caller (messaging crate) still needs to attach supplier_id/
                // supply_request_id/invoice_id resolved from the webhook payload.
                Ok(InterpretationOutcome::Unprocessed {
                    reason: "InvoiceReceived recognized; awaiting invoice detail extraction by caller".into(),
                })
            }
            _ => Ok(InterpretationOutcome::Unprocessed { reason: "message did not match any known Supplier action".into() }),
        }
    }
}

fn construct_worker_event(variant: DomainEventVariant, worker_id: WorkerId, order_id: OrderId) -> DomainEvent {
    match variant {
        DomainEventVariant::WorkerAssigned => DomainEvent::WorkerAssigned { worker_id, order_id },
        DomainEventVariant::WorkerAccepted => DomainEvent::WorkerAccepted { worker_id, order_id },
        DomainEventVariant::WorkerUnavailable => DomainEvent::WorkerUnavailable { worker_id, order_id },
        DomainEventVariant::WorkerCancelled => DomainEvent::WorkerCancelled { worker_id, order_id },
        DomainEventVariant::ClarificationRequested => DomainEvent::ClarificationRequested { worker_id, order_id },
        DomainEventVariant::WorkerReadyForPickup => DomainEvent::WorkerReadyForPickup { worker_id, order_id },
        DomainEventVariant::OrderDone => DomainEvent::OrderDone { order_id },
        // Supplier-only variants never reach here — WorkerAgent's prefilter/classify vocab excludes them.
        other => unreachable!("Supplier-only variant {other:?} routed into WorkerAgent"),
    }
}
