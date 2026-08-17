//! EventHandler fan-out — fires push notifications after each DomainEvent.
//!
//! P04: OwnerCancelled pushes a cancellation notice to the Worker;
//! ClarificationResolved pushes confirmation back to the Worker.
//! OrderReset has no outbound push (no Worker assigned at reset time).
//!
//! Notification matrix:
//!   WorkerAssigned           → LINE/Telegram push to Worker
//!   OwnerCancelled           → push to Worker (if assigned)
//!   ClarificationResolved    → push to Worker: "Owner has responded"
//!   SupplyRequestSent        → WhatsApp push to all Suppliers in Branch
//!   InvoiceApproved          → WhatsApp push to Supplier
//!   All others               → SSE-only (dashboard auto-refreshes)
//!
//! Errors are logged but never propagate — a failed push must not roll back
//! the event that already landed in the store.

use domain::{BranchId, Channel, ChannelIdentity, DomainEvent};
use messaging::ChannelAdapter;
use sqlx::PgPool;
use uuid::Uuid;

use crate::state::AppState;

pub async fn fan_out(state: &AppState, event: &DomainEvent) {
    if let Err(e) = try_fan_out(state, event).await {
        tracing::error!("fan_out failed for {event}: {e}");
    }
}

async fn try_fan_out(
    state: &AppState,
    event: &DomainEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    match event {
        // ── Worker receives a push ───────────────────────────────────── //

        DomainEvent::WorkerAssigned { worker_id, order_id } => {
            let desc = order_description(&state.pool, order_id.into_inner()).await?;
            let identity = worker_identity(&state.pool, worker_id.into_inner()).await?;
            let text = format!(
                "📋 You have a new order: {desc}\n\
                 Reply 'รับงาน' (accept) or 'ไม่ว่าง' (unavailable)."
            );
            send_to_identity(state, &identity, &text).await;
        }

        // P04: Owner cancelled — notify the assigned Worker if any.
        DomainEvent::OwnerCancelled { order_id } => {
            let desc = order_description(&state.pool, order_id.into_inner()).await?;
            if let Ok(identity) = assigned_worker_identity(&state.pool, order_id.into_inner()).await {
                let text = format!("❌ Order cancelled by Owner: {desc}");
                send_to_identity(state, &identity, &text).await;
            }
            // No Worker assigned → no push needed.
        }

        // P04: Clarification resolved — push the Owner's reply back to Worker.
        DomainEvent::ClarificationResolved { worker_id, order_id } => {
            let desc = order_description(&state.pool, order_id.into_inner()).await?;
            let identity = worker_identity(&state.pool, worker_id.into_inner()).await?;
            let text = format!(
                "✅ Owner has responded to your question about: {desc}\n\
                 Order is back to Assigned. Please proceed."
            );
            send_to_identity(state, &identity, &text).await;
        }

        // ── Supplier receives a push ─────────────────────────────────── //

        DomainEvent::SupplyRequestSent { supply_request_id, branch_id } => {
            let desc = supply_request_description(&state.pool, supply_request_id.into_inner()).await?;
            let identities =
                supplier_identities_for_branch(&state.pool, branch_id.into_inner()).await?;
            let text = format!(
                "📦 New supply request: {desc}\n\
                 Please reply with your invoice / price list."
            );
            for identity in identities {
                send_to_identity(state, &identity, &text).await;
            }
        }

        DomainEvent::InvoiceApproved { invoice_id, .. } => {
            let identity =
                supplier_identity_for_invoice(&state.pool, invoice_id.into_inner()).await?;
            let text = "✅ Your invoice has been approved. \
                        Please reply 'ยืนยัน' (confirm) to proceed.";
            send_to_identity(state, &identity, text).await;
        }

        // ── All others: SSE handles dashboard refresh ────────────────── //
        _ => {}
    }

    Ok(())
}

// ── Send helper ──────────────────────────────────────────────────────────── //

