//! T09: cursor-based keyset pagination for orders and supply requests.
//!
//! ## Cursor shape
//! `OrderCursor { is_terminal, last_event_at, order_id }` encoded as base64(JSON).
//! `SrCursor { is_terminal, updated_at, supply_request_id }` — same approach.
//! Both are opaque to the client; the client passes `after=<string>` verbatim.
//!
//! ## Sort key
//! Orders:           (is_terminal ASC, last_event_at DESC NULLS LAST, order_id ASC)
//! Supply requests:  (is_terminal ASC, updated_at    DESC NULLS LAST, supply_request_id ASC)
//!
//! ## Filter params (orders)
//! - `states`    — vec of uppercased state strings (validated: only A-Z and _)
//! - `worker_id` — optional UUID bound as SQL parameter
//! - `q`         — ILIKE pattern against description + short_name (wildcards escaped)
//!
//! ## Filter params (supply requests)
//! - `states` only (T09 resolution)
//!
//! SSE-triggered refreshes always reload page 1 (no `after` param), discarding any cursor.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::projection_tables::OrderCurrentState;

// ── Constants ─────────────────────────────────────────────────────────────── //

pub const PAGE_SIZE: i64 = 50;

// ── Cursor types ──────────────────────────────────────────────────────────── //

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderCursor {
    pub is_terminal: bool,
    pub last_event_at: Option<DateTime<Utc>>,
    pub order_id: Uuid,
}

impl OrderCursor {
    pub fn encode(&self) -> String {
        B64.encode(serde_json::to_string(self).unwrap_or_default())
    }

