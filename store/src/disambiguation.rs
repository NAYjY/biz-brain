//! Harness disambiguation state — replaces the thin yes/no loop.
//!
//! One row per sender currently in a declaration flow.
//! The Gemini harness reads and writes this across multiple turns until:
//!   (a) Worker declares → row deleted, event emitted, notes flushed to owner alert
//!   (b) turns_elapsed reaches 3 → escalated_at set, owner gets ⚠️ on dashboard
//!   (c) Row expires (>24h old) → background cleanup, fresh start on next message

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// Full row as read from DB — handed to the harness as context on each turn.
#[derive(Debug, Clone, FromRow)]
pub struct DisambiguationRow {
    pub id: Uuid,
    pub sender_key: String,
    pub original_text: String,
    pub candidate_ids: serde_json::Value,
    pub aggregate_type: String,
    pub original_intent: Option<String>,
    pub turns_elapsed: i32,
    pub extracted_notes: serde_json::Value,
    pub last_question: Option<String>,
    pub narrowed_to: Option<Uuid>,
    pub escalated_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl DisambiguationRow {
    /// All candidate UUIDs in insertion order.
    pub fn candidates(&self) -> Vec<Uuid> {
        self.candidate_ids
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str()?.parse().ok())
            .collect()
    }

    /// Notes accumulated so far as strings.
    pub fn notes(&self) -> Vec<String> {
        self.extracted_notes
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    }

    pub fn is_escalated(&self) -> bool {
        self.escalated_at.is_some()
    }

    pub fn needs_escalation(&self) -> bool {
        self.turns_elapsed >= 3 && self.escalated_at.is_none()
    }
}

/// Everything the harness writes back after processing one turn.
#[derive(Debug, Clone)]
pub struct DisambiguationUpdate {
    /// New notes extracted from this turn's message (appended to existing).
    pub new_notes: Vec<String>,
    /// Updated narrowed_to — Some(uuid) if harness narrowed to one candidate.
    pub narrowed_to: Option<Uuid>,
    /// The question asked this turn — stored so next turn rephrases.
    pub last_question: Option<String>,
    /// Whether to mark this row as escalated.
    pub escalate: bool,
}

pub struct DisambiguationStore {
    pool: PgPool,
}

impl DisambiguationStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Find active disambiguation for this sender.
    /// Returns None if no pending flow or if the row is stale (>24h).
    pub async fn find(&self, sender_key: &str) -> Result<Option<DisambiguationRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, sender_key, original_text, candidate_ids, aggregate_type,
                    original_intent, turns_elapsed, extracted_notes, last_question,
                    narrowed_to, escalated_at, created_at
             FROM disambiguation_pending
             WHERE sender_key = $1
               AND created_at > NOW() - INTERVAL '24 hours'",
        )
        .bind(sender_key)
        .fetch_optional(&self.pool)
        .await
    }

    /// Create a new disambiguation flow on turn 1.
    /// ON CONFLICT: replace existing row for this sender (fresh start).
    pub async fn create(
        &self,
        sender_key: &str,
        original_text: &str,
        candidate_ids: &[Uuid],
        aggregate_type: &str,
        original_intent: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let ids_json = serde_json::to_value(
            candidate_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();

        sqlx::query(
            "INSERT INTO disambiguation_pending
                (sender_key, original_text, candidate_ids, aggregate_type,
                 original_intent, turns_elapsed, extracted_notes)
             VALUES ($1, $2, $3, $4, $5, 0, '[]')
             ON CONFLICT (sender_key) DO UPDATE
                SET original_text   = EXCLUDED.original_text,
                    candidate_ids   = EXCLUDED.candidate_ids,
                    aggregate_type  = EXCLUDED.aggregate_type,
                    original_intent = EXCLUDED.original_intent,
                    turns_elapsed   = 0,
                    extracted_notes = '[]',
                    last_question   = NULL,
                    narrowed_to     = NULL,
                    escalated_at    = NULL,
                    created_at      = NOW()",
        )
        .bind(sender_key)
        .bind(original_text)
        .bind(ids_json)
        .bind(aggregate_type)
        .bind(original_intent)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Apply harness output after one turn — increment turns, append notes,
    /// update narrowed_to and last_question, optionally escalate.
    pub async fn update(
        &self,
        sender_key: &str,
        update: DisambiguationUpdate,
    ) -> Result<(), sqlx::Error> {
        // Append new notes to existing JSON array
        let new_notes_json = serde_json::to_value(&update.new_notes).unwrap_or_default();

        let escalated_at_expr = if update.escalate {
            "COALESCE(escalated_at, NOW())"
        } else {
            "escalated_at"
        };

        // Build query dynamically to handle the escalation expression cleanly
        let query = format!(
            "UPDATE disambiguation_pending
             SET turns_elapsed   = turns_elapsed + 1,
                 extracted_notes = extracted_notes || $2::jsonb,
                 narrowed_to     = $3,
                 last_question   = $4,
                 escalated_at    = {escalated_at_expr}
             WHERE sender_key = $1"
        );

        sqlx::query(&query)
            .bind(sender_key)
            .bind(new_notes_json)
            .bind(update.narrowed_to)
            .bind(update.last_question)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete on successful declaration — event emitted, flow complete.
    pub async fn delete(&self, sender_key: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM disambiguation_pending WHERE sender_key = $1")
            .bind(sender_key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Purge stale rows older than 24h — called by a background task.
    pub async fn purge_stale(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM disambiguation_pending
             WHERE created_at < NOW() - INTERVAL '24 hours'",
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}