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
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO order_current_state (id, branch_id, customer_id, description, state)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO UPDATE SET state = EXCLUDED.state, updated_at = NOW()
            "#,
        )
        .bind(id.into_inner())
        .bind(branch_id)
        .bind(customer_id)
        .bind(description)
        .bind(state.to_string())
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
            INSERT INTO supply_request_current_state (id, branch_id, description, state)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (id) DO UPDATE SET state = EXCLUDED.state, updated_at = NOW()
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
        sqlx::query_as("SELECT id, branch_id, customer_id, description, state FROM order_current_state WHERE branch_id = $1 ORDER BY updated_at DESC")
            .bind(branch_id)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn supply_requests_by_branch(&self, branch_id: Uuid) -> Result<Vec<SupplyRequestCurrentState>, sqlx::Error> {
        sqlx::query_as("SELECT id, branch_id, description, state FROM supply_request_current_state WHERE branch_id = $1 ORDER BY updated_at DESC")
            .bind(branch_id)
            .fetch_all(&self.pool)
            .await
    }
}
