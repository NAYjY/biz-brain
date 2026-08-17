//! P14: unified inbox_worker routing flow.
//!
//! Worker path (LINE / Telegram):
//!   1. Resolve actor from actor_directory (fail-closed)
//!   2. Load active orders + write conversation_history
//!   3. Check disambiguation_pending FIRST — if a yes/no flow is active,
//!      parse the reply and advance or resolve it
//!   4. Otherwise: P13 classify call (variant + order_id)
//!   5. If clear (variant + order_id in active set) → apply event
//!   6. If not clear → start P02 ranked yes/no flow
//!
//! Supplier path (WhatsApp):
//!   1. Resolve actor
//!   2. Check disambiguation_pending FIRST — yes/no flow takes priority
//!   3. Otherwise: P13 classify → InvoiceReceived or SupplierConfirmed
//!   4. P05: if message has media_id, fetch and store on invoice
//!
//! P07: terminal events (OrderDone, WorkerUnavailable, WorkerCancelled,
//! OwnerCancelled) remove the order from ThreadContextStore.

use std::time::Duration;

use agent::classify::ActiveOrderContext;
use agent::InterpretationError;
use domain::{BranchId, Channel, ChannelIdentity, DomainEvent, InvoiceId, OrderId, SupplierId, SupplyRequestId, WorkerId};
use messaging::ChannelAdapter;
use store::conversation_history::ConversationHistoryRepository;
use store::disambiguation::DisambiguationStore;
use store::reply_templates::ReplyTemplateRepository;

use crate::{event_handler, state::AppState};

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
        tracing::warn!(
            external_id = %sender.external_id,
            "unknown Worker sender — registering as pending"
        );
        state.actors.register_pending(sender, store::actor_directory::ActorType::Worker).await?;
        return Ok(());
    };

    let sender_key = ConversationHistoryRepository::sender_key(
        sender.channel.as_sql(),
        &sender.external_id,
    );
    let history_repo = ConversationHistoryRepository::new(state.pool.clone());
    let disambig_store = DisambiguationStore::new(state.pool.clone());

    // Load history and active orders.
    let history = history_repo.load(&sender_key).await?;
    let active_order_ids = state.actors.active_orders_for_worker(worker_id).await?;

    // Persist this incoming message immediately (P13).
    history_repo.append(&sender_key, "user", text).await?;

    // Attach active orders to ThreadContextStore.
    {
        let mut threads = state.threads.lock().await;
        for oid in &active_order_ids {
            threads.add_active_order(sender.clone(), *oid);
        }
    }

    // Save raw message to order projection so Owner can see it.
    if let Some(&first_order_id) = active_order_ids.first() {
        let _ = state.projections.update_worker_message(first_order_id, text).await;
        let signal = state.projection_worker.project_order(first_order_id).await?;
        state.publish_sse(signal).await;
    }

    // P14: check disambiguation_pending FIRST — active yes/no flow.
    if let Some(pending) = disambig_store.find(&sender_key).await? {
        return handle_worker_disambiguation_reply(
            state,
            sender,
            worker_id,
            &sender_key,
            text,
            &pending,
            &history_repo,
            &disambig_store,
        )
        .await;
    }

    // P13/P14: classify call — includes conversation history + order contexts.
    let active_contexts = build_order_contexts(state, &active_order_ids).await?;
    let classify_result = state
        .worker_agent
        .classify(text, &history, &active_contexts)
        .await;

    match classify_result {
        Err(InterpretationError::UnexpectedVariant { received, allowed }) => {
            tracing::error!(
                owner_alert = true,
                "unexpected variant '{received}' (allowed: {allowed}) for Worker message"
            );
            send_to_sender(state, sender, "Sorry, I didn't understand that. The Owner has been alerted.").await;
            return Ok(());
        }
        Err(e) => {
            tracing::error!(owner_alert = true, "classify error: {e}");
            send_to_sender(state, sender, "Sorry, something went wrong. The Owner has been alerted.").await;
            return Ok(());
        }
        Ok(None) => {
            send_to_sender(
                state,
                sender,
                "Sorry, I didn't understand that. The Owner can see your message and will respond.",
            )
            .await;
            return Ok(());
        }
        Ok(Some((variant, resolved_order_id))) => {
            let order_id = resolve_order_id(
                resolved_order_id,
                &active_order_ids,
                &active_contexts,
            );

            match order_id {
                ResolvedOrder::Single(order_id) => {
                    // Clear and happy path.
                    disambig_store.delete(&sender_key).await?;
                    let event = domain::construct_worker_event(variant, worker_id, order_id);

                    // P07: remove from thread context for terminal events.
                    if event.is_terminal_for_worker() {
                        let mut threads = state.threads.lock().await;
                        threads.remove_active_order(sender, order_id);
                    }

                    let reply = fetch_reply_template(state, order_id, &event).await;
                    send_to_sender(state, sender, &reply).await;
                    history_repo.append(&sender_key, "assistant", &reply).await?;
                    append_order_event(state, event, sender, order_id).await?;
                }
                ResolvedOrder::NeedsDisambiguation(candidates) => {
                    start_worker_disambiguation(
                        state,
                        sender,
                        &sender_key,
                        text,
                        candidates,
                        &active_contexts,
                        &disambig_store,
                        &history_repo,
                    )
                    .await?;
                }
                ResolvedOrder::NoActiveOrders => {
                    send_to_sender(
                        state,
                        sender,
                        "You have no active orders at the moment.",
                    )
                    .await;
                }
            }
        }
    }

    Ok(())
}

