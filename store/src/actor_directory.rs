//! S06: ChannelIdentity binding store. Root-of-trust mapping from inbound
//! (channel, external_id) -> WorkerId/SupplierId.

use domain::{BranchId, Channel, ChannelIdentity, SupplierId, WorkerId};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorType {
    Worker,
    Supplier,
}

impl ActorType {
    fn as_sql(&self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Supplier => "supplier",
        }
    }
}

#[derive(Debug, FromRow, Serialize)]
pub struct PendingBinding {
    pub id: Uuid,
    pub channel: String,
    pub external_id: String,
    pub actor_type: String,
    pub actor_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub struct ActorDirectory {
    pool: PgPool,
}

impl ActorDirectory {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn resolve_worker(&self, identity: &ChannelIdentity) -> Result<Option<WorkerId>, sqlx::Error> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT actor_id FROM actor_directory \
             WHERE channel = $1 AND external_id = $2 \
               AND actor_type = 'worker' \
               AND owner_confirmed = TRUE",
        )
        .bind(identity.channel.as_sql())
        .bind(&identity.external_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| WorkerId::new(id)))
    }

    pub async fn resolve_supplier(&self, identity: &ChannelIdentity) -> Result<Option<SupplierId>, sqlx::Error> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT actor_id FROM actor_directory \
             WHERE channel = $1 AND external_id = $2 \
               AND actor_type = 'supplier' \
               AND owner_confirmed = TRUE",
        )
        .bind(identity.channel.as_sql())
        .bind(&identity.external_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| SupplierId::new(id)))
    }

    pub async fn register_pending(
        &self,
        branch_id: BranchId,
        identity: &ChannelIdentity,
        actor_type: ActorType,
        actor_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO actor_directory \
                (channel, external_id, actor_type, actor_id, branch_id, owner_confirmed) \
             VALUES ($1, $2, $3, $4, $5, FALSE) \
             ON CONFLICT (channel, external_id) DO NOTHING",
        )
        .bind(identity.channel.as_sql())
        .bind(&identity.external_id)
        .bind(actor_type.as_sql())
        .bind(actor_id)
        .bind(branch_id.into_inner())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn confirm_binding(&self, binding_id: Uuid, branch_id: BranchId) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE actor_directory \
             SET owner_confirmed = TRUE, confirmed_at = NOW() \
             WHERE id = $1 AND branch_id = $2 AND owner_confirmed = FALSE",
        )
        .bind(binding_id)
        .bind(branch_id.into_inner())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn reject_binding(&self, binding_id: Uuid, branch_id: BranchId) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM actor_directory WHERE id = $1 AND branch_id = $2",
        )
        .bind(binding_id)
        .bind(branch_id.into_inner())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn list_pending(&self, branch_id: BranchId) -> Result<Vec<PendingBinding>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, channel, external_id, actor_type, actor_id, created_at \
             FROM actor_directory \
             WHERE branch_id = $1 AND owner_confirmed = FALSE \
             ORDER BY created_at ASC",
        )
        .bind(branch_id.into_inner())
        .fetch_all(&self.pool)
        .await
    }

    /// T03: set-valued active Orders for a Worker — used to seed ThreadContextStore.
    ///
    /// Fix over the old approximation: join against `order_current_state` projection
    /// and filter by *current* state, not just event presence. This correctly excludes
    /// orders that had the worker assigned but are now Done/Cancelled.
    pub async fn active_orders_for_worker(
        &self,
        worker_id: WorkerId,
    ) -> Result<Vec<domain::OrderId>, sqlx::Error> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT ocs.order_id \
             FROM order_current_state ocs \
             WHERE ocs.worker_id = $1 \
               AND ocs.state NOT IN ('done', 'cancelled', 'unavailable')",
        )
        .bind(worker_id.into_inner())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| domain::OrderId::new(id)).collect())
    }
}