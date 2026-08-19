//! Domain Events — one closed enum (T01). Exhaustive `match` everywhere an
//! event is consumed: store's serialization, EventHandler fan-out, and
//! agent's output contract.
//!
//! P04/P15: three new Owner-command events added —
//!   OwnerCancelled, OrderReset, ClarificationResolved.
//! P16: five new Owner force-state events —
//!   OwnerForceAccepted, OwnerForceUnavailable, OwnerForceClarification,
//!   OwnerForceReady, OwnerReassignWorker.

use serde::{Deserialize, Serialize};

use crate::{BranchId, InvoiceId, OrderId, SupplierId, SupplyRequestId, WorkerId};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DomainEvent {
    // ── Worker / Order events ─────────────────────────────────────── //
    WorkerAssigned { worker_id: WorkerId, order_id: OrderId },
    WorkerAccepted { worker_id: WorkerId, order_id: OrderId },
    WorkerUnavailable { worker_id: WorkerId, order_id: OrderId },
    WorkerCancelled { worker_id: WorkerId, order_id: OrderId },
    ClarificationRequested { worker_id: WorkerId, order_id: OrderId },
    WorkerReadyForPickup { worker_id: WorkerId, order_id: OrderId },
    OrderDone { order_id: OrderId },

    // P04: Owner-dashboard command events (never Agent-originated).
    OwnerCancelled { order_id: OrderId },
    OrderReset { order_id: OrderId },
    ClarificationResolved { worker_id: WorkerId, order_id: OrderId },

    // P16: Owner full-control force-state events.
    /// Owner forces Accepted (Worker verbally confirmed, no message needed).
    OwnerForceAccepted { order_id: OrderId },
    /// Owner marks Worker as unavailable for this order.
    OwnerForceUnavailable { worker_id: WorkerId, order_id: OrderId },
    /// Owner manually opens a clarification request.
    OwnerForceClarification { order_id: OrderId },
    /// Owner marks the order as ready for pickup.
    OwnerForceReady { order_id: OrderId },
    /// Owner swaps the assigned Worker (no cancel+reset cycle required).
    OwnerReassignWorker { new_worker_id: WorkerId, order_id: OrderId },

    // ── Supplier / SupplyRequest events ──────────────────────────── //
    SupplyRequestSent { supply_request_id: SupplyRequestId, branch_id: BranchId },
    InvoiceReceived { supplier_id: SupplierId, supply_request_id: SupplyRequestId, invoice_id: InvoiceId },
    InvoiceApproved { invoice_id: InvoiceId, branch_id: BranchId },
    SupplierConfirmed { supplier_id: SupplierId, supply_request_id: SupplyRequestId, invoice_id: InvoiceId },
}

impl DomainEvent {
    pub fn variant(&self) -> DomainEventVariant {
        match self {
            Self::WorkerAssigned { .. }          => DomainEventVariant::WorkerAssigned,
            Self::WorkerAccepted { .. }          => DomainEventVariant::WorkerAccepted,
            Self::WorkerUnavailable { .. }       => DomainEventVariant::WorkerUnavailable,
            Self::WorkerCancelled { .. }         => DomainEventVariant::WorkerCancelled,
            Self::ClarificationRequested { .. }  => DomainEventVariant::ClarificationRequested,
            Self::WorkerReadyForPickup { .. }    => DomainEventVariant::WorkerReadyForPickup,
            Self::OrderDone { .. }               => DomainEventVariant::OrderDone,
            Self::OwnerCancelled { .. }          => DomainEventVariant::OwnerCancelled,
            Self::OrderReset { .. }              => DomainEventVariant::OrderReset,
            Self::ClarificationResolved { .. }   => DomainEventVariant::ClarificationResolved,
            Self::OwnerForceAccepted { .. }      => DomainEventVariant::OwnerForceAccepted,
            Self::OwnerForceUnavailable { .. }   => DomainEventVariant::OwnerForceUnavailable,
            Self::OwnerForceClarification { .. } => DomainEventVariant::OwnerForceClarification,
            Self::OwnerForceReady { .. }         => DomainEventVariant::OwnerForceReady,
            Self::OwnerReassignWorker { .. }     => DomainEventVariant::OwnerReassignWorker,
            Self::SupplyRequestSent { .. }       => DomainEventVariant::SupplyRequestSent,
            Self::InvoiceReceived { .. }         => DomainEventVariant::InvoiceReceived,
            Self::InvoiceApproved { .. }         => DomainEventVariant::InvoiceApproved,
            Self::SupplierConfirmed { .. }       => DomainEventVariant::SupplierConfirmed,
        }
    }

