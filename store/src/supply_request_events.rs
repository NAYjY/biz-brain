//! T02: read (replay) side of the supply_request_events stream.

use domain::{BranchId, DomainEvent, InvoiceId, SupplierId, SupplyRequestId};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::order_events::ReadError;

#[derive(Debug, FromRow)]
struct SupplyRequestEventRow {
    aggregate_id: Uuid,
    branch_id: Uuid,
    event_type: String,
    supplier_id: Option<Uuid>,
    invoice_id: Option<Uuid>,
}

pub struct SupplyRequestEventRepository {
    pool: PgPool,
}

impl SupplyRequestEventRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn current_sequence(&self, aggregate_id: SupplyRequestId) -> Result<i64, ReadError> {
        let row: (Option<i64>,) =
            sqlx::query_as("SELECT MAX(sequence) FROM supply_request_events WHERE aggregate_id = $1")
                .bind(aggregate_id.into_inner())
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0.unwrap_or(0))
    }

    pub async fn load_stream(&self, aggregate_id: SupplyRequestId) -> Result<Vec<DomainEvent>, ReadError> {
        let rows: Vec<SupplyRequestEventRow> = sqlx::query_as(
            "SELECT aggregate_id, branch_id, event_type, supplier_id, invoice_id \
             FROM supply_request_events WHERE aggregate_id = $1 ORDER BY sequence ASC",
        )
        .bind(aggregate_id.into_inner())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Self::row_to_event).collect()
    }

    fn row_to_event(row: SupplyRequestEventRow) -> Result<DomainEvent, ReadError> {
        let supply_request_id = SupplyRequestId::new(row.aggregate_id);
        let branch_id = BranchId::new(row.branch_id);
        let missing = |col: &'static str| ReadError::MissingColumn { event_type: row.event_type.clone(), column: col };

        Ok(match row.event_type.as_str() {
            "supply_request_sent" => DomainEvent::SupplyRequestSent { supply_request_id, branch_id },
            "invoice_received" => DomainEvent::InvoiceReceived {
                supplier_id: SupplierId::new(row.supplier_id.ok_or_else(|| missing("supplier_id"))?),
                supply_request_id,
                invoice_id: InvoiceId::new(row.invoice_id.ok_or_else(|| missing("invoice_id"))?),
            },
            "invoice_approved" => DomainEvent::InvoiceApproved {
                invoice_id: InvoiceId::new(row.invoice_id.ok_or_else(|| missing("invoice_id"))?),
                branch_id,
            },
            "supplier_confirmed" => DomainEvent::SupplierConfirmed {
                supplier_id: SupplierId::new(row.supplier_id.ok_or_else(|| missing("supplier_id"))?),
                supply_request_id,
                invoice_id: InvoiceId::new(row.invoice_id.ok_or_else(|| missing("invoice_id"))?),
            },
            other => return Err(ReadError::UnknownEventType(other.to_string())),
        })
    }
}