/// Parse a yes/no reply from a Worker during a P02 disambiguation flow.
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
    let is_yes = is_affirmative(reply_text);
    let is_no = is_negative(reply_text);

    if !is_yes && !is_no {
        // Not a yes/no — treat as a new message, let the main flow handle it.
        // Delete the pending row so classify-first logic runs.
        disambig_store.delete(sender_key).await?;
        // Re-classify the new text (fall through by returning Ok — next inbox
        // poll picks this up, but since we deleted the row it won't loop).
        send_to_sender(
            state,
            sender,
            "I'm not sure — please reply Yes or No. Which order is this about?",
        )
        .await;
        return Ok(());
    }

    if is_no {
        disambig_store.advance(sender_key).await?;
        // Reload with updated index.
        let updated = disambig_store.find(sender_key).await?;
        if let Some(row) = updated {
            if let Some(next_id) = row.current_candidate() {
                let question = format_disambiguation_question(state, next_id, &row).await;
                send_to_sender(state, sender, &question).await;
                return Ok(());
            }
        }
        // All candidates exhausted → ClarificationRequested.
        disambig_store.delete(sender_key).await?;
        let order_id = pending.candidates().first().copied().map(OrderId::new);
        if let Some(order_id) = order_id {
            let event = DomainEvent::ClarificationRequested { worker_id, order_id };
            send_to_sender(
                state,
                sender,
                "I've flagged this to the Owner. They'll be in touch shortly.",
            )
            .await;
            append_order_event(state, event, sender, order_id).await?;
        }
        return Ok(());
    }

    // is_yes — confirmed. Re-classify original_text on the confirmed order.
    let confirmed_id = match pending.current_candidate() {
        Some(id) => OrderId::new(id),
        None => {
            disambig_store.delete(sender_key).await?;
            return Ok(());
        }
    };
    disambig_store.delete(sender_key).await?;

    // Re-run classify on the original text now that the order is pinned.
    let history = history_repo.load(sender_key).await?;
    let ctx = vec![ActiveOrderContext {
        order_id: confirmed_id.into_inner(),
        description: order_description(state, confirmed_id.into_inner()).await?,
        state: "confirmed".to_string(),
    }];
    let variant = state
        .worker_agent
        .classify(&pending.original_text, &history, &ctx)
        .await
        .ok()
        .flatten()
        .map(|(v, _)| v)
        .unwrap_or(domain::DomainEventVariant::ClarificationRequested);

    let event = domain::construct_worker_event(variant, worker_id, confirmed_id);

    if event.is_terminal_for_worker() {
        let mut threads = state.threads.lock().await;
        threads.remove_active_order(sender, confirmed_id);
    }

    let reply = fetch_reply_template(state, confirmed_id, &event).await;
    send_to_sender(state, sender, &reply).await;
    history_repo.append(sender_key, "assistant", &reply).await?;
    append_order_event(state, event, sender, confirmed_id).await?;

    Ok(())
}

