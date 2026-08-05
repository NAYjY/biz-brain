//! Order aggregate — closed enum state, runtime-validated transitions (T01).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{BranchId, CustomerId, OrderId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderState {
    Unassigned,
    Assigned,
    Accepted,
    PendingClarification,
    Unavailable,
    ReadyForPickup,
    Done,
    Cancelled,
}

impl std::fmt::Display for OrderState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Unassigned => "UNASSIGNED",
            Self::Assigned => "ASSIGNED",
            Self::Accepted => "ACCEPTED",
            Self::PendingClarification => "PENDING_CLARIFICATION",
            Self::Unavailable => "UNAVAILABLE",
            Self::ReadyForPickup => "READY_FOR_PICKUP",
            Self::Done => "DONE",
            Self::Cancelled => "CANCELLED",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: OrderId,
    pub branch_id: BranchId,
    pub customer_id: CustomerId,
    pub description: String,
    pub state: OrderState,
    pub created_at: DateTime<Utc>,
}

/// Error attempting an illegal state transition.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid Order transition: {from} -> {to}")]
pub struct TransitionError {
    pub from: OrderState,
    pub to: OrderState,
}

impl Order {
    pub fn new(id: OrderId, branch_id: BranchId, customer_id: CustomerId, description: impl Into<String>) -> Self {
        Self {
            id,
            branch_id,
            customer_id,
            description: description.into(),
            state: OrderState::Unassigned,
            created_at: Utc::now(),
        }
    }

    /// Validates and applies a transition. Legality lives here; the *meaning*
    /// difference between e.g. Cancelled/Unavailable (both -> Unassigned)
    /// lives in which DomainEvent drove the call, not in the resulting state.
    pub fn transition(&mut self, to: OrderState) -> Result<(), TransitionError> {
        if !Self::is_valid(self.state, to) {
            return Err(TransitionError { from: self.state, to });
        }
        self.state = to;
        Ok(())
    }

    fn is_valid(from: OrderState, to: OrderState) -> bool {
        use OrderState::*;
        match from {
            Unassigned => matches!(to, Assigned),
            Assigned => matches!(to, Accepted | Unavailable | PendingClarification),
            Accepted => matches!(to, ReadyForPickup | Cancelled),
            PendingClarification => matches!(to, Assigned | Cancelled),
            Unavailable => matches!(to, Unassigned),
            ReadyForPickup => matches!(to, Done | Cancelled),
            Done => false,
            Cancelled => matches!(to, Unassigned),
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, OrderState::Done)
    }
}
