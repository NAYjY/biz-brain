//! P13: per-sender conversation history.  Sliding window of 20 messages,
//! keyed on `channel:external_id`.  Oldest rows pruned on insert.
//!
//! Returns raw (role, content) pairs; callers in `api` convert to
//! `agent::classify::HistoryMessage` — no circular crate dependency.

use sqlx::PgPool;

const WINDOW_SIZE: i64 = 20;

/// A single conversation turn (role = "user" | "assistant", content = text).
#[derive(Debug, Clone)]
pub struct HistoryRow {
    pub role: String,
    pub content: String,
}

pub struct ConversationHistoryRepository {
    pool: PgPool,
}

impl ConversationHistoryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn sender_key(channel: &str, external_id: &str) -> String {
        format!("{channel}:{external_id}")
    }

    /// Append a message and prune rows beyond the sliding window.
    pub async fn append(
        &self,
        sender_key: &str,
        role: &str,
        content: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO conversation_history (sender_key, role, content) VALUES ($1, $2, $3)",
        )
        .bind(sender_key)
        .bind(role)
        .bind(content)
        .execute(&self.pool)
        .await?;

        // Prune to the most recent WINDOW_SIZE rows for this sender.
        sqlx::query(
            "DELETE FROM conversation_history \
             WHERE sender_key = $1 \
               AND id NOT IN ( \
                   SELECT id FROM conversation_history \
                   WHERE sender_key = $1 \
                   ORDER BY id DESC \
                   LIMIT $2 \
               )",
        )
        .bind(sender_key)
        .bind(WINDOW_SIZE)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Load the most recent `WINDOW_SIZE` messages for a sender, oldest first.
    pub async fn load(&self, sender_key: &str) -> Result<Vec<HistoryRow>, sqlx::Error> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT role, content FROM ( \
                 SELECT id, role, content FROM conversation_history \
                 WHERE sender_key = $1 \
                 ORDER BY id DESC LIMIT $2 \
             ) sub ORDER BY id ASC",
        )
        .bind(sender_key)
        .bind(WINDOW_SIZE)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(role, content)| HistoryRow { role, content })
            .collect())
    }
}
