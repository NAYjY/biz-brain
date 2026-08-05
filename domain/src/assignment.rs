//! Assignment — separate aggregate binding Worker to Order (T01).
//! Does not exist until the Owner commits.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AssignmentId, OrderId, WorkerId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssignmentState {
    Notified,
    Accepted,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Assignment {
    pub id: AssignmentId,
    pub order_id: OrderId,
    pub worker_id: WorkerId,
    pub state: AssignmentState,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("cannot transition Assignment out of {0:?}")]
pub struct AssignmentError(pub AssignmentState);

impl Assignment {
    pub fn new(id: AssignmentId, order_id: OrderId, worker_id: WorkerId) -> Self {
        Self { id, order_id, worker_id, state: AssignmentState::Notified, created_at: Utc::now() }
    }

    pub fn accept(&mut self) -> Result<(), AssignmentError> {
        if self.state != AssignmentState::Notified {
            return Err(AssignmentError(self.state));
        }
        self.state = AssignmentState::Accepted;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), AssignmentError> {
        if self.state != AssignmentState::Notified {
            return Err(AssignmentError(self.state));
        }
        self.state = AssignmentState::Cancelled;
        Ok(())
    }
}
