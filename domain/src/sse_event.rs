//! T07 resolution: SSE carries a bare invalidation signal ("this changed,
//! re-fetch"), not a raw DomainEvent or a diff. Fired off the projection
//! worker's write, so a client re-fetch after receiving this is guaranteed
//! consistent (no projection-lag race).

use serde::{Deserialize, Serialize};

use crate::{BranchId, OrderId, SupplyRequestId};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SseSignal {
    OrderChanged { order_id: OrderId, branch_id: BranchId },
    SupplyRequestChanged { supply_request_id: SupplyRequestId, branch_id: BranchId },
}

impl SseSignal {
    pub fn branch_id(&self) -> BranchId {
        match self {
            Self::OrderChanged { branch_id, .. } | Self::SupplyRequestChanged { branch_id, .. } => *branch_id,
        }
    }
}
