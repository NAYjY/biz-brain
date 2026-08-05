//! Invoice — separate aggregate referencing SupplyRequestId (T01).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{BranchId, InvoiceId, SupplierId, SupplyRequestId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvoiceState {
    Pending,
    Sent,
    Approved,
    Confirmed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceItem {
    pub sku: String,
    pub quantity: u32,
    pub unit_price_cents: i64,
}

impl InvoiceItem {
    pub fn total_cents(&self) -> i64 {
        self.quantity as i64 * self.unit_price_cents
    }
}

#[derive(Debug, Clone)]
pub struct Invoice {
    pub id: InvoiceId,
    pub supplier_id: SupplierId,
    pub supply_request_id: SupplyRequestId,
    pub branch_id: BranchId,
    pub items: Vec<InvoiceItem>,
    pub state: InvoiceState,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("cannot transition Invoice out of {0:?}")]
pub struct InvoiceTransitionError(pub InvoiceState);

impl Invoice {
    pub fn new(
        id: InvoiceId,
        supplier_id: SupplierId,
        supply_request_id: SupplyRequestId,
        branch_id: BranchId,
        items: Vec<InvoiceItem>,
    ) -> Self {
        Self { id, supplier_id, supply_request_id, branch_id, items, state: InvoiceState::Pending, created_at: Utc::now() }
    }

    pub fn total_cents(&self) -> i64 {
        self.items.iter().map(InvoiceItem::total_cents).sum()
    }

    pub fn approve(&mut self) -> Result<(), InvoiceTransitionError> {
        if self.state != InvoiceState::Sent {
            return Err(InvoiceTransitionError(self.state));
        }
        self.state = InvoiceState::Approved;
        Ok(())
    }

    pub fn confirm(&mut self) -> Result<(), InvoiceTransitionError> {
        if self.state != InvoiceState::Approved {
            return Err(InvoiceTransitionError(self.state));
        }
        self.state = InvoiceState::Confirmed;
        Ok(())
    }
}
