//! P14: unified inbox_worker routing flow.
//!
//! Worker path (LINE / Telegram):
//!   1. Resolve actor — fail-closed (register pending on unknown)
//!   2. Load active orders + write conversation_history (P13)
//!   3. Check disambiguation_pending FIRST — active yes/no flow takes priority
//!   4. Otherwise: P13 classify (variant + order_id)
//!   5. Clear → apply event; Ambiguous → start P02 yes/no flow
//!
//! Supplier path (WhatsApp):
//!   1. Resolve actor
//!   2. Check disambiguation_pending FIRST
//!   3. Otherwise: classify → InvoiceReceived or SupplierConfirmed
//!   4. P05: if message carries media_id, fetch + store bytes on invoice row
//!
//! P07: terminal events remove the order from ThreadContextStore.
//! F04: increment_unread on every inbound worker message; flag_low_confidence
//!      when prefilter misses and Claude routes with low certainty.
//! F01: display_name() used everywhere the bot refers to an order.

use std::time::Duration;

use agent::classify::ActiveOrderContext;
use agent::InterpretationError;
use domain::{
    BranchId, Channel, ChannelIdentity, DomainEvent, InvoiceId, OrderId, SupplierId,
    SupplyRequestId, WorkerId,
};
use messaging::ChannelAdapter;
use store::conversation_history::ConversationHistoryRepository;
use store::disambiguation::DisambiguationStore;
use store::reply_templates::ReplyTemplateRepository;

use crate::{event_handler, state::AppState};

fn to_history_msg(row: store::HistoryRow) -> agent::classify::HistoryMessage {
    agent::classify::HistoryMessage { role: row.role, content: row.content }
}

// ── F01: display name helper ─────────────────────────────────────────────── //

/// Returns `short_name` when set, otherwise the first 30 chars of `description`
/// with an ellipsis appended. Used in every bot outbound message that names an order.
pub fn display_name(short_name: Option<&str>, description: &str) -> String {
    match short_name.map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) => n.to_string(),
        None => {
            let truncated: String = description.chars().take(30).collect();
            if description.chars().count() > 30 {
                format!("{truncated}…")
            } else {
                truncated
            }
        }
    }
}

pub async fn run(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    loop {
        interval.tick().await;
        if let Err(e) = process_batch(&state).await {
            tracing::error!("inbox processing batch failed: {e}");
        }
    }
}

async fn process_batch(state: &AppState) -> Result<(), sqlx::Error> {
    let rows = state.inbox.fetch_unprocessed(50).await?;
    for row in rows {
        if let Err(e) = process_row(state, &row).await {
            tracing::error!("failed to process webhook_inbox row {}: {e}", row.id);
        }
        let _ = state.inbox.mark_processed(row.id).await;
    }
    Ok(())
}

async fn process_row(
    state: &AppState,
    row: &store::webhook_inbox::InboxRow,
) -> Result<(), Box<dyn std::error::Error>> {
    let sender: ChannelIdentity = serde_json::from_value(row.raw_payload["sender"].clone())?;
    let text = row.raw_payload["text"].as_str().unwrap_or_default();
    let media_id = row.raw_payload["media_id"].as_str().map(str::to_owned);

    let channel = match row.channel.as_str() {
        "line"      => Channel::Line,
        "whats_app" => Channel::WhatsApp,
        "telegram"  => Channel::Telegram,
        _           => return Ok(()),
    };

    match channel {
        Channel::Line | Channel::Telegram => {
            process_worker_message(state, &sender, text).await
        }
        Channel::WhatsApp => {
            process_supplier_message(state, &sender, text, media_id.as_deref()).await
        }
    }
}

// ── Worker path ──────────────────────────────────────────────────────────── //

