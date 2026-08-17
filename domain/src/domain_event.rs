//! Domain Events — one closed enum (T01). Exhaustive `match` everywhere an
//! event is consumed: store's serialization, EventHandler fan-out, and
//! agent's output contract.
//!
//! P04/P15: three new Owner-command events added —
//!   OwnerCancelled, OrderReset, ClarificationResolved.

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
    /// Owner explicitly cancels any non-Done order.
    OwnerCancelled { order_id: OrderId },
    /// Owner resets a Cancelled or Unavailable order back to Unassigned.
    OrderReset { order_id: OrderId },
    /// Owner resolves a worker's clarification request (re-assigns to Accepted).
    ClarificationResolved { worker_id: WorkerId, order_id: OrderId },

    // ── Supplier / SupplyRequest events ──────────────────────────── //
    SupplyRequestSent { supply_request_id: SupplyRequestId, branch_id: BranchId },
    InvoiceReceived { supplier_id: SupplierId, supply_request_id: SupplyRequestId, invoice_id: InvoiceId },
    InvoiceApproved { invoice_id: InvoiceId, branch_id: BranchId },
    SupplierConfirmed { supplier_id: SupplierId, supply_request_id: SupplyRequestId, invoice_id: InvoiceId },
}

impl DomainEvent {
    pub fn variant(&self) -> DomainEventVariant {
        match self {
            Self::WorkerAssigned { .. }        => DomainEventVariant::WorkerAssigned,
            Self::WorkerAccepted { .. }        => DomainEventVariant::WorkerAccepted,
            Self::WorkerUnavailable { .. }     => DomainEventVariant::WorkerUnavailable,
            Self::WorkerCancelled { .. }       => DomainEventVariant::WorkerCancelled,
            Self::ClarificationRequested { .. }=> DomainEventVariant::ClarificationRequested,
            Self::WorkerReadyForPickup { .. }  => DomainEventVariant::WorkerReadyForPickup,
            Self::OrderDone { .. }             => DomainEventVariant::OrderDone,
            Self::OwnerCancelled { .. }        => DomainEventVariant::OwnerCancelled,
            Self::OrderReset { .. }            => DomainEventVariant::OrderReset,
            Self::ClarificationResolved { .. } => DomainEventVariant::ClarificationResolved,
            Self::SupplyRequestSent { .. }     => DomainEventVariant::SupplyRequestSent,
            Self::InvoiceReceived { .. }       => DomainEventVariant::InvoiceReceived,
            Self::InvoiceApproved { .. }       => DomainEventVariant::InvoiceApproved,
            Self::SupplierConfirmed { .. }     => DomainEventVariant::SupplierConfirmed,
        }
    }

    /// The Order this event pertains to, if any (Order-aggregate events only).
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
            | Self::ClarificationResolved { order_id, .. } => Some(*order_id),
            _ => None,
        }
    }

    /// The SupplyRequest this event pertains to, if any.
    pub fn supply_request_id(&self) -> Option<SupplyRequestId> {
        match self {
            Self::SupplyRequestSent { supply_request_id, .. }
            | Self::InvoiceReceived { supply_request_id, .. }
            | Self::SupplierConfirmed { supply_request_id, .. } => Some(*supply_request_id),
            _ => None,
        }
    }

    /// Whether this event terminates a Worker's active assignment on the order.
    /// Used by inbox_worker (P07) to remove the order from ThreadContextStore.
    pub fn is_terminal_for_worker(&self) -> bool {
        matches!(
            self,
            Self::OrderDone { .. }
                | Self::WorkerUnavailable { .. }
                | Self::WorkerCancelled { .. }
                | Self::OwnerCancelled { .. }
        )
    }
}

impl std::fmt::Display for DomainEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.variant())
    }
}

/// Discriminant-only view of `DomainEvent` — for CHECK-constrained `event_type`
/// columns (store crate) and command-endpoint routing (api crate).
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
    // Supplier
    SupplyRequestSent,
    InvoiceReceived,
    InvoiceApproved,
    SupplierConfirmed,
}

impl DomainEventVariant {
    /// String form matching the SQL CHECK constraint values in store migrations.
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::WorkerAssigned        => "worker_assigned",
            Self::WorkerAccepted        => "worker_accepted",
            Self::WorkerUnavailable     => "worker_unavailable",
            Self::WorkerCancelled       => "worker_cancelled",
            Self::ClarificationRequested=> "clarification_requested",
            Self::WorkerReadyForPickup  => "worker_ready_for_pickup",
            Self::OrderDone             => "order_done",
            Self::OwnerCancelled        => "owner_cancelled",
            Self::OrderReset            => "order_reset",
            Self::ClarificationResolved => "clarification_resolved",
            Self::SupplyRequestSent     => "supply_request_sent",
            Self::InvoiceReceived       => "invoice_received",
            Self::InvoiceApproved       => "invoice_approved",
            Self::SupplierConfirmed     => "supplier_confirmed",
        }
    }

    /// Which event table (T02: two separate tables) this variant belongs to.
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
            | Self::ClarificationResolved => Aggregate::Order,

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
