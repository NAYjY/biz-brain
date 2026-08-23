//! F05: Follow-up alert persistence.
//! Background worker polls `follow_up_alerts` every minute and fires
//! any rows where `next_fire_at <= NOW()` and `deleted_at IS NULL`.
//! After firing a 'once' alert the row is soft-deleted.
//! Recurring alerts update `next_fire_at` to the next interval.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertMode {
    Once,
    Hourly,
    Daily,
    Every3d,
}

impl AlertMode {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Once    => "once",
            Self::Hourly  => "hourly",
            Self::Daily   => "daily",
            Self::Every3d => "every3d",
        }
    }

    pub fn from_sql(s: &str) -> Option<Self> {
        match s {
            "once"    => Some(Self::Once),
            "hourly"  => Some(Self::Hourly),
            "daily"   => Some(Self::Daily),
            "every3d" => Some(Self::Every3d),
            _         => None,
        }
    }

    /// Duration to add for the next fire. None for 'once'.
    pub fn interval(&self) -> Option<Duration> {
        match self {
            Self::Once    => None,
            Self::Hourly  => Some(Duration::hours(1)),
            Self::Daily   => Some(Duration::hours(24)),
            Self::Every3d => Some(Duration::hours(72)),
        }
    }
}

/// Row returned from DB for pending alert processing.
#[derive(Debug, Clone, FromRow)]
pub struct AlertRow {
    pub id: Uuid,
    pub order_id: Uuid,
    pub branch_id: Uuid,
    pub alert_mode: String,
    pub message: Option<String>,
    pub fired_count: i32,
}

/// Row returned for display (Owner-facing API).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AlertView {
    pub id: Uuid,
    pub alert_mode: String,
    pub alert_at: DateTime<Utc>,
    pub next_fire_at: DateTime<Utc>,
    pub message: Option<String>,
    pub fired_count: i32,
}

pub struct AlertRepository {
    pool: PgPool,
}

impl AlertRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create a new alert. `alert_at` is both the first fire time and the
    /// reference point for recurring intervals.
    pub async fn create(
        &self,
        order_id: Uuid,
        branch_id: Uuid,
        mode: &AlertMode,
        alert_at: DateTime<Utc>,
        message: Option<&str>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO follow_up_alerts \
                (id, order_id, branch_id, alert_mode, alert_at, next_fire_at, message) \
             VALUES ($1, $2, $3, $4, $5, $5, $6)",
        )
        .bind(id)
        .bind(order_id)
        .bind(branch_id)
        .bind(mode.as_sql())
        .bind(alert_at)
        .bind(message)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// List active (non-deleted) alerts for an order.
    pub async fn list_for_order(&self, order_id: Uuid) -> Result<Vec<AlertView>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, alert_mode, alert_at, next_fire_at, message, fired_count \
             FROM follow_up_alerts \
             WHERE order_id = $1 AND deleted_at IS NULL \
             ORDER BY next_fire_at ASC",
        )
        .bind(order_id)
        .fetch_all(&self.pool)
        .await
    }

    /// Fetch alerts due to fire right now.
    pub async fn fetch_due(&self) -> Result<Vec<AlertRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, order_id, branch_id, alert_mode, message, fired_count \
             FROM follow_up_alerts \
             WHERE next_fire_at <= NOW() AND deleted_at IS NULL \
             ORDER BY next_fire_at ASC \
             LIMIT 100",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// After firing: advance next_fire_at for recurring, or soft-delete for 'once'.
    pub async fn mark_fired(&self, id: Uuid, mode: &AlertMode) -> Result<(), sqlx::Error> {
        match mode.interval() {
            None => {
                // once — soft-delete
                sqlx::query(
                    "UPDATE follow_up_alerts \
                     SET deleted_at = NOW(), fired_count = fired_count + 1 \
                     WHERE id = $1",
                )
                .bind(id)
                .execute(&self.pool)
                .await?;
            }
            Some(interval) => {
                // recurring — advance
                sqlx::query(
                    "UPDATE follow_up_alerts \
                     SET next_fire_at = next_fire_at + $2, \
                         fired_count  = fired_count + 1 \
                     WHERE id = $1",
                )
                .bind(id)
                .bind(interval)
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(())
    }

    /// Owner clears (soft-deletes) an alert.
    pub async fn delete_for_order(
        &self,
        alert_id: Uuid,
        order_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let r = sqlx::query(
            "UPDATE follow_up_alerts \
             SET deleted_at = NOW() \
             WHERE id = $1 AND order_id = $2 AND deleted_at IS NULL",
        )
        .bind(alert_id)
        .bind(order_id)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Clear all active alerts for an order (e.g. on close/delete).
    pub async fn clear_all_for_order(&self, order_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE follow_up_alerts SET deleted_at = NOW() \
             WHERE order_id = $1 AND deleted_at IS NULL",
        )
        .bind(order_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}