async fn process_worker_message(
    state: &AppState,
    sender: &ChannelIdentity,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(worker_id) = state.actors.resolve_worker(sender).await? else {
        tracing::warn!(external_id = %sender.external_id, "unknown Worker — registering pending");
        state.actors.register_pending(sender, store::actor_directory::ActorType::Worker).await?;
        return Ok(());
    };

    let sender_key = ConversationHistoryRepository::sender_key(
        sender.channel.as_sql(),
        &sender.external_id,
    );
    let history_repo = ConversationHistoryRepository::new(state.pool.clone());
    let disambig_store = DisambiguationStore::new(state.pool.clone());

    let history_rows = history_repo.load(&sender_key).await?;
    let history: Vec<agent::classify::HistoryMessage> =
        history_rows.into_iter().map(to_history_msg).collect();

    let active_order_ids = state.actors.active_orders_for_worker(worker_id).await?;

    // Persist incoming message immediately (P13).
    history_repo.append(&sender_key, "user", text).await?;

    // Attach active orders to in-memory ThreadContextStore.
    {
        let mut threads = state.threads.lock().await;
        for oid in &active_order_ids {
            threads.add_active_order(sender.clone(), *oid);
        }
    }

    // P14: check disambiguation_pending FIRST.
    if let Some(pending) = disambig_store.find(&sender_key).await? {
        return handle_worker_disambiguation_reply(
            state, sender, worker_id, &sender_key, text,
            &pending, &history_repo, &disambig_store,
        )
        .await;
    }

    // P13/P14: classify with history + order contexts.
    let active_contexts = build_order_contexts(state, &active_order_ids).await?;

    // Track whether we fell through to the Claude classifier (prefilter miss).
    let prefilter_hit = state.worker_agent.prefilter_hit(text);
    let classify_result = state.worker_agent.classify(text, &history, &active_contexts).await;

    match classify_result {
        Err(InterpretationError::UnexpectedVariant { received, allowed }) => {
            tracing::error!(owner_alert = true,
                "unexpected variant '{received}' (allowed: {allowed}) for Worker");
            send_to_sender(state, sender,
                "Sorry, I didn't understand that. The Owner has been alerted.").await;
        }
        Err(e) => {
            tracing::error!(owner_alert = true, "Worker classify error: {e}");
            send_to_sender(state, sender,
                "Something went wrong. The Owner has been alerted.").await;
        }
        Ok(None) => {
            send_to_sender(state, sender,
                "Sorry, I didn't understand that. \
                 The Owner can see your message and will respond.").await;
        }
        Ok(Some((variant, resolved_order_id))) => {
            let order_id = resolve_order_id(resolved_order_id, &active_order_ids);

            match order_id {
                ResolvedOrder::Single(oid) => {
                    disambig_store.delete(&sender_key).await?;
                    let event = agent::construct_worker_event(variant, worker_id, oid);

                    if event.is_terminal_for_worker() {
                        let mut threads = state.threads.lock().await;
                        threads.remove_active_order(sender, oid);
                    }

                    // Save message against the confirmed order so Owner sees it under the right row.
                    let _ = state.projections.update_worker_message(oid, text).await;

                    // F04: increment unread count so thread button badge updates.
                    let _ = state.projections.increment_unread(oid).await;

                    // F04: flag low confidence if we had to use Claude (prefilter miss)
                    // AND the order_id came from Claude rather than being unambiguous.
                    if !prefilter_hit && resolved_order_id.is_some() {
                        let _ = state.projections.flag_low_confidence(oid).await;
                    }

                    let reply = fetch_reply_template(state, oid, &event).await;
                    send_to_sender(state, sender, &reply).await;
                    append_order_event(state, event, oid).await?;
                }
                ResolvedOrder::NeedsDisambiguation(candidates) => {
                    start_worker_disambiguation(
                        state, sender, &sender_key, text,
                        candidates, &active_contexts, &disambig_store, &history_repo,
                    ).await?;
                }
                ResolvedOrder::NoActiveOrders => {
                    send_to_sender(state, sender, "You have no active orders at the moment.").await;
                }
            }
        }
    }

    Ok(())
}

