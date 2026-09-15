//! Shared helpers for the inbox_worker module family.
//! Pure utilities: DB queries, send, event append, resolution logic.

use agent::classify::ActiveOrderContext;
use domain::{BranchId, Channel, ChannelIdentity, DomainEvent, InvoiceId, OrderId, SupplierId, SupplyRequestId};
use messaging::ChannelAdapter;
use store::conversation_history::ConversationHistoryRepository;
use store::reply_templates::ReplyTemplateRepository;

use crate::event_handler;
use crate::state::AppState;

// ── Resolution enums ──────────────────────────────────────────────────────── //

pub(super) enum ResolvedOrder {
    Single(OrderId),
    NeedsDisambiguation(Vec<OrderId>),
    NoActiveOrders,
}

pub(super) enum SupplyRequestResolved {
    Single(SupplyRequestId),
    NeedsDisambiguation(Vec<SupplyRequestId>),
    None,
}

// ── Pure logic ────────────────────────────────────────────────────────────── //

pub(super) fn resolve_order_id(
    from_ai: Option<uuid::Uuid>,
    active_ids: &[OrderId],
) -> ResolvedOrder {
    match active_ids {
        [] => ResolvedOrder::NoActiveOrders,
        [only] => ResolvedOrder::Single(*only),
        many => {
            if let Some(id) = from_ai {
                let oid = OrderId::new(id);
                if many.contains(&oid) {
                    return ResolvedOrder::Single(oid);
                }
            }
            ResolvedOrder::NeedsDisambiguation(many.to_vec())
        }
    }
}

pub(super) fn resolve_supply_request(
    from_ai: Option<uuid::Uuid>,
    active: &[ActiveOrderContext],
) -> SupplyRequestResolved {
    match active {
        [] => SupplyRequestResolved::None,
        [only] => SupplyRequestResolved::Single(SupplyRequestId::new(only.order_id)),
        many => {
            if let Some(id) = from_ai {
                if many.iter().any(|ctx| ctx.order_id == id) {
                    return SupplyRequestResolved::Single(SupplyRequestId::new(id));
                }
            }
            SupplyRequestResolved::NeedsDisambiguation(
                many.iter().map(|ctx| SupplyRequestId::new(ctx.order_id)).collect(),
            )
        }
    }
}

pub(super) fn is_affirmative(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "yes" | "y" | "ใช่" | "ใช่ครับ" | "ใช่ค่ะ" | "ok" | "โอเค" | "correct" | "right"
    )
}

pub(super) fn is_negative(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "no" | "n" | "ไม่" | "ไม่ใช่" | "ไม่ครับ" | "ไม่ค่ะ" | "nope" | "wrong"
    )
}

pub(super) fn to_history_msg(row: store::HistoryRow) -> agent::HistoryMessage {
    agent::HistoryMessage { role: row.role, content: row.content }
}

// ── Send helpers ──────────────────────────────────────────────────────────── //

pub(super) async fn send_to_sender(state: &AppState, sender: &ChannelIdentity, text: &str) {
    let result = match sender.channel {
        Channel::Line     => state.line.send_push(sender, text).await,
        Channel::WhatsApp => state.whatsapp.send_push(sender, text).await,
        Channel::Telegram => state.telegram.send_push(sender, text).await,
    };
    if let Err(e) = result {
        tracing::error!("push to {} failed: {e}", sender.external_id);
    }

    let sender_key = ConversationHistoryRepository::sender_key(
        sender.channel.as_sql(),
        &sender.external_id,
    );
    let _ = ConversationHistoryRepository::new(state.pool.clone())
        .append(&sender_key, "assistant", text)
        .await;
}

pub(super) async fn fetch_reply_template(
    state: &AppState,
    order_id: OrderId,
    event: &DomainEvent,
) -> String {
    let event_type = event.variant().as_sql();
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT branch_id FROM orders WHERE id = $1")
            .bind(order_id.into_inner())
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();

    if let Some((bid,)) = row {
        if let Ok(tmpl) = ReplyTemplateRepository::new(state.pool.clone())
            .fetch(bid, event_type)
            .await
        {
            return tmpl;
        }
    }
    format!("Done ✓ ({})", event_type)
}

// ── DB helpers ────────────────────────────────────────────────────────────── //

pub(super) async fn order_display_fields(
    state: &AppState,
    id: uuid::Uuid,
) -> Result<(Option<String>, String), sqlx::Error> {
    let row: (Option<String>, String) = sqlx::query_as(
        "SELECT o.short_name, \
                COALESCE( \
                    (SELECT new_description FROM order_description_edits \
                     WHERE order_id = $1 ORDER BY id DESC LIMIT 1), \
                    o.description \
                ) \
         FROM orders o WHERE o.id = $1",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(row)
}

pub(super) async fn build_order_contexts(
    state: &AppState,
    order_ids: &[OrderId],
) -> Result<Vec<ActiveOrderContext>, Box<dyn std::error::Error>> {
    let mut out = Vec::with_capacity(order_ids.len());
    for &oid in order_ids {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT state FROM order_current_state WHERE order_id = $1",
        )
        .bind(oid.into_inner())
        .fetch_optional(&state.pool)
        .await?;

        if let Some((st,)) = row {
            let (_, desc) = order_display_fields(state, oid.into_inner()).await?;
            out.push(ActiveOrderContext {
                order_id: oid.into_inner(),
                description: desc,
                state: st,
            });
        }
    }
    Ok(out)
}