async fn start_worker_disambiguation(
    state: &AppState,
    sender: &ChannelIdentity,
    sender_key: &str,
    original_text: &str,
    candidates: Vec<OrderId>,
    contexts: &[ActiveOrderContext],
    disambig_store: &DisambiguationStore,
    history_repo: &ConversationHistoryRepository,
) -> Result<(), Box<dyn std::error::Error>> {
    // Rank candidates: let Claude pick the most likely one first by ordering
    // the contexts by closest description match (simple heuristic — Claude's
    // classify already picked a variant so just preserve active order ordering).
    let candidate_ids: Vec<uuid::Uuid> = candidates.iter().map(|id| id.into_inner()).collect();

    disambig_store
        .create(sender_key, original_text, &candidate_ids, "order")
        .await?;

    let first = candidates.first().copied().unwrap();
    let row = disambig_store.find(sender_key).await?.unwrap();
    let question = format_disambiguation_question(state, first.into_inner(), &row).await;
    send_to_sender(state, sender, &question).await;
    history_repo.append(sender_key, "assistant", &question).await?;

    Ok(())
}

async fn format_disambiguation_question(
    state: &AppState,
    candidate_id: uuid::Uuid,
    _row: &store::disambiguation::DisambiguationRow,
) -> String {
    let desc = order_description(state, candidate_id)
        .await
        .unwrap_or_else(|_| candidate_id.to_string());
    format!("Is this about **{desc}**? (Reply Yes or No)")
}

// ── Supplier path ────────────────────────────────────────────────────────── //

