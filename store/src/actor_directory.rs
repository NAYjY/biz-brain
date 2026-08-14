//! S06: ChannelIdentity binding store. Root-of-trust mapping from inbound
//! (channel, external_id) -> WorkerId/SupplierId.
//!
//! Pending rows have no branch_id or actor_id — those are supplied by the
//! Owner at confirm time via the dashboard (migration 021).

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
    pub actor_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub struct ActorDirectory {
    pool: PgPool,
}

impl ActorDirectory {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Trusted lookup — only confirmed rows, fail closed.
    pub async fn resolve_worker(&self, identity: &ChannelIdentity) -> Result<Option<WorkerId>, sqlx::Error> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT actor_id FROM actor_directory \
             WHERE channel = $1 AND external_id = $2 \
               AND actor_type = 'worker' \
               AND owner_confirmed = TRUE \
               AND actor_id IS NOT NULL",
        )
        .bind(identity.channel.as_sql())
        .bind(&identity.external_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| WorkerId::new(id)))
    }

    /// Trusted lookup — only confirmed rows, fail closed.
    pub async fn resolve_supplier(&self, identity: &ChannelIdentity) -> Result<Option<SupplierId>, sqlx::Error> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT actor_id FROM actor_directory \
             WHERE channel = $1 AND external_id = $2 \
               AND actor_type = 'supplier' \
               AND owner_confirmed = TRUE \
               AND actor_id IS NOT NULL",
        )
        .bind(identity.channel.as_sql())
        .bind(&identity.external_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| SupplierId::new(id)))
    }

    /// Register an unknown sender as a pending binding.
    /// No branch_id or actor_id needed — Owner supplies those at confirm time.
    /// ON CONFLICT DO NOTHING: repeated messages from same sender don't create duplicates.
    pub async fn register_pending(
        &self,
        identity: &ChannelIdentity,
        actor_type: ActorType,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO actor_directory \
                (channel, external_id, actor_type, owner_confirmed) \
             VALUES ($1, $2, $3, FALSE) \
             ON CONFLICT (channel, external_id) DO NOTHING",
        )
        .bind(identity.channel.as_sql())
        .bind(&identity.external_id)
        .bind(actor_type.as_sql())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Owner confirms a binding, supplying branch_id and actor_id (WorkerId or SupplierId).
    pub async fn confirm_binding(
        &self,
        binding_id: Uuid,
        branch_id: BranchId,
        actor_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE actor_directory \
             SET owner_confirmed = TRUE, \
                 confirmed_at = NOW(), \
                 branch_id = $2, \
                 actor_id = $3 \
             WHERE id = $1 \
               AND owner_confirmed = FALSE",
        )
        .bind(binding_id)
        .bind(branch_id.into_inner())
        .bind(actor_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Owner rejects/removes a binding.
    /// Rebinding: reject old entry, next message from that sender creates a new pending row.
    pub async fn reject_binding(
        &self,
        binding_id: Uuid,
        branch_id: BranchId,
    ) -> Result<bool, sqlx::Error> {
        // For pending rows branch_id may be NULL, so match on id only
        // but verify the row belongs to a branch this owner owns (or is pending).
        let result = sqlx::query(
            "DELETE FROM actor_directory \
             WHERE id = $1 \
               AND (branch_id = $2 OR branch_id IS NULL)",
        )
        .bind(binding_id)
        .bind(branch_id.into_inner())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// List all unconfirmed bindings — not branch-scoped since branch is
    /// unknown until Owner confirms. Owner sees all pending across all channels.
    pub async fn list_pending(
        &self,
        _branch_id: BranchId,
    ) -> Result<Vec<PendingBinding>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, channel, external_id, actor_type, actor_id, created_at \
             FROM actor_directory \
             WHERE owner_confirmed = FALSE \
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// T03: set-valued active Orders for a Worker — seeds ThreadContextStore.
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