pub(super) async fn active_supply_request_contexts(
    state: &AppState,
    supplier_id: SupplierId,
) -> Result<Vec<ActiveOrderContext>, sqlx::Error> {
    let rows: Vec<(uuid::Uuid, String, String)> = sqlx::query_as(
        "SELECT srcs.supply_request_id, srcs.state, sr.description \
         FROM supply_request_current_state srcs \
         JOIN supply_requests sr ON sr.id = srcs.supply_request_id \
         JOIN actor_directory ad \
           ON ad.actor_id = $1 \
           AND ad.actor_type = 'supplier' \
           AND ad.owner_confirmed = TRUE \
           AND ad.branch_id = srcs.branch_id \
         WHERE srcs.branch_id = ad.branch_id \
           AND srcs.state NOT IN ('SUPPLIER_CONFIRMED')",
    )
    .bind(supplier_id.into_inner())
    .fetch_all(&state.pool)
    .await?;

    Ok(rows.into_iter().map(|(id, st, desc)| {
        ActiveOrderContext { order_id: id, description: desc, state: st }
    }).collect())
}

pub(super) async fn supply_request_description(
    state: &AppState,
    id: uuid::Uuid,
) -> Result<String, sqlx::Error> {
    let (desc,): (String,) =
        sqlx::query_as("SELECT description FROM supply_requests WHERE id = $1")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    Ok(desc)
}

pub(super) async fn branch_for_supply_request(
    state: &AppState,
    id: uuid::Uuid,
) -> Result<uuid::Uuid, sqlx::Error> {
    let (bid,): (uuid::Uuid,) =
        sqlx::query_as("SELECT branch_id FROM supply_requests WHERE id = $1")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    Ok(bid)
}

pub(super) async fn approved_invoice_for_supply_request(
    state: &AppState,
    sr_id: SupplyRequestId,
) -> Result<InvoiceId, Box<dyn std::error::Error>> {
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT invoice_id FROM invoice_current_state \
         WHERE supply_request_id = $1 AND state = 'OwnerApproved'",
    )
    .bind(sr_id.into_inner())
    .fetch_optional(&state.pool)
    .await?;

    row.map(|(id,)| InvoiceId::new(id))
        .ok_or_else(|| "no OwnerApproved invoice for supply request".into())
}

pub(super) async fn create_invoice_and_fetch_media(
    state: &AppState,
    sender: &ChannelIdentity,
    supplier_id: SupplierId,
    supply_request_id: SupplyRequestId,
    media_id: Option<&str>,
) -> Result<InvoiceId, Box<dyn std::error::Error>> {
    let invoice_id = InvoiceId::generate();
    let branch_id = branch_for_supply_request(state, supply_request_id.into_inner()).await?;

    sqlx::query(
        "INSERT INTO invoices (id, supply_request_id, branch_id, supplier_id) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(invoice_id.into_inner())
    .bind(supply_request_id.into_inner())
    .bind(branch_id)
    .bind(supplier_id.into_inner())
    .execute(&state.pool)
    .await?;

    if let Some(mid) = media_id {
        if sender.channel == Channel::WhatsApp {
            match state.whatsapp.fetch_media(mid).await {
                Ok(blob) => {
                    sqlx::query(
                        "UPDATE invoices SET media_data = $1, media_mime_type = $2 WHERE id = $3",
                    )
                    .bind(&blob.data)
                    .bind(&blob.mime_type)
                    .bind(invoice_id.into_inner())
                    .execute(&state.pool)
                    .await?;
                }
                Err(e) => tracing::warn!("media fetch failed for invoice {invoice_id}: {e}"),
            }
        }
    }

    Ok(invoice_id)
}

// ── Event append ──────────────────────────────────────────────────────────── //

pub(super) async fn append_order_event(
    state: &AppState,
    event: DomainEvent,
    order_id: OrderId,
) -> Result<(), Box<dyn std::error::Error>> {
    let seq = state.order_events.current_sequence(order_id).await?;
    let (branch_id,): (uuid::Uuid,) =
        sqlx::query_as("SELECT branch_id FROM orders WHERE id = $1")
            .bind(order_id.into_inner())
            .fetch_one(&state.pool)
            .await?;

    state.event_sourcing.append(BranchId::new(branch_id), seq + 1, &event).await?;
    event_handler::fan_out(state, &event).await;
    let signal = state.projection_worker.project_order(order_id).await?;
    state.publish_sse(signal).await;
    Ok(())
}

pub(super) async fn append_supply_request_event(
    state: &AppState,
    event: DomainEvent,
    branch_id: BranchId,
) -> Result<(), Box<dyn std::error::Error>> {
    let sr_id = event
        .supply_request_id()
        .expect("Supplier-agent events always carry supply_request_id");
    let seq = state.supply_request_events.current_sequence(sr_id).await?;
    state.event_sourcing.append(branch_id, seq + 1, &event).await?;
    event_handler::fan_out(state, &event).await;
    let signal = state.projection_worker.project_supply_request(sr_id).await?;
    state.publish_sse(signal).await;
    Ok(())
}