async fn handle_worker_disambiguation_reply(
    state: &AppState,
    sender: &ChannelIdentity,
    worker_id: WorkerId,
    sender_key: &str,
    reply_text: &str,
    pending: &store::disambiguation::DisambiguationRow,
    history_repo: &ConversationHistoryRepository,
    disambig_store: &DisambiguationStore,
) -> Result<(), Box<dyn std::error::Error>> {
    if !is_affirmative(reply_text) && !is_negative(reply_text) {
        disambig_store.delete(sender_key).await?;
        send_to_sender(state, sender,
            "I'm not sure — please reply Yes or No. Which order is this about?").await;
        return Ok(());
    }

    if is_negative(reply_text) {
        disambig_store.advance(sender_key).await?;
        if let Some(row) = disambig_store.find(sender_key).await? {
            if let Some(next_id) = row.current_candidate() {
                // F01: use display_name in disambiguation question.
                let (short_name, desc) = order_display_fields(state, next_id).await?;
                let name = display_name(short_name.as_deref(), &desc);
                let q = format!("Is this about **{name}**? (Yes / No)");
                send_to_sender(state, sender, &q).await;
                return Ok(());
            }
        }
        // All candidates exhausted → ClarificationRequested on first candidate.
        disambig_store.delete(sender_key).await?;
        if let Some(oid) = pending.candidates().first().copied() {
            let event = DomainEvent::ClarificationRequested {
                worker_id,
                order_id: OrderId::new(oid),
            };
            send_to_sender(state, sender,
                "I've flagged this to the Owner. They'll be in touch shortly.").await;
            append_order_event(state, event, OrderId::new(oid)).await?;
        }
        return Ok(());
    }

    // Affirmative — confirm and re-classify original text on the pinned order.
    let confirmed_id = match pending.current_candidate() {
        Some(id) => OrderId::new(id),
        None => { disambig_store.delete(sender_key).await?; return Ok(()); }
    };
    disambig_store.delete(sender_key).await?;

    let history_rows = history_repo.load(sender_key).await?;
    let history: Vec<agent::classify::HistoryMessage> =
        history_rows.into_iter().map(to_history_msg).collect();

    let (short_name, desc) = order_display_fields(state, confirmed_id.into_inner()).await?;
    let ctx = vec![ActiveOrderContext {
        order_id: confirmed_id.into_inner(),
        description: desc.clone(),
        state: "confirmed".to_string(),
    }];

    let variant = state.worker_agent
        .classify(&pending.original_text, &history, &ctx).await
        .ok().flatten()
        .map(|(v, _)| v)
        .unwrap_or(domain::DomainEventVariant::ClarificationRequested);

    let event = agent::construct_worker_event(variant, worker_id, confirmed_id);

    if event.is_terminal_for_worker() {
        let mut threads = state.threads.lock().await;
        threads.remove_active_order(sender, confirmed_id);
    }

    let _ = state.projections.update_worker_message(confirmed_id, &pending.original_text).await;
    // F04: increment unread on the confirmed order (disambiguation was about this message).
    let _ = state.projections.increment_unread(confirmed_id).await;

    let reply = fetch_reply_template(state, confirmed_id, &event).await;
    send_to_sender(state, sender, &reply).await;
    append_order_event(state, event, confirmed_id).await?;

    Ok(())
}

async fn start_worker_disambiguation(
    state: &AppState,
    sender: &ChannelIdentity,
    sender_key: &str,
    original_text: &str,
    candidates: Vec<OrderId>,
    _contexts: &[ActiveOrderContext],
    disambig_store: &DisambiguationStore,
    history_repo: &ConversationHistoryRepository,
) -> Result<(), Box<dyn std::error::Error>> {
    let candidate_ids: Vec<uuid::Uuid> = candidates.iter().map(|id| id.into_inner()).collect();
    disambig_store.create(sender_key, original_text, &candidate_ids, "order").await?;

    let first = candidates.first().copied().unwrap();
    // F01: use display_name in first disambiguation question.
    let (short_name, desc) = order_display_fields(state, first.into_inner()).await?;
    let name = display_name(short_name.as_deref(), &desc);
    let question = format!("Is this about **{name}**? (Yes / No)");
    send_to_sender(state, sender, &question).await;
    Ok(())
}

