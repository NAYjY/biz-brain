//! Store crate (T02 / P01 / P02 / P13): event-sourced schema, async
//! projections, conversation history, disambiguation state, reply templates.

#![warn(clippy::all)]

pub mod actor_directory;
pub mod conversation_history;
pub mod disambiguation;
pub mod event_sourcing;
pub mod order_events;
pub mod projection_tables;
pub mod projection_worker;
pub mod reply_templates;
pub mod supply_request_events;
pub mod webhook_inbox;

pub use actor_directory::{ActorDirectory, PendingBinding};
pub use conversation_history::ConversationHistoryRepository;
pub use disambiguation::DisambiguationStore;
pub use event_sourcing::{AppendError, EventSourcing};
pub use order_events::OrderEventRepository;
pub use projection_tables::ProjectionTables;
pub use projection_worker::ProjectionWorker;
pub use reply_templates::ReplyTemplateRepository;
pub use supply_request_events::SupplyRequestEventRepository;
pub use webhook_inbox::WebhookInbox;