async fn process_supplier_message(
    state: &AppState,
    sender: &ChannelIdentity,
    text: &str,
    media_id: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(supplier_id) = state.actors.resolve_supplier(sender).await? else {
        tracing::warn!(
            external_id = %sender.external_id,
            "unknown Supplier sender — registering as pending"
        );
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

    // P14 Supplier path: check disambiguation_pending FIRST.
    if let Some(pending) = disambig_store.find(&sender_key).await? {
        return handle_supplier_disambiguation_reply(
            state,
            sender,
            supplier_id,
            &sender_key,
            text,
            &pending,
            &history_repo,
            &disambig_store,
        )
        .await;
    }

    // Classify — history + any active supply requests as context.
    let history = history_repo.load(&sender_key).await?;
    let active_supply_requests = active_supply_request_contexts(state, supplier_id).await?;

    let result = state
        .supplier_agent
        .classify(text, &history, &active_supply_requests)
        .await;

    match result {
        Err(InterpretationError::UnexpectedVariant { received, allowed }) => {
            tracing::error!(owner_alert = true, "unexpected supplier variant '{received}' (allowed: {allowed})");
            send_to_sender(state, sender, "Sorry, I didn't understand that.").await;
        }
        Err(e) => {
            tracing::error!(owner_alert = true, "supplier classify error: {e}");
        }
        Ok(None) => {
            send_to_sender(state, sender, "Sorry, I didn't understand that.").await;
        }
        Ok(Some((domain::DomainEventVariant::InvoiceReceived, resolved_sr_id))) => {
            handle_invoice_received(
                state,
                sender,
                supplier_id,
                &sender_key,
                &active_supply_requests,
                resolved_sr_id,
                media_id,
                &history_repo,
                &disambig_store,
            )
            .await?;
        }
        Ok(Some((domain::DomainEventVariant::SupplierConfirmed, resolved_sr_id))) => {
            handle_supplier_confirmed(
                state,
                sender,
                supplier_id,
                &sender_key,
                &active_supply_requests,
                resolved_sr_id,
                &history_repo,
                &disambig_store,
            )
            .await?;
        }
        Ok(Some((other, _))) => {
            tracing::warn!("unexpected supplier variant in happy path: {other:?}");
        }
    }

    Ok(())
}

async fn handle_invoice_received(
    state: &AppState,
    sender: &ChannelIdentity,
    supplier_id: SupplierId,
    sender_key: &str,
    active_supply_requests: &[ActiveOrderContext],
    resolved_sr_id: Option<uuid::Uuid>,
    media_id: Option<&str>,
    history_repo: &ConversationHistoryRepository,
    disambig_store: &DisambiguationStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let supply_request_id = match resolve_supply_request(resolved_sr_id, active_supply_requests) {
        SupplyRequestResolved::Single(id) => id,
        SupplyRequestResolved::NeedsDisambiguation(candidates) => {
            let candidate_ids: Vec<uuid::Uuid> =
                candidates.iter().map(|id| id.into_inner()).collect();
            disambig_store
                .create(sender_key, "invoice", &candidate_ids, "supply_request")
                .await?;
            let first = candidates.first().copied().unwrap();
            let desc = supply_request_description(state, first.into_inner()).await?;
            let q = format!("Is this invoice for **{desc}**? (Reply Yes or No)");
            send_to_sender(state, sender, &q).await;
            history_repo.append(sender_key, "assistant", &q).await?;
            return Ok(());
        }
        SupplyRequestResolved::None => {
            send_to_sender(state, sender, "No active supply requests found.").await;
            return Ok(());
        }
    };

    // Create the invoice row and collect media (P05).
    let invoice_id = create_invoice_and_fetch_media(
        state,
        sender,
        supplier_id,
        supply_request_id,
        media_id,
    )
    .await?;

    let branch_id = branch_for_supply_request(state, supply_request_id.into_inner()).await?;

    let event = DomainEvent::InvoiceReceived {
        supplier_id,
        supply_request_id,
        invoice_id,
    };

    let reply = "Got it ✓ Your invoice has been received.";
    send_to_sender(state, sender, reply).await;
    history_repo.append(sender_key, "assistant", reply).await?;
    append_supply_request_event(state, event, BranchId::new(branch_id)).await?;

    Ok(())
}

async fn handle_supplier_confirmed(
    state: &AppState,
    sender: &ChannelIdentity,
    supplier_id: SupplierId,
    sender_key: &str,
    active_supply_requests: &[ActiveOrderContext],
    resolved_sr_id: Option<uuid::Uuid>,
    history_repo: &ConversationHistoryRepository,
    disambig_store: &DisambiguationStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let supply_request_id = match resolve_supply_request(resolved_sr_id, active_supply_requests) {
        SupplyRequestResolved::Single(id) => id,
        SupplyRequestResolved::NeedsDisambiguation(candidates) => {
            let candidate_ids: Vec<uuid::Uuid> =
                candidates.iter().map(|id| id.into_inner()).collect();
            disambig_store
                .create(sender_key, "supplier_confirmed", &candidate_ids, "supply_request")
                .await?;
            let first = candidates.first().copied().unwrap();
            let desc = supply_request_description(state, first.into_inner()).await?;
            let q = format!("Is this confirmation for **{desc}**? (Reply Yes or No)");
            send_to_sender(state, sender, &q).await;
            return Ok(());
        }
        SupplyRequestResolved::None => {
            send_to_sender(state, sender, "No active supply request found to confirm.").await;
            return Ok(());
        }
    };

    // Need invoice_id from the OwnerApprovedInvoice state.
    let invoice_id = approved_invoice_for_supply_request(state, supply_request_id).await?;
    let branch_id = branch_for_supply_request(state, supply_request_id.into_inner()).await?;

    let event = DomainEvent::SupplierConfirmed {
        supplier_id,
        supply_request_id,
        invoice_id,
    };

    let reply = "Confirmed ✓ Thank you for confirming.";
    send_to_sender(state, sender, reply).await;
    history_repo.append(sender_key, "assistant", reply).await?;
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
                let q = format!("Is this for **{desc}**? (Reply Yes or No)");
                send_to_sender(state, sender, &q).await;
                return Ok(());
            }
        }
        // Exhausted.
        disambig_store.delete(sender_key).await?;
        send_to_sender(state, sender, "I couldn't match your message to a supply request. The Owner has been alerted.").await;
        return Ok(());
    }

    if !is_affirmative(reply_text) {
        send_to_sender(state, sender, "Please reply Yes or No.").await;
        return Ok(());
    }

    let confirmed_id = match pending.current_candidate() {
        Some(id) => SupplyRequestId::new(id),
        None => {
            disambig_store.delete(sender_key).await?;
            return Ok(());
        }
    };
    disambig_store.delete(sender_key).await?;

    let branch_id = branch_for_supply_request(state, confirmed_id.into_inner()).await?;

    let event = if pending.original_text == "supplier_confirmed" {
        let invoice_id = approved_invoice_for_supply_request(state, confirmed_id).await?;
        DomainEvent::SupplierConfirmed {
            supplier_id,
            supply_request_id: confirmed_id,
            invoice_id,
        }
    } else {
        let invoice_id = create_invoice_and_fetch_media(state, sender, supplier_id, confirmed_id, None)
            .await?;
        DomainEvent::InvoiceReceived {
            supplier_id,
            supply_request_id: confirmed_id,
            invoice_id,
        }
    };

    let reply = "Got it ✓";
    send_to_sender(state, sender, reply).await;
    history_repo.append(sender_key, "assistant", reply).await?;
    append_supply_request_event(state, event, BranchId::new(branch_id)).await?;

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────── //