// ── Supplier path ────────────────────────────────────────────────────────── //

async fn process_supplier_message(
    state: &AppState,
    sender: &ChannelIdentity,
    text: &str,
    media_id: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(supplier_id) = state.actors.resolve_supplier(sender).await? else {
        tracing::warn!(external_id = %sender.external_id, "unknown Supplier — registering pending");
        state.actors.register_pending(sender, store::actor_directory::ActorType::Supplier).await?;
        return Ok(());
    };

    let sender_key = ConversationHistoryRepository::sender_key(
        sender.channel.as_sql(),
        &sender.external_id,
    );
    let history_repo = ConversationHistoryRepository::new(state.pool.clone());
    let disambig_store = DisambiguationStore::new(state.pool.clone());

    history_repo.append(&sender_key, "user", text).await?;

    if let Some(pending) = disambig_store.find(&sender_key).await? {
        return handle_supplier_disambiguation_reply(
            state, sender, supplier_id, &sender_key, text,
            &pending, &history_repo, &disambig_store,
        ).await;
    }

    let history_rows = history_repo.load(&sender_key).await?;
    let history: Vec<agent::classify::HistoryMessage> =
        history_rows.into_iter().map(to_history_msg).collect();

    let active_srs = active_supply_request_contexts(state, supplier_id).await?;

    match state.supplier_agent.classify(text, &history, &active_srs).await {
        Err(InterpretationError::UnexpectedVariant { received, allowed }) => {
            tracing::error!(owner_alert = true,
                "unexpected supplier variant '{received}' (allowed: {allowed})");
            send_to_sender(state, sender, "Sorry, I didn't understand that.").await;
        }
        Err(e) => tracing::error!(owner_alert = true, "supplier classify error: {e}"),
        Ok(None) => {
            send_to_sender(state, sender, "Sorry, I didn't understand that.").await;
        }
        Ok(Some((domain::DomainEventVariant::InvoiceReceived, resolved_sr_id))) => {
            handle_invoice_received(
                state, sender, supplier_id, &sender_key,
                &active_srs, resolved_sr_id, media_id, &history_repo, &disambig_store,
            ).await?;
        }
        Ok(Some((domain::DomainEventVariant::SupplierConfirmed, resolved_sr_id))) => {
            handle_supplier_confirmed(
                state, sender, supplier_id, &sender_key,
                &active_srs, resolved_sr_id, &history_repo, &disambig_store,
            ).await?;
        }
        Ok(Some((other, _))) => {
            tracing::warn!("unexpected supplier variant: {other:?}");
        }
    }
    Ok(())
}

