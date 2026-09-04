//! Store crate (T02 / P01 / P02 / P13 / F05 / T09): event-sourced schema, async
//! projections, conversation history, disambiguation state, reply templates,
//! follow-up alerts, cursor-based pagination.

#![warn(clippy::all)]

pub mod actor_directory;
pub mod alerts;                         // F05
pub mod conversation_history;
pub mod disambiguation;
pub mod event_sourcing;
pub mod order_events;
pub mod projection_tables;
pub mod projection_tables_paginated;    // T09
pub mod projection_worker;
pub mod reply_templates;
pub mod supply_request_events;
pub mod webhook_inbox;
pub mod branch_config;

pub use branch_config::BranchConfigRepository;
pub use actor_directory::{ActorDirectory, PendingBinding};
pub use alerts::{AlertMode, AlertRepository, AlertView};  // F05
pub use conversation_history::{ConversationHistoryRepository, HistoryRow};
pub use disambiguation::{
        DisambiguationRow,
        DisambiguationStore,
        DisambiguationUpdate,
    };
pub use event_sourcing::{AppendError, EventSourcing};
pub use order_events::OrderEventRepository;
pub use projection_tables::ProjectionTables;
pub use projection_tables_paginated::{   // T09
    OrderCursor, OrderFilter,
    PaginatedProjections,
    SrCursor, SrFilter, SrPageRow,
    PAGE_SIZE,
};
pub use projection_worker::ProjectionWorker;
pub use reply_templates::ReplyTemplateRepository;
pub use supply_request_events::SupplyRequestEventRepository;
pub use webhook_inbox::WebhookInbox;