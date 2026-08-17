//! Domain crate (T01 / P04 / P15): pure types, Events, state machines.
//! No I/O, no async.

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
pub use supply_request::{
    SupplyRequest, SupplyRequestState, TransitionError as SupplyRequestTransitionError,
};

/// Cross-aggregate context carried alongside an Order when the caller needs
/// Assignment/SupplyRequest data (not stored on Order itself — T01).
#[derive(Debug, Clone, Default)]
pub struct OrderContext {
    pub assignments: Vec<Assignment>,
    pub supply_requests: Vec<SupplyRequest>,
}
