//! Order aggregate — closed enum state, runtime-validated transitions (T01).
//!
//! P04/P15: OwnerCancelled, OrderReset, ClarificationResolved.
//! P16: Owner force-state transitions — Owner can move to any non-Done state
//!      from any non-Done state (bypasses Worker messaging). Done is still
//!      terminal and requires the explicit Close command.

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
            Self::Unassigned           => "UNASSIGNED",
            Self::Assigned             => "ASSIGNED",
            Self::Accepted             => "ACCEPTED",
            Self::PendingClarification => "PENDING_CLARIFICATION",
            Self::Unavailable          => "UNAVAILABLE",
            Self::ReadyForPickup       => "READY_FOR_PICKUP",
            Self::Done                 => "DONE",
            Self::Cancelled            => "CANCELLED",
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

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid Order transition: {from} -> {to}")]
pub struct TransitionError {
    pub from: OrderState,
    pub to: OrderState,
}

impl Order {
    pub fn new(
        id: OrderId,
        branch_id: BranchId,
        customer_id: CustomerId,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id,
            branch_id,
            customer_id,
            description: description.into(),
            state: OrderState::Unassigned,
            created_at: Utc::now(),
        }
    }

    pub fn transition(&mut self, to: OrderState) -> Result<(), TransitionError> {
        if !Self::is_valid(self.state, to) {
            return Err(TransitionError { from: self.state, to });
        }
        self.state = to;
        Ok(())
    }

    /// P16: Owner-bypass transition — any non-Done state → any non-Done state.
    /// Done is always terminal; Cancelled can only be reached via explicit cancel.
    pub fn owner_force(&mut self, to: OrderState) -> Result<(), TransitionError> {
        if self.state == OrderState::Done {
            return Err(TransitionError { from: self.state, to });
        }
        self.state = to;
        Ok(())
    }

    fn is_valid(from: OrderState, to: OrderState) -> bool {
        use OrderState::*;
        match from {
            Unassigned           => matches!(to, Assigned | Cancelled),
            Assigned             => matches!(to, Accepted | Unavailable | PendingClarification | Cancelled),
            Accepted             => matches!(to, ReadyForPickup | Cancelled),
            PendingClarification => matches!(to, Assigned | Cancelled),
            Unavailable          => matches!(to, Unassigned | Cancelled),
            ReadyForPickup       => matches!(to, Done | Cancelled),
            Cancelled            => matches!(to, Unassigned),
            Done                 => false,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, OrderState::Done)
    }
}
