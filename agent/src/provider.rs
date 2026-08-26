//! T01: provider abstraction.
//!
//! `ClassifyClient` is the single interface both `ClaudeClassifier` and
//! `GeminiClassifier` implement. The correct one is chosen per-branch at
//! inbox_worker dispatch time based on `branches.ai_provider`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::classify::{ActiveOrderContext, HistoryMessage};
use crate::outcome::InterpretationError;
use domain::DomainEventVariant;

/// Which AI backend a branch uses for message classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    #[default]
    Claude,
    Gemini,
}

impl AiProvider {
    pub fn from_sql(s: &str) -> Self {
        match s { "gemini" => Self::Gemini, _ => Self::Claude }
    }

    pub fn as_sql(self) -> &'static str {
        match self { Self::Claude => "claude", Self::Gemini => "gemini" }
    }
}

/// Shared classify contract implemented by both backends.
#[async_trait]
pub trait ClassifyClient: Send + Sync {
    async fn classify_worker(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_orders: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError>;

    async fn classify_supplier(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_supply_requests: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError>;
}