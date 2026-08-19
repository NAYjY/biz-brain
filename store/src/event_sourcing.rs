//! T02: append domain events to the correct stream with optimistic
//! concurrency via UNIQUE (aggregate_id, sequence).
//!
//! P04/P15: OwnerCancelled, OrderReset, ClarificationResolved added.
//! P16: OwnerForce* and OwnerReassignWorker added.

use domain::{Aggregate, BranchId, DomainEvent, DomainEventVariant};
use sqlx::PgPool;

#[derive(Debug, thiserror::Error)]
pub enum AppendError {
    #[error("optimistic concurrency conflict at sequence {0}")]
    Conflict(i64),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

pub struct EventSourcing {
    pool: PgPool,
}

impl EventSourcing {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn append(
        &self,
        branch_id: BranchId,
        expected_next_sequence: i64,
        event: &DomainEvent,
    ) -> Result<(), AppendError> {
        let variant = event.variant();
        match variant.aggregate() {
            Aggregate::Order => {
                self.append_order_event(branch_id, expected_next_sequence, variant, event).await
            }
            Aggregate::SupplyRequest => {
                self.append_supply_request_event(branch_id, expected_next_sequence, variant, event)
                    .await
            }
        }
    }

    async fn append_order_event(
        &self,
        branch_id: BranchId,
        sequence: i64,
        variant: DomainEventVariant,
        event: &DomainEvent,
    ) -> Result<(), AppendError> {
        let aggregate_id = event.order_id().expect("Order-aggregate variant must carry order_id");

        let worker_id: Option<uuid::Uuid> = match event {
            DomainEvent::WorkerAssigned { worker_id, .. }
            | DomainEvent::WorkerAccepted { worker_id, .. }
            | DomainEvent::WorkerUnavailable { worker_id, .. }
            | DomainEvent::WorkerCancelled { worker_id, .. }
            | DomainEvent::ClarificationRequested { worker_id, .. }
            | DomainEvent::WorkerReadyForPickup { worker_id, .. }
            | DomainEvent::ClarificationResolved { worker_id, .. }
            | DomainEvent::OwnerForceUnavailable { worker_id, .. } => {
                Some(worker_id.into_inner())
            }
            // P16: reassign carries new_worker_id in the worker_id column.
            DomainEvent::OwnerReassignWorker { new_worker_id, .. } => {
                Some(new_worker_id.into_inner())
            }
            // Force events that operate on existing worker — no worker_id stored
            // (the projection worker reads it from the existing state).
            DomainEvent::OwnerForceAccepted { .. }
            | DomainEvent::OwnerForceClarification { .. }
            | DomainEvent::OwnerForceReady { .. }
            // No-worker events
            | DomainEvent::OrderDone { .. }
            | DomainEvent::OwnerCancelled { .. }
            | DomainEvent::OrderReset { .. } => None,
            _ => None,
        };

        let result = sqlx::query(
            "INSERT INTO order_events \
                 (aggregate_id, sequence, branch_id, event_type, worker_id) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(aggregate_id.into_inner())
        .bind(sequence)
        .bind(branch_id.into_inner())
        .bind(variant.as_sql())
        .bind(worker_id)
        .execute(&self.pool)
        .await;

        self.map_conflict(result, sequence)
    }

    async fn append_supply_request_event(
        &self,
        branch_id: BranchId,
        sequence: i64,
        variant: DomainEventVariant,
        event: &DomainEvent,
    ) -> Result<(), AppendError> {
        let aggregate_id = event
            .supply_request_id()
            .expect("SupplyRequest-aggregate variant must carry supply_request_id");

        let (supplier_id, invoice_id) = match event {
            DomainEvent::InvoiceReceived { supplier_id, invoice_id, .. } => {
                (Some(supplier_id.into_inner()), Some(invoice_id.into_inner()))
            }
            DomainEvent::SupplierConfirmed { supplier_id, invoice_id, .. } => {
                (Some(supplier_id.into_inner()), Some(invoice_id.into_inner()))
            }
            DomainEvent::InvoiceApproved { invoice_id, .. } => {
                (None, Some(invoice_id.into_inner()))
            }
            _ => (None, None),
        };

        let result = sqlx::query(
            "INSERT INTO supply_request_events \
                 (aggregate_id, sequence, branch_id, event_type, supplier_id, invoice_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(aggregate_id.into_inner())
        .bind(sequence)
        .bind(branch_id.into_inner())
        .bind(variant.as_sql())
        .bind(supplier_id)
        .bind(invoice_id)
        .execute(&self.pool)
        .await;

        self.map_conflict(result, sequence)
    }

    fn map_conflict(
        &self,
        result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>,
        sequence: i64,
    ) -> Result<(), AppendError> {
        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                Err(AppendError::Conflict(sequence))
            }
            Err(e) => Err(AppendError::Database(e)),
        }
    }
}
