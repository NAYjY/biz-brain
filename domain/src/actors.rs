//! Worker and Supplier — the two non-Owner actors who communicate via a
//! messaging Channel. Kept minimal: no invented verification/availability
//! state machine here — that's out of scope of any resolved ticket.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::channel::ChannelIdentity;
use crate::{BranchId, SupplierId, WorkerId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worker {
    pub id: WorkerId,
    pub branch_id: BranchId,
    pub name: String,
    pub identity: ChannelIdentity,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Supplier {
    pub id: SupplierId,
    pub branch_id: BranchId,
    pub name: String,
    pub identity: ChannelIdentity,
    pub created_at: DateTime<Utc>,
}
