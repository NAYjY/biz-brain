//! D05: invoice_current_state projection reads + upsert.
//! Projection worker calls upsert; api reads list filtered to 'Sent'.

use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct InvoiceCurrentStateRow {
    pub invoice_id: Uuid,
    pub branch_id: Uuid,
    pub supply_request_id: Uuid,
    pub supplier_id: Uuid,
    pub state: String,
    pub notes: Option<String>,
}

pub struct InvoiceProjection {
    pool: PgPool,
}

impl InvoiceProjection {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// D05: Approve-Invoice form only shows Sent invoices.
    pub async fn list_sent_by_branch(
        &self,
        branch_id: Uuid,
    ) -> Result<Vec<InvoiceCurrentStateRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT invoice_id, branch_id, supply_request_id, supplier_id, state, notes \
             FROM invoice_current_state \
             WHERE branch_id = $1 AND state = 'Sent' \
             ORDER BY updated_at DESC",
        )
        .bind(branch_id)
        .fetch_all(&self.pool)
        .await
    }

    /// Called by projection worker after InvoiceReceived / InvoiceApproved / SupplierConfirmed.
    pub async fn upsert(
        &self,
        invoice_id: Uuid,
        branch_id: Uuid,
        supply_request_id: Uuid,
        supplier_id: Uuid,
        state: &str,
        notes: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO invoice_current_state
                (invoice_id, branch_id, supply_request_id, supplier_id, state, notes)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (invoice_id) DO UPDATE
                SET state = EXCLUDED.state,
                    notes = EXCLUDED.notes,
                    updated_at = NOW()
            "#,
        )
        .bind(invoice_id)
        .bind(branch_id)
        .bind(supply_request_id)
        .bind(supplier_id)
        .bind(state)
        .bind(notes)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}