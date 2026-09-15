//! Supplier inbound message path.
//! Handles WhatsApp messages from Suppliers: invoice receipt, confirmation,
//! and disambiguation when multiple supply requests are active.

use agent::{InterpretationError, SupplierAgent};
use domain::{BranchId, Channel, ChannelIdentity, DomainEvent, SupplierId, SupplyRequestId};
use store::conversation_history::ConversationHistoryRepository;
use store::disambiguation::DisambiguationStore;

use crate::state::AppState;

use super::helpers::{
    active_supply_request_contexts, append_supply_request_event,
    approved_invoice_for_supply_request, branch_for_supply_request,
    create_invoice_and_fetch_media, is_affirmative, is_negative,
    resolve_supply_request, send_to_sender, supply_request_description,
    to_history_msg, SupplyRequestResolved,
};

// ── Entry point ───────────────────────────────────────────────────────────── //

pub(super) async fn process_supplier_message(
    state: &AppState,
    sender: &ChannelIdentity,
    text: &str,
    media_id: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Resolve actor — fail-closed.
    let Some(supplier_id) = state.actors.resolve_supplier(sender).await? else {
        tracing::warn!(external_id = %sender.external_id, "unknown Supplier — registering pending");
        state.actors.register_pending(sender, store::actor_directory::ActorType::Supplier).await?;
        return Ok(());
    };

    // T01: fetch branch_id so we can select the right AI classifier.
    let branch_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT branch_id FROM suppliers WHERE id = $1",
    )
    .bind(supplier_id.into_inner())
    .fetch_one(&state.pool)
    .await?;

    // T01: build the agent for this branch's configured provider.
    let classifier = state.classifier_for_branch(branch_id).await;
    let supplier_agent = SupplierAgent::new(classifier);

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
    let history: Vec<agent::HistoryMessage> = history_rows.into_iter().map(to_history_msg).collect();

    let active_srs = active_supply_request_contexts(state, supplier_id).await?;

    match supplier_agent.classify(text, &history, &active_srs).await {
        Err(InterpretationError::UnexpectedVariant { received, allowed }) => {
            tracing::error!(owner_alert = true,
                "unexpected supplier variant '{received}' (allowed: {allowed})");
            send_to_sender(state, sender, "Sorry, I didn't understand that.").await;
        }
        Err(e) => {
            tracing::error!(owner_alert = true, "supplier classify error: {e}");
            send_to_sender(state, sender,
                "Something went wrong processing your message. The Owner has been alerted.").await;
        }
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

// ── Invoice received ──────────────────────────────────────────────────────── //

async fn handle_invoice_received(
    state: &AppState,
    sender: &ChannelIdentity,
    supplier_id: SupplierId,
    sender_key: &str,
    active_srs: &[agent::classify::ActiveOrderContext],
    resolved_sr_id: Option<uuid::Uuid>,
    media_id: Option<&str>,
    history_repo: &ConversationHistoryRepository,
    disambig_store: &DisambiguationStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let supply_request_id = match resolve_supply_request(resolved_sr_id, active_srs) {
        SupplyRequestResolved::Single(id) => id,
        SupplyRequestResolved::NeedsDisambiguation(candidates) => {
            let ids: Vec<uuid::Uuid> = candidates.iter().map(|id| id.into_inner()).collect();
            disambig_store.create(sender_key, "invoice", &ids, "supply_request", None).await?;
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

    send_to_sender(state, sender, "Got it ✓ Your invoice has been received.").await;
    append_supply_request_event(state, event, BranchId::new(branch_id)).await?;
    Ok(())
}

// ── Supplier confirmed ────────────────────────────────────────────────────── //

async fn handle_supplier_confirmed(
    state: &AppState,
    sender: &ChannelIdentity,
    supplier_id: SupplierId,
    sender_key: &str,
    active_srs: &[agent::classify::ActiveOrderContext],
    resolved_sr_id: Option<uuid::Uuid>,
    history_repo: &ConversationHistoryRepository,
    disambig_store: &DisambiguationStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let supply_request_id = match resolve_supply_request(resolved_sr_id, active_srs) {
        SupplyRequestResolved::Single(id) => id,
        SupplyRequestResolved::NeedsDisambiguation(candidates) => {
            let ids: Vec<uuid::Uuid> = candidates.iter().map(|id| id.into_inner()).collect();
            disambig_store.create(sender_key, "supplier_confirmed", &ids, "supply_request", None).await?;
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

    send_to_sender(state, sender, "Confirmed ✓ Thank you.").await;
    append_supply_request_event(state, event, BranchId::new(branch_id)).await?;
    Ok(())
}

// ── Supplier disambiguation reply ─────────────────────────────────────────── //

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
        disambig_store.delete(sender_key).await?;
        send_to_sender(state, sender,
            "I couldn't match your message. The Owner has been alerted.").await;
        return Ok(());
    }
    if !is_affirmative(reply_text) {
        send_to_sender(state, sender, "Please reply Yes or No.").await;
        return Ok(());
    }

    // Affirmative — use narrowed_to or first candidate.
    let candidates = pending.candidates();
    let confirmed_id = match pending.narrowed_to {
        Some(id) => SupplyRequestId::new(id),
        None => match candidates.first() {
            Some(&id) => SupplyRequestId::new(id),
            None => { disambig_store.delete(sender_key).await?; return Ok(()); }
        },
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

    send_to_sender(state, sender, "Got it ✓").await;
    append_supply_request_event(state, event, BranchId::new(branch_id)).await?;
    Ok(())
}