async fn handle_invoice_received(
    state: &AppState,
    sender: &ChannelIdentity,
    supplier_id: SupplierId,
    sender_key: &str,
    active_srs: &[ActiveOrderContext],
    resolved_sr_id: Option<uuid::Uuid>,
    media_id: Option<&str>,
    history_repo: &ConversationHistoryRepository,
    disambig_store: &DisambiguationStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let supply_request_id = match resolve_supply_request(resolved_sr_id, active_srs) {
        SupplyRequestResolved::Single(id) => id,
        SupplyRequestResolved::NeedsDisambiguation(candidates) => {
            let ids: Vec<uuid::Uuid> = candidates.iter().map(|id| id.into_inner()).collect();
            disambig_store.create(sender_key, "invoice", &ids, "supply_request").await?;
            let desc = supply_request_description(state, candidates[0].into_inner()).await?;
            let q = format!("Is this invoice for **{desc}**? (Yes / No)");
            send_to_sender(state, sender, &q).await;
            return Ok(());
        }
        SupplyRequestResolved::None => {
            send_to_sender(state, sender, "No active supply requests found.").await;
            return Ok(());
        }
    };

    let invoice_id = create_invoice_and_fetch_media(
        state, sender, supplier_id, supply_request_id, media_id,
    ).await?;

    let branch_id = branch_for_supply_request(state, supply_request_id.into_inner()).await?;
    let event = DomainEvent::InvoiceReceived { supplier_id, supply_request_id, invoice_id };

    let reply = "Got it ✓ Your invoice has been received.";
    send_to_sender(state, sender, reply).await;
    append_supply_request_event(state, event, BranchId::new(branch_id)).await?;
    Ok(())
}

async fn handle_supplier_confirmed(
    state: &AppState,
    sender: &ChannelIdentity,
    supplier_id: SupplierId,
    sender_key: &str,
    active_srs: &[ActiveOrderContext],
    resolved_sr_id: Option<uuid::Uuid>,
    history_repo: &ConversationHistoryRepository,
    disambig_store: &DisambiguationStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let supply_request_id = match resolve_supply_request(resolved_sr_id, active_srs) {
        SupplyRequestResolved::Single(id) => id,
        SupplyRequestResolved::NeedsDisambiguation(candidates) => {
            let ids: Vec<uuid::Uuid> = candidates.iter().map(|id| id.into_inner()).collect();
            disambig_store.create(sender_key, "supplier_confirmed", &ids, "supply_request").await?;
            let desc = supply_request_description(state, candidates[0].into_inner()).await?;
            let q = format!("Is this confirmation for **{desc}**? (Yes / No)");
            send_to_sender(state, sender, &q).await;
            return Ok(());
        }
        SupplyRequestResolved::None => {
            send_to_sender(state, sender, "No active supply request to confirm.").await;
            return Ok(());
        }
    };

    let invoice_id = approved_invoice_for_supply_request(state, supply_request_id).await?;
    let branch_id = branch_for_supply_request(state, supply_request_id.into_inner()).await?;
    let event = DomainEvent::SupplierConfirmed { supplier_id, supply_request_id, invoice_id };

    let reply = "Confirmed ✓ Thank you.";
    send_to_sender(state, sender, reply).await;
    append_supply_request_event(state, event, BranchId::new(branch_id)).await?;
    Ok(())
}

async fn handle_supplier_disambiguation_reply(
    state: &AppState,
    sender: &ChannelIdentity,
    supplier_id: SupplierId,
    sender_key: &str,
    reply_text: &str,
    pending: &store::disambiguation::DisambiguationRow,
    history_repo: &ConversationHistoryRepository,
    disambig_store: &DisambiguationStore,
) -> Result<(), Box<dyn std::error::Error>> {
    if is_negative(reply_text) {
        disambig_store.advance(sender_key).await?;
        if let Some(row) = disambig_store.find(sender_key).await? {
            if let Some(next_id) = row.current_candidate() {
                let desc = supply_request_description(state, next_id).await?;
                let q = format!("Is this for **{desc}**? (Yes / No)");
                send_to_sender(state, sender, &q).await;
                return Ok(());
            }
        }
        disambig_store.delete(sender_key).await?;
        send_to_sender(state, sender,
            "I couldn't match your message. The Owner has been alerted.").await;
        return Ok(());
    }
    if !is_affirmative(reply_text) {
        send_to_sender(state, sender, "Please reply Yes or No.").await;
        return Ok(());
    }

    let confirmed_id = match pending.current_candidate() {
        Some(id) => SupplyRequestId::new(id),
        None => { disambig_store.delete(sender_key).await?; return Ok(()); }
    };
    disambig_store.delete(sender_key).await?;
    let branch_id = branch_for_supply_request(state, confirmed_id.into_inner()).await?;

    let event = if pending.original_text == "supplier_confirmed" {
        let invoice_id = approved_invoice_for_supply_request(state, confirmed_id).await?;
        DomainEvent::SupplierConfirmed { supplier_id, supply_request_id: confirmed_id, invoice_id }
    } else {
        let invoice_id = create_invoice_and_fetch_media(
            state, sender, supplier_id, confirmed_id, None,
        ).await?;
        DomainEvent::InvoiceReceived { supplier_id, supply_request_id: confirmed_id, invoice_id }
    };

    let reply = "Got it ✓";
    send_to_sender(state, sender, reply).await;
    append_supply_request_event(state, event, BranchId::new(branch_id)).await?;
    Ok(())
}

