//! T02: materialized current-state projections. Dashboard (api crate) reads
//! these directly; never the raw event stream.
//!
//! F04: unread_message_count, ai_routed_low_confidence, last_event_at added.
//! orders_by_branch now sorts by last_event_at DESC (most recent activity first).
//! F01: short_name added to OrderCurrentState, upsert, and query.

use domain::{OrderId, OrderState, SupplyRequestId, SupplyRequestState};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, FromRow)]
pub struct OrderCurrentState {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub customer_id: Uuid,
    pub description: String,
    pub state: String,
    pub worker_id: Option<Uuid>,
    pub worker_name: Option<String>,
    pub last_worker_message: Option<String>,
    pub last_worker_message_at: Option<chrono::DateTime<chrono::Utc>>,
    /// F04: unread worker message count. Reset when Owner opens thread / replies.
    pub unread_message_count: i32,
    /// F04: true when the AI classifier was below confidence threshold (F02 hook).
    pub ai_routed_low_confidence: bool,
    /// F04: timestamp of most-recent event — used for row sort order.
    pub last_event_at: Option<chrono::DateTime<chrono::Utc>>,
    /// F01: optional short handle (≤20 chars) the bot uses instead of description.
    pub short_name: Option<String>,
}

#[derive(Debug, FromRow)]
pub struct SupplyRequestCurrentState {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub description: String,
    pub state: String,
}

pub struct ProjectionTables {
    pool: PgPool,
}

impl ProjectionTables {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert_order_state(
        &self,
        id: OrderId,
        branch_id: Uuid,
        customer_id: Uuid,
        description: &str,
        state: OrderState,
        worker_id: Option<uuid::Uuid>,
    ) -> Result<(), sqlx::Error> {
        // short_name is not supplied here — it is written separately by
        // set_short_name and never cleared by normal event projection.
        sqlx::query(
            r#"
            INSERT INTO order_current_state
                (order_id, branch_id, customer_id, description, state, worker_id, last_event_at)
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            ON CONFLICT (order_id) DO UPDATE
                SET state        = EXCLUDED.state,
                    description  = EXCLUDED.description,
                    worker_id    = COALESCE(EXCLUDED.worker_id, order_current_state.worker_id),
                    last_event_at = NOW(),
                    updated_at   = NOW()
            "#,
        )
        .bind(id.into_inner())
        .bind(branch_id)
        .bind(customer_id)
        .bind(description)
        .bind(state.to_string())
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// F01: write short_name to both tables in one call.
    /// Called by the set_short_name command endpoint and create_order.
    pub async fn set_short_name(
        &self,
        order_id: OrderId,
        short_name: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE order_current_state \
             SET short_name = $2, updated_at = NOW() \
             WHERE order_id = $1",
        )
        .bind(order_id.into_inner())
        .bind(short_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Save the raw text of the last Worker message on a specific order.
    pub async fn update_worker_message(
        &self,
        order_id: OrderId,
        message: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE order_current_state \
             SET last_worker_message = $2, \
                 last_worker_message_at = NOW() \
             WHERE order_id = $1",
        )
        .bind(order_id.into_inner())
        .bind(message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// F04: increment unread count and bump last_event_at.
    /// Called by inbox_worker after each inbound worker message is persisted.
    pub async fn increment_unread(&self, order_id: OrderId) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE order_current_state \
             SET unread_message_count = unread_message_count + 1, \
                 last_event_at = NOW() \
             WHERE order_id = $1",
        )
        .bind(order_id.into_inner())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// F04: reset unread count and low-confidence flag.
    /// Called when Owner opens the thread modal or sends a reply.
    pub async fn clear_unread(&self, order_id: OrderId) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE order_current_state \
             SET unread_message_count = 0, \
                 ai_routed_low_confidence = FALSE \
             WHERE order_id = $1",
        )
        .bind(order_id.into_inner())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// F04 / F02 hook: mark this order row as AI-routed with low confidence.
    /// Cleared by clear_unread().
    pub async fn flag_low_confidence(&self, order_id: OrderId) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE order_current_state \
             SET ai_routed_low_confidence = TRUE \
             WHERE order_id = $1",
        )
        .bind(order_id.into_inner())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_supply_request_state(
        &self,
        id: SupplyRequestId,
        branch_id: Uuid,
        description: &str,
        state: SupplyRequestState,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO supply_request_current_state (supply_request_id, branch_id, description, state)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (supply_request_id) DO UPDATE
                SET state = EXCLUDED.state,
                    updated_at = NOW()
            "#,
        )
        .bind(id.into_inner())
        .bind(branch_id)
        .bind(description)
        .bind(state.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn orders_by_branch(&self, branch_id: Uuid) -> Result<Vec<OrderCurrentState>, sqlx::Error> {
        // F04: sorted by last_event_at DESC (most recent activity first).
        // F01: short_name included so API and SSR never need a separate join.
        sqlx::query_as(
            "SELECT
                ocs.order_id        AS id,
                ocs.branch_id,
                ocs.customer_id,
                COALESCE(
                    (SELECT new_description FROM order_description_edits
                     WHERE order_id = ocs.order_id ORDER BY id DESC LIMIT 1),
                    o.description
                )                   AS description,
                ocs.state,
                ocs.worker_id,
                w.name              AS worker_name,
                ocs.last_worker_message,
                ocs.last_worker_message_at,
                ocs.unread_message_count,
                ocs.ai_routed_low_confidence,
                ocs.last_event_at,
                COALESCE(ocs.short_name, o.short_name) AS short_name
             FROM order_current_state ocs
             JOIN orders o ON o.id = ocs.order_id
             LEFT JOIN workers w ON w.id = ocs.worker_id
             WHERE ocs.branch_id = $1
               AND o.deleted_at IS NULL
             ORDER BY
                CASE WHEN ocs.state IN ('DONE','CANCELLED') THEN 1 ELSE 0 END ASC,
                ocs.last_event_at DESC NULLS LAST,
                ocs.updated_at DESC",
        )
        .bind(branch_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn supply_requests_by_branch(&self, branch_id: Uuid) -> Result<Vec<SupplyRequestCurrentState>, sqlx::Error> {
        sqlx::query_as(
            "SELECT supply_request_id AS id, branch_id, description, state \
             FROM supply_request_current_state \
             WHERE branch_id = $1 \
             ORDER BY updated_at DESC",
        )
        .bind(branch_id)
        .fetch_all(&self.pool)
        .await
    }
}