enum ResolvedOrder {
    Single(OrderId),
    NeedsDisambiguation(Vec<OrderId>),
    NoActiveOrders,
}

fn resolve_order_id(
    from_claude: Option<uuid::Uuid>,
    active_ids: &[OrderId],
    _contexts: &[ActiveOrderContext],
) -> ResolvedOrder {
    if active_ids.is_empty() {
        return ResolvedOrder::NoActiveOrders;
    }

    if active_ids.len() == 1 {
        return ResolvedOrder::Single(active_ids[0]);
    }

    // Multi-order: Claude may have named one.
    if let Some(claude_id) = from_claude {
        let order_id = OrderId::new(claude_id);
        if active_ids.contains(&order_id) {
            return ResolvedOrder::Single(order_id);
        }
    }

    ResolvedOrder::NeedsDisambiguation(active_ids.to_vec())
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
    if active.is_empty() {
        return SupplyRequestResolved::None;
    }

    if active.len() == 1 {
        return SupplyRequestResolved::Single(SupplyRequestId::new(active[0].order_id));
    }

    if let Some(id) = from_claude {
        if active.iter().any(|ctx| ctx.order_id == id) {
            return SupplyRequestResolved::Single(SupplyRequestId::new(id));
        }
    }

    SupplyRequestResolved::NeedsDisambiguation(
        active.iter().map(|ctx| SupplyRequestId::new(ctx.order_id)).collect(),
    )
}

fn is_affirmative(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    matches!(
        t.as_str(),
        "yes" | "y" | "ใช่" | "ใช่ครับ" | "ใช่ค่ะ" | "ok" | "โอเค" | "correct" | "right"
    )
}

fn is_negative(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    matches!(
        t.as_str(),
        "no" | "n" | "ไม่" | "ไม่ใช่" | "ไม่ครับ" | "ไม่ค่ะ" | "nope" | "wrong"
    )
}

async fn send_to_sender(state: &AppState, sender: &ChannelIdentity, text: &str) {
    let result = match sender.channel {
        Channel::Line      => state.line.send_push(sender, text).await,
        Channel::WhatsApp  => state.whatsapp.send_push(sender, text).await,
        Channel::Telegram  => state.telegram.send_push(sender, text).await,
    };
    if let Err(e) = result {
        tracing::error!("push to {:?} failed: {e}", sender.external_id);
    }
}

async fn fetch_reply_template(state: &AppState, order_id: OrderId, event: &DomainEvent) -> String {
    let event_type = event.variant().as_sql();
    // Look up branch_id from the order.
    let branch_id: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT branch_id FROM orders WHERE id = $1",
    )
    .bind(order_id.into_inner())
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();

    if let Some((bid,)) = branch_id {
        let repo = ReplyTemplateRepository::new(state.pool.clone());
        if let Ok(tmpl) = repo.fetch(bid, event_type).await {
            return tmpl;
        }
    }

    // Hard fallback — should never be reached if templates are seeded.
    format!("Done ✓ ({})", event_type)
}

