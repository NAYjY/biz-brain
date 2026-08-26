//! T01: per-branch configuration reads.
//! Currently only covers `ai_provider`; extend here for future branch-level settings.

use sqlx::PgPool;
use uuid::Uuid;

pub struct BranchConfigRepository {
    pool: PgPool,
}

impl BranchConfigRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns the raw `ai_provider` string for a branch ("claude" | "gemini").
    /// Falls back to "claude" on any error so a missing/NULL row is safe.
    pub async fn ai_provider(&self, branch_id: Uuid) -> String {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT ai_provider FROM branches WHERE id = $1")
                .bind(branch_id)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None);

        row.map(|(p,)| p).unwrap_or_else(|| "claude".to_string())
    }

    /// Update the ai_provider for a branch. Called by the PATCH endpoint.
    pub async fn set_ai_provider(
        &self,
        branch_id: Uuid,
        provider: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE branches SET ai_provider = $1 WHERE id = $2")
            .bind(provider)
            .bind(branch_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}