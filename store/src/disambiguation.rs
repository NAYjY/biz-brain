//! P02: disambiguation_pending — stores in-progress ranked yes/no flows.
//! One row per sender; advance_index moves to the next candidate on "no".

use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct DisambiguationRow {
    pub id: Uuid,
    pub sender_key: String,
    pub original_text: String,
    pub candidate_ids: serde_json::Value,
    pub candidate_index: i32,
    pub aggregate_type: String,
}

impl DisambiguationRow {
    pub fn candidates(&self) -> Vec<Uuid> {
        self.candidate_ids
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str()?.parse().ok())
            .collect()
    }

    /// The candidate currently being asked about.
    pub fn current_candidate(&self) -> Option<Uuid> {
        self.candidates().get(self.candidate_index as usize).copied()
    }
}

pub struct DisambiguationStore {
    pool: PgPool,
}

impl DisambiguationStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find(&self, sender_key: &str) -> Result<Option<DisambiguationRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, sender_key, original_text, candidate_ids, candidate_index, aggregate_type \
             FROM disambiguation_pending WHERE sender_key = $1",
        )
        .bind(sender_key)
        .fetch_optional(&self.pool)
        .await
    }

    /// Create a new disambiguation flow for this sender.
    pub async fn create(
        &self,
        sender_key: &str,
        original_text: &str,
        candidate_ids: &[Uuid],
        aggregate_type: &str,
    ) -> Result<(), sqlx::Error> {
        let ids_json = serde_json::to_value(
            candidate_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
        )
        .unwrap_or_default();

        sqlx::query(
            "INSERT INTO disambiguation_pending \
                 (sender_key, original_text, candidate_ids, candidate_index, aggregate_type) \
             VALUES ($1, $2, $3, 0, $4) \
             ON CONFLICT (sender_key) DO UPDATE \
                 SET original_text   = EXCLUDED.original_text, \
                     candidate_ids   = EXCLUDED.candidate_ids, \
                     candidate_index = 0, \
                     aggregate_type  = EXCLUDED.aggregate_type, \
                     created_at      = NOW()",
        )
        .bind(sender_key)
        .bind(original_text)
        .bind(ids_json)
        .bind(aggregate_type)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Advance to the next candidate (Worker replied "no").
    pub async fn advance(&self, sender_key: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE disambiguation_pending \
             SET candidate_index = candidate_index + 1 \
             WHERE sender_key = $1",
        )
        .bind(sender_key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove the pending row (Worker confirmed or all candidates exhausted).
    pub async fn delete(&self, sender_key: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM disambiguation_pending WHERE sender_key = $1")
            .bind(sender_key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
