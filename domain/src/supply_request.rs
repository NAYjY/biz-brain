//! SupplyRequest aggregate — separate aggregate referencing OrderId/BranchId (T01).
//! Spans multiple Orders within a Branch; structurally cannot nest under Order.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{BranchId, OrderId, SupplyRequestId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupplyRequestState {
    Draft,
    Sent,
    InvoiceReceived,
    OwnerApprovedInvoice,
    SupplierConfirmed,
}

impl std::fmt::Display for SupplyRequestState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Draft => "DRAFT",
            Self::Sent => "SENT",
            Self::InvoiceReceived => "INVOICE_RECEIVED",
            Self::OwnerApprovedInvoice => "OWNER_APPROVED_INVOICE",
            Self::SupplierConfirmed => "SUPPLIER_CONFIRMED",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone)]
pub struct SupplyRequest {
    pub id: SupplyRequestId,
    pub branch_id: BranchId,
    pub order_ids: Vec<OrderId>,
    pub description: String,
    pub state: SupplyRequestState,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid SupplyRequest transition: {from} -> {to}")]
pub struct TransitionError {
    pub from: SupplyRequestState,
    pub to: SupplyRequestState,
}

impl SupplyRequest {
    pub fn new(id: SupplyRequestId, branch_id: BranchId, order_ids: Vec<OrderId>, description: impl Into<String>) -> Self {
        Self {
            id,
            branch_id,
            order_ids,
            description: description.into(),
            state: SupplyRequestState::Draft,
            created_at: Utc::now(),
        }
    }

    pub fn transition(&mut self, to: SupplyRequestState) -> Result<(), TransitionError> {
        if !Self::is_valid(self.state, to) {
            return Err(TransitionError { from: self.state, to });
        }
        self.state = to;
        Ok(())
    }

    fn is_valid(from: SupplyRequestState, to: SupplyRequestState) -> bool {
        use SupplyRequestState::*;
        matches!(
            (from, to),
            (Draft, Sent)
                | (Sent, InvoiceReceived)
                | (InvoiceReceived, OwnerApprovedInvoice)
                | (OwnerApprovedInvoice, SupplierConfirmed)
        )
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, SupplyRequestState::SupplierConfirmed)
    }
}
