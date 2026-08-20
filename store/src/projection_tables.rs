//! T02: materialized current-state projections. Dashboard (api crate) reads
//! these directly; never the raw event stream.

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
        sqlx::query(
            r#"
            INSERT INTO order_current_state
                (order_id, branch_id, customer_id, description, state, worker_id)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (order_id) DO UPDATE
                SET state      = EXCLUDED.state,
                    description = EXCLUDED.description,
                    worker_id  = COALESCE(EXCLUDED.worker_id, order_current_state.worker_id),
                    updated_at = NOW()
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
        // Pulls latest edited description, joins worker name, excludes soft-deleted.
        sqlx::query_as(
            "SELECT
                ocs.order_id        AS id,
                ocs.branch_id,
                ocs.customer_id,
                -- latest description edit wins, falls back to original
                COALESCE(
                    (SELECT new_description FROM order_description_edits
                     WHERE order_id = ocs.order_id ORDER BY id DESC LIMIT 1),
                    o.description
                )                   AS description,
                ocs.state,
                ocs.worker_id,
                w.name              AS worker_name,
                ocs.last_worker_message,
                ocs.last_worker_message_at
             FROM order_current_state ocs
             JOIN orders o ON o.id = ocs.order_id
             LEFT JOIN workers w ON w.id = ocs.worker_id
             WHERE ocs.branch_id = $1
               AND o.deleted_at IS NULL
             ORDER BY ocs.updated_at DESC",
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