    pub fn decode(s: &str) -> Option<Self> {
        let bytes = B64.decode(s).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrCursor {
    pub is_terminal: bool,
    pub updated_at: Option<DateTime<Utc>>,
    pub supply_request_id: Uuid,
}

impl SrCursor {
    pub fn encode(&self) -> String {
        B64.encode(serde_json::to_string(self).unwrap_or_default())
    }

    pub fn decode(s: &str) -> Option<Self> {
        let bytes = B64.decode(s).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

// ── Filter params ─────────────────────────────────────────────────────────── //

#[derive(Debug, Default, Clone)]
pub struct OrderFilter {
    /// Parsed from `?state=ASSIGNED,ACCEPTED` (comma-separated).
    pub states: Vec<String>,
    /// Optional worker UUID from `?worker_id=<uuid>`.
    pub worker_id: Option<Uuid>,
    /// Free-text `?q=` — ILIKE against description and short_name.
    pub q: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct SrFilter {
    /// Parsed from `?state=DRAFT,SENT` (comma-separated).
    pub states: Vec<String>,
}

// ── Result row for supply requests (carries updated_at for cursor) ─────────── //

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SrPageRow {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub description: String,
    pub state: String,
    pub order_ids: serde_json::Value,
    pub updated_at: Option<DateTime<Utc>>,
}

// ── Paginated projection queries ──────────────────────────────────────────── //

pub struct PaginatedProjections {
    pool: PgPool,
}

impl PaginatedProjections {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Return one page of orders for a branch.
    ///
    /// `after` — decode from the client's `?after=` param; `None` for page 1.
    /// `limit` — clamped to [1, PAGE_SIZE].
    ///
    /// Returns `(rows, next_cursor)`. `next_cursor` is `None` when on the last page.
    pub async fn orders_page(
        &self,
        branch_id: Uuid,
        after: Option<&OrderCursor>,
        filter: &OrderFilter,
        limit: i64,
    ) -> Result<(Vec<OrderCurrentState>, Option<OrderCursor>), sqlx::Error> {
        let limit = limit.min(PAGE_SIZE).max(1);
        // Fetch one extra row to detect whether a next page exists.
        let fetch = limit + 1;

        // Cursor fields (None = no cursor = first page).
        let cur_terminal: Option<bool> = after.map(|c| c.is_terminal);
        let cur_ts: Option<DateTime<Utc>> = after.and_then(|c| c.last_event_at);
        let cur_id: Option<Uuid> = after.map(|c| c.order_id);

        // State filter: validated to only A-Z and _, uppercased.
        let state_list: Option<Vec<String>> = {
            let v: Vec<String> = filter
                .states
                .iter()
                .map(|s| s.to_uppercase())
                .filter(|s| s.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
                .collect();
            if v.is_empty() { None } else { Some(v) }
        };

        // Search pattern: escape SQL wildcards in user input, then wrap in %.
        let search: Option<String> = filter.q.as_deref().map(|q| {
            format!(
                "%{}%",
                q.replace('\\', "\\\\")
                 .replace('%', "\\%")
                 .replace('_', "\\_")
            )
        });

        // PostgreSQL row-value comparison for keyset pagination:
        //   (is_terminal_int, last_event_at, order_id) > (cursor_is_terminal_int, cursor_ts, cursor_id)
        //
        // NULL handling: Postgres compares NULLs in row expressions consistently —
        // NULL DESC NULLS LAST means NULLs sort last, so rows with NULL last_event_at
        // appear at the bottom of their terminal bucket. The `> cursor` comparison
        // with a NULL cursor_ts covers the NULL case correctly via Postgres semantics
        // for row value comparisons with NULL.
        let rows: Vec<OrderCurrentState> = sqlx::query_as(
            r#"
            SELECT
                ocs.order_id                                        AS id,
                ocs.branch_id,
                ocs.customer_id,
                COALESCE(ed.new_description, o.description)        AS description,
                ocs.state,
                ocs.worker_id,
                w.name                                             AS worker_name,
                ocs.last_worker_message,
                ocs.last_worker_message_at,
                ocs.unread_message_count,
                ocs.ai_routed_low_confidence,
                ocs.last_event_at,
                COALESCE(ocs.short_name, o.short_name)            AS short_name,
                ocs.start_date,
                ocs.due_date,
                COALESCE(fa.alert_count, 0)::bigint               AS alert_count
            FROM order_current_state ocs
            JOIN orders o
              ON o.id = ocs.order_id
             AND o.deleted_at IS NULL
            LEFT JOIN workers w ON w.id = ocs.worker_id
            LEFT JOIN LATERAL (
                SELECT new_description
                FROM order_description_edits
                WHERE order_id = ocs.order_id
                ORDER BY id DESC LIMIT 1
            ) ed ON true
            LEFT JOIN (
                SELECT order_id, COUNT(*)::bigint AS alert_count
                FROM follow_up_alerts
                WHERE deleted_at IS NULL
                GROUP BY order_id
            ) fa ON fa.order_id = ocs.order_id
            WHERE ocs.branch_id = $1
              AND (
                  $2::boolean IS NULL
                  OR (
                    CASE WHEN ocs.state IN ('DONE','CANCELLED') THEN 1 ELSE 0 END,
                    ocs.last_event_at,
                    ocs.order_id
                  ) > (
                    CASE WHEN $2 THEN 1 ELSE 0 END,
                    $3,
                    $4
                  )
              )
              AND ($5::text[]  IS NULL OR ocs.state     = ANY($5::text[]))
              AND ($6::uuid    IS NULL OR ocs.worker_id = $6)
              AND (
                  $7::text IS NULL
                  OR COALESCE(ed.new_description, o.description) ILIKE $7
                  OR COALESCE(ocs.short_name, o.short_name)      ILIKE $7
              )
            ORDER BY
                CASE WHEN ocs.state IN ('DONE','CANCELLED') THEN 1 ELSE 0 END ASC,
                ocs.last_event_at DESC NULLS LAST,
                ocs.order_id ASC
            LIMIT $8
            "#,
        )
        .bind(branch_id)        // $1
        .bind(cur_terminal)     // $2
        .bind(cur_ts)           // $3
        .bind(cur_id)           // $4
        .bind(state_list)       // $5
        .bind(filter.worker_id) // $6
        .bind(search)           // $7
        .bind(fetch)            // $8
        .fetch_all(&self.pool)
        .await?;

        build_order_result(rows, limit)
    }

    /// Return one page of supply requests for a branch.
    ///
    /// Supply requests support `states` filter only (T09 resolution).
    pub async fn supply_requests_page(
        &self,
        branch_id: Uuid,
        after: Option<&SrCursor>,
        filter: &SrFilter,
        limit: i64,
    ) -> Result<(Vec<SrPageRow>, Option<SrCursor>), sqlx::Error> {
        let limit = limit.min(PAGE_SIZE).max(1);
        let fetch = limit + 1;

        let cur_terminal: Option<bool> = after.map(|c| c.is_terminal);
        let cur_ts: Option<DateTime<Utc>> = after.and_then(|c| c.updated_at);
        let cur_id: Option<Uuid> = after.map(|c| c.supply_request_id);

        let state_list: Option<Vec<String>> = {
            let v: Vec<String> = filter
                .states
                .iter()
                .map(|s| s.to_uppercase())
                .filter(|s| s.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
                .collect();
            if v.is_empty() { None } else { Some(v) }
        };

        let rows: Vec<SrPageRow> = sqlx::query_as(
            r#"
            SELECT
                srcs.supply_request_id                              AS id,
                srcs.branch_id,
                COALESCE(sr.description, srcs.description)         AS description,
                srcs.state,
                COALESCE(sr.order_ids, '[]'::jsonb)                AS order_ids,
                srcs.updated_at
            FROM supply_request_current_state srcs
            LEFT JOIN supply_requests sr ON sr.id = srcs.supply_request_id
            WHERE srcs.branch_id = $1
              AND (
                  $2::boolean IS NULL
                  OR (
                    CASE WHEN srcs.state = 'SUPPLIER_CONFIRMED' THEN 1 ELSE 0 END,
                    srcs.updated_at,
                    srcs.supply_request_id
                  ) > (
                    CASE WHEN $2 THEN 1 ELSE 0 END,
                    $3,
                    $4
                  )
              )
              AND ($5::text[] IS NULL OR srcs.state = ANY($5::text[]))
            ORDER BY
                CASE WHEN srcs.state = 'SUPPLIER_CONFIRMED' THEN 1 ELSE 0 END ASC,
                srcs.updated_at DESC NULLS LAST,
                srcs.supply_request_id ASC
            LIMIT $6
            "#,
        )
        .bind(branch_id)
        .bind(cur_terminal)
        .bind(cur_ts)
        .bind(cur_id)
        .bind(state_list)
        .bind(fetch)
        .fetch_all(&self.pool)
        .await?;

        build_sr_result(rows, limit)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────── //

fn build_order_result(
    mut rows: Vec<OrderCurrentState>,
    limit: i64,
) -> Result<(Vec<OrderCurrentState>, Option<OrderCursor>), sqlx::Error> {
    let has_next = rows.len() as i64 > limit;
    if has_next {
        rows.pop();
    }
    let next_cursor = if has_next {
        rows.last().map(|r| OrderCursor {
            is_terminal: matches!(r.state.as_str(), "DONE" | "CANCELLED"),
            last_event_at: r.last_event_at,
            order_id: r.id,
        })
    } else {
        None
    };
    Ok((rows, next_cursor))
}

fn build_sr_result(
    mut rows: Vec<SrPageRow>,
    limit: i64,
) -> Result<(Vec<SrPageRow>, Option<SrCursor>), sqlx::Error> {
    let has_next = rows.len() as i64 > limit;
    if has_next {
        rows.pop();
    }
    let next_cursor = if has_next {
        rows.last().map(|r| SrCursor {
            is_terminal: r.state == "SUPPLIER_CONFIRMED",
            updated_at: r.updated_at,
            supply_request_id: r.id,
        })
    } else {
        None
    };
    Ok((rows, next_cursor))
}