//! T04: durable landing table for inbound LINE/WhatsApp webhooks. The
//! `messaging` crate inserts here (dedup by UNIQUE(channel, external_event_id))
//! *before* returning 200, then a worker drains unprocessed rows async.

use domain::Channel;
use serde_json::Value;
use sqlx::{FromRow, PgPool};

#[derive(Debug, FromRow)]
pub struct InboxRow {
    pub id: i64,
    pub channel: String,
    pub external_event_id: String,
    pub raw_payload: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum InboxError {
    #[error("duplicate webhook event (channel={channel}, external_event_id={external_event_id})")]
    Duplicate { channel: String, external_event_id: String },
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

pub struct WebhookInbox {
    pool: PgPool,
}

impl WebhookInbox {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert the raw payload. Returns `Err(Duplicate)` if this event was
    /// already received — the webhook handler should still ack 200 either way.
    pub async fn record(&self, channel: Channel, external_event_id: &str, raw_payload: Value) -> Result<(), InboxError> {
        let result = sqlx::query(
            "INSERT INTO webhook_inbox (channel, external_event_id, raw_payload) VALUES ($1, $2, $3)",
        )
        .bind(channel.as_sql())
        .bind(external_event_id)
        .bind(raw_payload)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                Err(InboxError::Duplicate { channel: channel.as_sql().to_string(), external_event_id: external_event_id.to_string() })
            }
            Err(e) => Err(InboxError::Database(e)),
        }
    }

    /// Fetch a batch of unprocessed rows for the background worker to drain.
    pub async fn fetch_unprocessed(&self, limit: i64) -> Result<Vec<InboxRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, channel, external_event_id, raw_payload FROM webhook_inbox \
             WHERE processed_at IS NULL ORDER BY received_at ASC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn mark_processed(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE webhook_inbox SET processed_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
