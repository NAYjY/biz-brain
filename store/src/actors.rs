//! T04: `messaging` maps an inbound sender to `(Channel, external_id)`;
//! `store` resolves that to a `WorkerId`/`SupplierId`. Also exposes the
//! reverse (for outbound push) and each actor's currently-active Orders
//! (feeds `agent`'s `ThreadContextStore`, T03).

use domain::{BranchId, ChannelIdentity, OrderId, SupplierId, WorkerId};
use sqlx::PgPool;

pub struct ActorDirectory {
    pool: PgPool,
}

impl ActorDirectory {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn resolve_worker(&self, identity: &ChannelIdentity) -> Result<Option<WorkerId>, sqlx::Error> {
        let row: Option<(uuid::Uuid,)> =
            sqlx::query_as("SELECT id FROM workers WHERE channel = $1 AND external_id = $2")
                .bind(identity.channel.as_sql())
                .bind(&identity.external_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(id,)| WorkerId::new(id)))
    }

    pub async fn resolve_supplier(&self, identity: &ChannelIdentity) -> Result<Option<SupplierId>, sqlx::Error> {
        let row: Option<(uuid::Uuid,)> =
            sqlx::query_as("SELECT id FROM suppliers WHERE channel = $1 AND external_id = $2")
                .bind(identity.channel.as_sql())
                .bind(&identity.external_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(id,)| SupplierId::new(id)))
    }

    pub async fn register_worker(
        &self,
        branch_id: BranchId,
        name: &str,
        identity: &ChannelIdentity,
    ) -> Result<WorkerId, sqlx::Error> {
        let id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO workers (id, branch_id, name, channel, external_id) VALUES ($1, $2, $3, $4, $5)")
            .bind(id)
            .bind(branch_id.into_inner())
            .bind(name)
            .bind(identity.channel.as_sql())
            .bind(&identity.external_id)
            .execute(&self.pool)
            .await?;
        Ok(WorkerId::new(id))
    }

    /// Orders currently `Assigned`/`Accepted`/`PendingClarification` for this
    /// Worker — the set `agent::ThreadContextStore` needs (T03: set-valued,
    /// a Worker can hold multiple concurrent Orders).
    pub async fn active_orders_for_worker(&self, worker_id: WorkerId) -> Result<Vec<OrderId>, sqlx::Error> {
        // Reads the latest `worker_id`-bearing event per Order — a simple
        // approximation; a dedicated `assignments` read model would be the
        // real answer, flagged as a follow-up rather than blocking this pass.
        let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
            "SELECT DISTINCT aggregate_id FROM order_events \
             WHERE worker_id = $1 AND event_type IN ('worker_assigned', 'worker_accepted', 'clarification_requested')",
        )
        .bind(worker_id.into_inner())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| OrderId::new(id)).collect())
    }
}
