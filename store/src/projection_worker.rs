//! T02: async projection worker. Reconstructs current state from a
//! replayed stream and writes it to the projection tables. Also — per T07's
//! resolution — is the point that fires the SSE invalidation signal, so a
//! subsequent GET is guaranteed consistent with what the client re-fetches.

use domain::{DomainEvent, OrderId, OrderState, SseSignal, SupplyRequestId, SupplyRequestState};
use sqlx::PgPool;

use crate::order_events::{OrderEventRepository, ReadError};
use crate::projection_tables::ProjectionTables;
use crate::supply_request_events::SupplyRequestEventRepository;

pub struct ProjectionWorker {
    order_events: OrderEventRepository,
    supply_request_events: SupplyRequestEventRepository,
    projections: ProjectionTables,
    pool: PgPool,
}

impl ProjectionWorker {
    pub fn new(pool: PgPool) -> Self {
        Self {
            order_events: OrderEventRepository::new(pool.clone()),
            supply_request_events: SupplyRequestEventRepository::new(pool.clone()),
            projections: ProjectionTables::new(pool.clone()),
            pool,
        }
    }

    /// Replay `order_id`'s stream, derive current OrderState, write the
    /// projection, and return the T07 invalidation signal to publish.
    pub async fn project_order(&self, order_id: OrderId) -> Result<SseSignal, ReadError> {
        let events = self.order_events.load_stream(order_id).await?;
    
        let state = events.iter().rev().find_map(Self::order_state_of)
            .unwrap_or(OrderState::Unassigned);
    
        // Extract worker_id from most recent worker-bearing event
        let worker_id = events.iter().rev().find_map(|e| match e {
            DomainEvent::WorkerAssigned { worker_id, .. }
            | DomainEvent::WorkerAccepted { worker_id, .. }
            | DomainEvent::WorkerUnavailable { worker_id, .. }
            | DomainEvent::WorkerCancelled { worker_id, .. }
            | DomainEvent::ClarificationRequested { worker_id, .. }
            | DomainEvent::WorkerReadyForPickup { worker_id, .. } => Some(worker_id.into_inner()),
            _ => None,
        });
    
        // Clear worker_id if order returned to Unassigned
        let worker_id = match state {
            OrderState::Unassigned => None,
            _ => worker_id,
        };
    
        let meta: (uuid::Uuid, uuid::Uuid, String) =
            sqlx::query_as("SELECT branch_id, customer_id, description FROM orders WHERE id = $1")
                .bind(order_id.into_inner())
                .fetch_one(&self.pool)
                .await?;
    
        self.projections
            .upsert_order_state(order_id, meta.0, meta.1, &meta.2, state, worker_id)
            .await?;
    
        Ok(SseSignal::OrderChanged { order_id, branch_id: domain::BranchId::new(meta.0) })
    }

    pub async fn project_supply_request(&self, id: SupplyRequestId) -> Result<SseSignal, ReadError> {
        let events = self.supply_request_events.load_stream(id).await?;
        let state = events.iter().rev().find_map(Self::supply_request_state_of).unwrap_or(SupplyRequestState::Draft);

        let meta: (uuid::Uuid, String) = sqlx::query_as("SELECT branch_id, description FROM supply_requests WHERE id = $1")
            .bind(id.into_inner())
            .fetch_one(&self.pool)
            .await?;

        self.projections.upsert_supply_request_state(id, meta.0, &meta.1, state).await?;

        Ok(SseSignal::SupplyRequestChanged { supply_request_id: id, branch_id: domain::BranchId::new(meta.0) })
    }

    fn order_state_of(event: &DomainEvent) -> Option<OrderState> {
        Some(match event {
            DomainEvent::WorkerAssigned { .. } => OrderState::Assigned,
            DomainEvent::WorkerAccepted { .. } => OrderState::Accepted,
            DomainEvent::WorkerUnavailable { .. } => OrderState::Unavailable,
            DomainEvent::WorkerCancelled { .. } => OrderState::Cancelled,
            DomainEvent::ClarificationRequested { .. } => OrderState::PendingClarification,
            DomainEvent::WorkerReadyForPickup { .. } => OrderState::ReadyForPickup,
            DomainEvent::OrderDone { .. } => OrderState::Done,
            _ => return None,
        })
    }

    fn supply_request_state_of(event: &DomainEvent) -> Option<SupplyRequestState> {
        Some(match event {
            DomainEvent::SupplyRequestSent { .. } => SupplyRequestState::Sent,
            DomainEvent::InvoiceReceived { .. } => SupplyRequestState::InvoiceReceived,
            DomainEvent::InvoiceApproved { .. } => SupplyRequestState::OwnerApprovedInvoice,
            DomainEvent::SupplierConfirmed { .. } => SupplyRequestState::SupplierConfirmed,
            _ => return None,
        })
    }
}
