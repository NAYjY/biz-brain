//! Agent — AI component scoped to a Branch (one per Branch).
//! AgentName is purely cosmetic display config, not behavior.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AgentId, BranchId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentName(pub String);

impl std::fmt::Display for AgentName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Agent {
    pub id: AgentId,
    pub branch_id: BranchId,
    pub name: AgentName,
    pub created_at: DateTime<Utc>,
}

impl Agent {
    pub fn new(id: AgentId, branch_id: BranchId, name: impl Into<String>) -> Self {
        Self { id, branch_id, name: AgentName(name.into()), created_at: Utc::now() }
    }
}