    pub fn order_id(&self) -> Option<OrderId> {
        match self {
            Self::WorkerAssigned { order_id, .. }
            | Self::WorkerAccepted { order_id, .. }
            | Self::WorkerUnavailable { order_id, .. }
            | Self::WorkerCancelled { order_id, .. }
            | Self::ClarificationRequested { order_id, .. }
            | Self::WorkerReadyForPickup { order_id, .. }
            | Self::OrderDone { order_id }
            | Self::OwnerCancelled { order_id }
            | Self::OrderReset { order_id }
            | Self::ClarificationResolved { order_id, .. }
            | Self::OwnerForceAccepted { order_id }
            | Self::OwnerForceClarification { order_id }
            | Self::OwnerForceReady { order_id }
            | Self::OwnerForceUnavailable { order_id, .. }
            | Self::OwnerReassignWorker { order_id, .. } => Some(*order_id),
            _ => None,
        }
    }

    pub fn supply_request_id(&self) -> Option<SupplyRequestId> {
        match self {
            Self::SupplyRequestSent { supply_request_id, .. }
            | Self::InvoiceReceived { supply_request_id, .. }
            | Self::SupplierConfirmed { supply_request_id, .. } => Some(*supply_request_id),
            _ => None,
        }
    }

    pub fn is_terminal_for_worker(&self) -> bool {
        matches!(
            self,
            Self::OrderDone { .. }
                | Self::WorkerUnavailable { .. }
                | Self::WorkerCancelled { .. }
                | Self::OwnerCancelled { .. }
                | Self::OwnerForceUnavailable { .. }
        )
    }
}

impl std::fmt::Display for DomainEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.variant())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DomainEventVariant {
    WorkerAssigned,
    WorkerAccepted,
    WorkerUnavailable,
    WorkerCancelled,
    ClarificationRequested,
    WorkerReadyForPickup,
    OrderDone,
    // P04
    OwnerCancelled,
    OrderReset,
    ClarificationResolved,
    // P16
    OwnerForceAccepted,
    OwnerForceUnavailable,
    OwnerForceClarification,
    OwnerForceReady,
    OwnerReassignWorker,
    // Supplier
    SupplyRequestSent,
    InvoiceReceived,
    InvoiceApproved,
    SupplierConfirmed,
}

impl DomainEventVariant {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::WorkerAssigned         => "worker_assigned",
            Self::WorkerAccepted         => "worker_accepted",
            Self::WorkerUnavailable      => "worker_unavailable",
            Self::WorkerCancelled        => "worker_cancelled",
            Self::ClarificationRequested => "clarification_requested",
            Self::WorkerReadyForPickup   => "worker_ready_for_pickup",
            Self::OrderDone              => "order_done",
            Self::OwnerCancelled         => "owner_cancelled",
            Self::OrderReset             => "order_reset",
            Self::ClarificationResolved  => "clarification_resolved",
            Self::OwnerForceAccepted     => "owner_force_accepted",
            Self::OwnerForceUnavailable  => "owner_force_unavailable",
            Self::OwnerForceClarification=> "owner_force_clarification",
            Self::OwnerForceReady        => "owner_force_ready",
            Self::OwnerReassignWorker    => "owner_reassign_worker",
            Self::SupplyRequestSent      => "supply_request_sent",
            Self::InvoiceReceived        => "invoice_received",
            Self::InvoiceApproved        => "invoice_approved",
            Self::SupplierConfirmed      => "supplier_confirmed",
        }
    }

    pub fn aggregate(&self) -> Aggregate {
        match self {
            Self::WorkerAssigned
            | Self::WorkerAccepted
            | Self::WorkerUnavailable
            | Self::WorkerCancelled
            | Self::ClarificationRequested
            | Self::WorkerReadyForPickup
            | Self::OrderDone
            | Self::OwnerCancelled
            | Self::OrderReset
            | Self::ClarificationResolved
            | Self::OwnerForceAccepted
            | Self::OwnerForceUnavailable
            | Self::OwnerForceClarification
            | Self::OwnerForceReady
            | Self::OwnerReassignWorker => Aggregate::Order,

            Self::SupplyRequestSent
            | Self::InvoiceReceived
            | Self::InvoiceApproved
            | Self::SupplierConfirmed => Aggregate::SupplyRequest,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggregate {
    Order,
    SupplyRequest,
}
