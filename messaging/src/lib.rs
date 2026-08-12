//! Messaging crate (T04): LINE + WhatsApp webhook ingestion and outbound
//! messaging behind a shared `ChannelAdapter` trait.

#![warn(clippy::all)]

pub mod channel_trait;
pub mod inbound;
pub mod line;
pub mod whatsapp;
pub mod telegram;

pub use channel_trait::{http_like::Headers, ChannelAdapter, ChannelError, InboundMessage};
pub use line::LineAdapter;
pub use whatsapp::WhatsAppAdapter;
pub use telegram::TelegramAdapter;