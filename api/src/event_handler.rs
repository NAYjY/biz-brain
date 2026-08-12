//! EventHandler fan-out (CONTEXT.md: "updates state in the database, and
//! fans out notifications to affected parties").
//!
//! Called from two places:
//!   - `inbox_worker` after Agent produces a DomainEvent (Worker/Supplier messages)
//!   - `api::routes::commands` after Owner triggers a DomainEvent (dashboard actions)
//!
//! Notification matrix:
//!   WorkerAssigned       → LINE push to Worker: "You have a new order"
//!   WorkerAccepted       → Owner: SSE already handles dashboard update (no extra push)
//!   WorkerUnavailable    → Owner: SSE handles it
//!   WorkerCancelled      → Owner: SSE handles it (URGENT shown via state pill)
//!   ClarificationRequested → Owner: SSE handles it
//!   WorkerReadyForPickup → Owner: SSE handles it
//!   OrderDone            → no outbound (Owner action, Worker already knows)
//!   SupplyRequestSent    → WhatsApp push to Supplier: "New supply request"
//!   InvoiceReceived      → Owner: SSE handles it
//!   InvoiceApproved      → WhatsApp push to Supplier: "Invoice approved, please confirm"
//!   SupplierConfirmed    → Owner: SSE handles it

use domain::{BranchId, Channel, ChannelIdentity, DomainEvent};
use messaging::ChannelAdapter;
use sqlx::PgPool;
use uuid::Uuid;

use crate::state::AppState;

/// Fan out notifications for a domain event that just fired.
/// Errors are logged but never propagate — a failed push must not
/// roll back the event that already landed in the store.
pub async fn fan_out(state: &AppState, event: &DomainEvent) {
    if let Err(e) = try_fan_out(state, event).await {
        tracing::error!("fan_out failed for {event}: {e}");
    }
}

async fn try_fan_out(state: &AppState, event: &DomainEvent) -> Result<(), Box<dyn std::error::Error>> {
    match event {
        // ── Worker receives a push ───────────────────────────────────── //

        DomainEvent::WorkerAssigned { worker_id, order_id } => {
            let desc = order_description(&state.pool, order_id.into_inner()).await?;
            let identity = worker_identity(&state.pool, worker_id.into_inner()).await?;
            let text = format!("📋 You have a new order: {desc}\nPlease reply 'accept' to confirm or 'unavailable' if you can't take it.");
            send_to_identity(state, &identity, &text).await;
        }

        // ── Supplier receives a push ─────────────────────────────────── //

        DomainEvent::SupplyRequestSent { supply_request_id, branch_id } => {
            let desc = supply_request_description(&state.pool, supply_request_id.into_inner()).await?;
            let identities = supplier_identities_for_branch(&state.pool, branch_id.into_inner()).await?;
            let text = format!("📦 New supply request: {desc}\nPlease reply with your invoice and available items.");
            for identity in identities {
                send_to_identity(state, &identity, &text).await;
            }
        }

        DomainEvent::InvoiceApproved { invoice_id, branch_id } => {
            let identity = supplier_identity_for_invoice(&state.pool, invoice_id.into_inner()).await?;
            let text = "✅ Your invoice has been approved. Please confirm stock availability to proceed.".to_string();
            send_to_identity(state, &identity, &text).await;
        }

        // ── Owner notified via SSE (dashboard updates automatically) ─── //
        // WorkerAccepted, WorkerUnavailable, WorkerCancelled,
        // ClarificationRequested, WorkerReadyForPickup, OrderDone,
        // InvoiceReceived, SupplierConfirmed — SSE signal already fired
        // from projection_worker; no extra push needed.
        _ => {}
    }

    Ok(())
}

async fn send_to_identity(state: &AppState, identity: &ChannelIdentity, text: &str) {
    let result = match identity.channel {
        Channel::Line => state.line.send_push(identity, text).await,
        Channel::WhatsApp => state.whatsapp.send_push(identity, text).await,
        Channel::Telegram => state.telegram.send_push(identity, text).await,
    };
    if let Err(e) = result {
        tracing::error!("push to {:?} failed: {e}", identity);
    }
}

// ── DB helpers ────────────────────────────────────────────────────────── //

async fn order_description(pool: &sqlx::PgPool, order_id: Uuid) -> Result<String, sqlx::Error> {
    let row: (String,) = sqlx::query_as("SELECT description FROM orders WHERE id = $1")
        .bind(order_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

async fn supply_request_description(pool: &sqlx::PgPool, sr_id: Uuid) -> Result<String, sqlx::Error> {
    let row: (String,) = sqlx::query_as("SELECT description FROM supply_requests WHERE id = $1")
        .bind(sr_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Get the confirmed channel identity for a Worker.
async fn worker_identity(pool: &sqlx::PgPool, worker_id: Uuid) -> Result<ChannelIdentity, Box<dyn std::error::Error>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT channel, external_id FROM actor_directory \
         WHERE actor_id = $1 AND actor_type = 'worker' AND owner_confirmed = TRUE",
    )
    .bind(worker_id)
    .fetch_optional(pool)
    .await?;

    let (channel_str, external_id) = row.ok_or_else(|| {
        format!("no confirmed binding for worker {worker_id}")
    })?;

    let channel = parse_channel(&channel_str)?;
    Ok(ChannelIdentity { channel, external_id })
}

/// Get confirmed channel identity for the Supplier who sent an Invoice.
async fn supplier_identity_for_invoice(
    pool: &sqlx::PgPool,
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

    let (channel_str, external_id) = row.ok_or_else(|| {
        format!("no confirmed binding for invoice {invoice_id}'s supplier")
    })?;

    Ok(ChannelIdentity { channel: parse_channel(&channel_str)?, external_id })
}

/// All confirmed Supplier identities for a Branch (SupplyRequestSent goes to all).
async fn supplier_identities_for_branch(
    pool: &sqlx::PgPool,
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
        .map(|(ch, ext)| Ok(ChannelIdentity { channel: parse_channel(&ch)?, external_id: ext }))
        .collect()
}

fn parse_channel(s: &str) -> Result<Channel, Box<dyn std::error::Error>> {
    match s {
        "line" => Ok(Channel::Line),
        "whats_app" => Ok(Channel::WhatsApp),
        "telegram" => Ok(Channel::Telegram),
        other => Err(format!("unknown channel: {other}").into()),
    }
}