async fn send_to_identity(state: &AppState, identity: &ChannelIdentity, text: &str) {
    let result = match identity.channel {
        Channel::Line      => state.line.send_push(identity, text).await,
        Channel::WhatsApp  => state.whatsapp.send_push(identity, text).await,
        Channel::Telegram  => state.telegram.send_push(identity, text).await,
    };
    if let Err(e) = result {
        tracing::error!("push to {} failed: {e}", identity.external_id);
    }
}

// ── DB helpers ────────────────────────────────────────────────────────────── //

async fn order_description(pool: &PgPool, order_id: Uuid) -> Result<String, sqlx::Error> {
    let (desc,): (String,) =
        sqlx::query_as("SELECT description FROM orders WHERE id = $1")
            .bind(order_id)
            .fetch_one(pool)
            .await?;
    Ok(desc)
}

async fn supply_request_description(pool: &PgPool, sr_id: Uuid) -> Result<String, sqlx::Error> {
    let (desc,): (String,) =
        sqlx::query_as("SELECT description FROM supply_requests WHERE id = $1")
            .bind(sr_id)
            .fetch_one(pool)
            .await?;
    Ok(desc)
}

async fn worker_identity(
    pool: &PgPool,
    worker_id: Uuid,
) -> Result<ChannelIdentity, Box<dyn std::error::Error>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT channel, external_id FROM actor_directory \
         WHERE actor_id = $1 AND actor_type = 'worker' AND owner_confirmed = TRUE",
    )
    .bind(worker_id)
    .fetch_optional(pool)
    .await?;

    let (channel_str, external_id) =
        row.ok_or_else(|| format!("no confirmed binding for worker {worker_id}"))?;

    Ok(ChannelIdentity { channel: parse_channel(&channel_str)?, external_id })
}

/// P04: get the currently assigned Worker's identity, if any.
async fn assigned_worker_identity(
    pool: &PgPool,
    order_id: Uuid,
) -> Result<ChannelIdentity, Box<dyn std::error::Error>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT worker_id FROM order_current_state WHERE order_id = $1 AND worker_id IS NOT NULL",
    )
    .bind(order_id)
    .fetch_optional(pool)
    .await?;

    let (worker_id,) =
        row.ok_or_else(|| format!("no worker assigned to order {order_id}"))?;

    worker_identity(pool, worker_id).await
}

async fn supplier_identity_for_invoice(
    pool: &PgPool,
    invoice_id: Uuid,
) -> Result<ChannelIdentity, Box<dyn std::error::Error>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT ad.channel, ad.external_id \
         FROM invoices i \
         JOIN actor_directory ad \
           ON ad.actor_id = i.supplier_id \
           AND ad.actor_type = 'supplier' \
           AND ad.owner_confirmed = TRUE \
         WHERE i.id = $1",
    )
    .bind(invoice_id)
    .fetch_optional(pool)
    .await?;

    let (channel_str, external_id) =
        row.ok_or_else(|| format!("no confirmed binding for invoice {invoice_id}'s supplier"))?;

    Ok(ChannelIdentity { channel: parse_channel(&channel_str)?, external_id })
}

async fn supplier_identities_for_branch(
    pool: &PgPool,
    branch_id: Uuid,
) -> Result<Vec<ChannelIdentity>, Box<dyn std::error::Error>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT ad.channel, ad.external_id \
         FROM suppliers s \
         JOIN actor_directory ad \
           ON ad.actor_id = s.id \
           AND ad.actor_type = 'supplier' \
           AND ad.owner_confirmed = TRUE \
         WHERE s.branch_id = $1",
    )
    .bind(branch_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|(ch, ext)| {
            Ok(ChannelIdentity { channel: parse_channel(&ch)?, external_id: ext })
        })
        .collect()
}

fn parse_channel(s: &str) -> Result<Channel, Box<dyn std::error::Error>> {
    match s {
        "line"      => Ok(Channel::Line),
        "whats_app" => Ok(Channel::WhatsApp),
        "telegram"  => Ok(Channel::Telegram),
        other       => Err(format!("unknown channel: {other}").into()),
    }
}
