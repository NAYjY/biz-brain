//! Store crate (T02): event-sourced schema, async projections.
//! Also owns T04's webhook_inbox (pre-domain dedup/durability).
//! S06: actor_directory module replaces the old actors.rs.

#![warn(clippy::all)]

pub mod actor_directory;
pub mod event_sourcing;
pub mod order_events;
pub mod projection_tables;
pub mod projection_worker;
pub mod supply_request_events;
pub mod webhook_inbox;

pub use actor_directory::{ActorDirectory, PendingBinding};
pub use event_sourcing::{AppendError, EventSourcing};
pub use order_events::OrderEventRepository;
pub use projection_tables::ProjectionTables;
pub use projection_worker::ProjectionWorker;
pub use supply_request_events::SupplyRequestEventRepository;
pub use webhook_inbox::WebhookInbox;