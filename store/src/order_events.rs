//! T02: read (replay) side of the order_events stream.
//! P04/P15: extended with owner_cancelled, order_reset, clarification_resolved.

use domain::{DomainEvent, OrderId, WorkerId};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("unknown event_type '{0}' in order_events (impossible under CHECK constraint)")]
    UnknownEventType(String),
    #[error("row for event_type '{event_type}' missing required column '{column}'")]
    MissingColumn { event_type: String, column: &'static str },
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, FromRow)]
struct OrderEventRow {
    aggregate_id: Uuid,
    #[allow(dead_code)]
    sequence: i64,
    event_type: String,
    worker_id: Option<Uuid>,
}

pub struct OrderEventRepository {
    pool: PgPool,
}

impl OrderEventRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn current_sequence(&self, aggregate_id: OrderId) -> Result<i64, ReadError> {
        let row: (Option<i64>,) =
            sqlx::query_as("SELECT MAX(sequence) FROM order_events WHERE aggregate_id = $1")
                .bind(aggregate_id.into_inner())
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0.unwrap_or(0))
    }

    /// Full replay ordered by sequence (T02: no snapshotting).
    pub async fn load_stream(&self, aggregate_id: OrderId) -> Result<Vec<DomainEvent>, ReadError> {
        let rows: Vec<OrderEventRow> = sqlx::query_as(
            "SELECT aggregate_id, sequence, event_type, worker_id \
             FROM order_events WHERE aggregate_id = $1 ORDER BY sequence ASC",
        )
        .bind(aggregate_id.into_inner())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Self::row_to_event).collect()
    }

    fn row_to_event(row: OrderEventRow) -> Result<DomainEvent, ReadError> {
        let order_id = OrderId::new(row.aggregate_id);

        // Helper: require worker_id or return MissingColumn
        let require_worker = || {
            row.worker_id
                .map(WorkerId::new)
                .ok_or_else(|| ReadError::MissingColumn {
                    event_type: row.event_type.clone(),
                    column: "worker_id",
                })
        };

        Ok(match row.event_type.as_str() {
            "worker_assigned"         => DomainEvent::WorkerAssigned { worker_id: require_worker()?, order_id },
            "worker_accepted"         => DomainEvent::WorkerAccepted { worker_id: require_worker()?, order_id },
            "worker_unavailable"      => DomainEvent::WorkerUnavailable { worker_id: require_worker()?, order_id },
            "worker_cancelled"        => DomainEvent::WorkerCancelled { worker_id: require_worker()?, order_id },
            "clarification_requested" => DomainEvent::ClarificationRequested { worker_id: require_worker()?, order_id },
            "worker_ready_for_pickup" => DomainEvent::WorkerReadyForPickup { worker_id: require_worker()?, order_id },
            "order_done"              => DomainEvent::OrderDone { order_id },
            // P04
            "owner_cancelled"         => DomainEvent::OwnerCancelled { order_id },
            "order_reset"             => DomainEvent::OrderReset { order_id },
            "clarification_resolved"  => DomainEvent::ClarificationResolved { worker_id: require_worker()?, order_id },
            other => return Err(ReadError::UnknownEventType(other.to_string())),
        })
    }
}
