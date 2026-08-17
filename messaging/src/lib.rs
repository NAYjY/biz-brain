//! Messaging crate (T04 / P05): LINE + WhatsApp + Telegram webhook ingestion
//! and outbound messaging behind a shared `ChannelAdapter` trait.
//!
//! P05: `fetch_media` added to `ChannelAdapter`; WhatsApp implements it,
//! LINE and Telegram stub with `MediaFetchUnsupported`.

#![warn(clippy::all)]

pub mod channel_trait;
pub mod inbound;
pub mod line;
pub mod telegram;
pub mod whatsapp;

pub use channel_trait::{
    http_like::Headers, ChannelAdapter, ChannelError, InboundMessage, MediaBlob,
};
pub use line::LineAdapter;
pub use telegram::TelegramAdapter;
pub use whatsapp::WhatsAppAdapter;