// ── Resolution helpers ───────────────────────────────────────────────────── //

enum ResolvedOrder {
    Single(OrderId),
    NeedsDisambiguation(Vec<OrderId>),
    NoActiveOrders,
}

fn resolve_order_id(
    from_claude: Option<uuid::Uuid>,
    active_ids: &[OrderId],
) -> ResolvedOrder {
    match active_ids {
        [] => ResolvedOrder::NoActiveOrders,
        [only] => ResolvedOrder::Single(*only),
        many => {
            if let Some(id) = from_claude {
                let oid = OrderId::new(id);
                if many.contains(&oid) {
                    return ResolvedOrder::Single(oid);
                }
            }
            ResolvedOrder::NeedsDisambiguation(many.to_vec())
        }
    }
}

enum SupplyRequestResolved {
    Single(SupplyRequestId),
    NeedsDisambiguation(Vec<SupplyRequestId>),
    None,
}

fn resolve_supply_request(
    from_claude: Option<uuid::Uuid>,
    active: &[ActiveOrderContext],
) -> SupplyRequestResolved {
    match active {
        [] => SupplyRequestResolved::None,
        [only] => SupplyRequestResolved::Single(SupplyRequestId::new(only.order_id)),
        many => {
            if let Some(id) = from_claude {
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

fn is_affirmative(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "yes" | "y" | "ใช่" | "ใช่ครับ" | "ใช่ค่ะ" | "ok" | "โอเค" | "correct" | "right"
    )
}

fn is_negative(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "no" | "n" | "ไม่" | "ไม่ใช่" | "ไม่ครับ" | "ไม่ค่ะ" | "nope" | "wrong"
    )
}

// ── Send helpers ─────────────────────────────────────────────────────────── //

async fn send_to_sender(state: &AppState, sender: &ChannelIdentity, text: &str) {
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

async fn fetch_reply_template(state: &AppState, order_id: OrderId, event: &DomainEvent) -> String {
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

// ── DB helpers ───────────────────────────────────────────────────────────── //

/// F01: fetch both short_name and description in one query.
async fn order_display_fields(
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

async fn build_order_contexts(
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
            out.push(ActiveOrderContext { order_id: oid.into_inner(), description: desc, state: st });
        }
    }
    Ok(out)
}

async fn active_supply_request_contexts(
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

async fn supply_request_description(state: &AppState, id: uuid::Uuid) -> Result<String, sqlx::Error> {
    let (desc,): (String,) =
        sqlx::query_as("SELECT description FROM supply_requests WHERE id = $1")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    Ok(desc)
}

async fn branch_for_supply_request(state: &AppState, id: uuid::Uuid) -> Result<uuid::Uuid, sqlx::Error> {
    let (bid,): (uuid::Uuid,) =
        sqlx::query_as("SELECT branch_id FROM supply_requests WHERE id = $1")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    Ok(bid)
}

async fn approved_invoice_for_supply_request(
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

async fn create_invoice_and_fetch_media(
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

// ── Event append ─────────────────────────────────────────────────────────── //

async fn append_order_event(
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

async fn append_supply_request_event(
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