async fn build_order_contexts(
    state: &AppState,
    order_ids: &[OrderId],
) -> Result<Vec<ActiveOrderContext>, Box<dyn std::error::Error>> {
    let mut contexts = Vec::with_capacity(order_ids.len());
    for &order_id in order_ids {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT ocs.state, o.description \
             FROM order_current_state ocs \
             JOIN orders o ON o.id = ocs.order_id \
             WHERE ocs.order_id = $1",
        )
        .bind(order_id.into_inner())
        .fetch_optional(&state.pool)
        .await?;

        if let Some((state_str, description)) = row {
            contexts.push(ActiveOrderContext {
                order_id: order_id.into_inner(),
                description,
                state: state_str,
            });
        }
    }
    Ok(contexts)
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

    Ok(rows
        .into_iter()
        .map(|(id, state, desc)| ActiveOrderContext {
            order_id: id,
            description: desc,
            state,
        })
        .collect())
}

async fn order_description(
    state: &AppState,
    order_id: uuid::Uuid,
) -> Result<String, sqlx::Error> {
    let row: (String,) =
        sqlx::query_as("SELECT description FROM orders WHERE id = $1")
            .bind(order_id)
            .fetch_one(&state.pool)
            .await?;
    Ok(row.0)
}

async fn supply_request_description(
    state: &AppState,
    sr_id: uuid::Uuid,
) -> Result<String, sqlx::Error> {
    let row: (String,) =
        sqlx::query_as("SELECT description FROM supply_requests WHERE id = $1")
            .bind(sr_id)
            .fetch_one(&state.pool)
            .await?;
    Ok(row.0)
}

async fn branch_for_supply_request(
    state: &AppState,
    sr_id: uuid::Uuid,
) -> Result<uuid::Uuid, sqlx::Error> {
    let row: (uuid::Uuid,) =
        sqlx::query_as("SELECT branch_id FROM supply_requests WHERE id = $1")
            .bind(sr_id)
            .fetch_one(&state.pool)
            .await?;
    Ok(row.0)
}

async fn approved_invoice_for_supply_request(
    state: &AppState,
    supply_request_id: SupplyRequestId,
) -> Result<InvoiceId, Box<dyn std::error::Error>> {
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT invoice_id FROM invoice_current_state \
         WHERE supply_request_id = $1 AND state = 'OwnerApproved'",
    )
    .bind(supply_request_id.into_inner())
    .fetch_optional(&state.pool)
    .await?;

    row.map(|(id,)| InvoiceId::new(id))
        .ok_or_else(|| "no OwnerApproved invoice for supply request".into())
}

/// P05: insert the invoice row and optionally fetch + store media bytes.
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
        // P05: only WhatsApp has media — fetch and store.
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
                Err(e) => {
                    tracing::warn!("media fetch failed for invoice {invoice_id}: {e}");
                }
            }
        }
    }

    Ok(invoice_id)
}

// ── Event append helpers ─────────────────────────────────────────────────── //

async fn append_order_event(
    state: &AppState,
    event: DomainEvent,
    sender: &ChannelIdentity,
    order_id: OrderId,
) -> Result<(), Box<dyn std::error::Error>> {
    let seq = state.order_events.current_sequence(order_id).await?;
    let branch_id: (uuid::Uuid,) =
        sqlx::query_as("SELECT branch_id FROM orders WHERE id = $1")
            .bind(order_id.into_inner())
            .fetch_one(&state.pool)
            .await?;

    state
        .event_sourcing
        .append(BranchId::new(branch_id.0), seq + 1, &event)
        .await?;

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
    let supply_request_id = event
        .supply_request_id()
        .expect("Supplier-agent events are always SupplyRequest-aggregate");
    let seq = state
        .supply_request_events
        .current_sequence(supply_request_id)
        .await?;

    state.event_sourcing.append(branch_id, seq + 1, &event).await?;
    event_handler::fan_out(state, &event).await;
    let signal = state.projection_worker.project_supply_request(supply_request_id).await?;
    state.publish_sse(signal).await;
    Ok(())
}
