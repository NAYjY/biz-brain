//! Domain crate (T01): pure types, Events, state machines. No I/O, no async.

#![warn(clippy::all)]

mod actors;
mod agent;
mod assignment;
pub mod channel;
mod domain_event;
mod ids;
mod invoice;
mod order;
pub mod sse_event;
mod supply_request;

pub use actors::{Supplier, Worker};
pub use agent::{Agent, AgentName};
pub use assignment::{Assignment, AssignmentError, AssignmentState};
pub use channel::{Channel, ChannelIdentity};
pub use domain_event::{Aggregate, DomainEvent, DomainEventVariant};
pub use ids::{
    AgentId, AssignmentId, BranchId, CustomerId, InvoiceId, OrderId, OwnerId, SupplierId,
    SupplyRequestId, WorkerId,
};
pub use invoice::{Invoice, InvoiceItem, InvoiceState, InvoiceTransitionError};
pub use order::{Order, OrderState, TransitionError as OrderTransitionError};
pub use sse_event::SseSignal;
pub use supply_request::{SupplyRequest, SupplyRequestState, TransitionError as SupplyRequestTransitionError};

/// Assignments + SupplyRequests relevant to an Order — carried alongside the
/// state machine when the caller needs cross-aggregate context (not stored
/// on Order itself; Assignment/SupplyRequest are separate aggregates, T01).
#[derive(Debug, Clone, Default)]
pub struct OrderContext {
    pub assignments: Vec<Assignment>,
    pub supply_requests: Vec<SupplyRequest>,
}
