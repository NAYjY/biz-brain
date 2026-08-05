//! Messaging channel — T04 resolution: `domain` owns the Channel+external-id
//! *shape* carried on Worker/Supplier; `messaging` crate owns the *lookup*
//! (mapping an inbound webhook sender to a Channel + external id).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Line,
    WhatsApp,
}

impl Channel {
    /// String form matching the `webhook_inbox.channel` CHECK constraint (store crate).
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::WhatsApp => "whats_app",
        }
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Line => write!(f, "LINE"),
            Self::WhatsApp => write!(f, "WhatsApp"),
        }
    }
}

/// A Worker/Supplier's transport identity — which channel, and their id on it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelIdentity {
    pub channel: Channel,
    pub external_id: String,
}
