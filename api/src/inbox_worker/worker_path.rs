//! Worker inbound message path.
//! Handles LINE / Telegram messages from Workers.
//! Branches on ai_provider: Gemini → harness, Claude → classify loop.

use agent::{InterpretationError, WorkerAgent};
use domain::{Channel, ChannelIdentity, DomainEvent, WorkerId};
use store::conversation_history::ConversationHistoryRepository;
use store::disambiguation::DisambiguationStore;

use crate::{inbox_worker_harness, state::AppState};

use super::disambig::{handle_worker_disambiguation_reply, start_worker_disambiguation};
use super::helpers::{
    append_order_event, build_order_contexts, fetch_reply_template, resolve_order_id,
    send_to_sender, to_history_msg, ResolvedOrder,
};

// ── Entry point ───────────────────────────────────────────────────────────── //

pub(super) async fn process_worker_message(
    state: &AppState,
    sender: &ChannelIdentity,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Resolve actor — fail-closed.
    let Some(worker_id) = state.actors.resolve_worker(sender).await? else {
        tracing::warn!(external_id = %sender.external_id, "unknown Worker — registering pending");
        state.actors.register_pending(sender, store::actor_directory::ActorType::Worker).await?;
        return Ok(());
    };

    // 2. Fetch branch_id.
    let branch_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT branch_id FROM workers WHERE id = $1",
    )
    .bind(worker_id.into_inner())
    .fetch_one(&state.pool)
    .await?;

    // 3. Branch on ai_provider.
    //    Gemini → full harness (rich disambiguation, AI-drafted replies, escalation).
    //    Claude → original classify path (unchanged below).
    let provider = state.branch_config.ai_provider(branch_id).await;

    if provider == "gemini" {
        return inbox_worker_harness::process_with_harness(
            state, sender, worker_id, branch_id, text,
        )
        .await;
    }

    // ── Claude classify path ──────────────────────────────────────────────── //

    let classifier = state.classifier_for_branch(branch_id).await;
    let worker_agent = WorkerAgent::new(classifier);

    let sender_key = ConversationHistoryRepository::sender_key(
        sender.channel.as_sql(),
        &sender.external_id,
    );
    let history_repo = ConversationHistoryRepository::new(state.pool.clone());
    let disambig_store = DisambiguationStore::new(state.pool.clone());

    let history_rows = history_repo.load(&sender_key).await?;
    let history: Vec<agent::HistoryMessage> = history_rows.into_iter().map(to_history_msg).collect();

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
            &pending, &history_repo, &disambig_store, &worker_agent,
        )
        .await;
    }

    let active_contexts = build_order_contexts(state, &active_order_ids).await?;

    // Track whether prefilter hit (vs. AI classifier used).
    let prefilter_hit = worker_agent.prefilter_hit(text);
    let classify_result = worker_agent.classify(text, &history, &active_contexts).await;

    match classify_result {
        Err(InterpretationError::UnexpectedVariant { received, allowed }) => {
            tracing::error!(owner_alert = true,
                "unexpected variant '{received}' (allowed: {allowed}) for Worker");
            send_to_sender(state, sender,
                "Sorry, I didn't understand that. The Owner has been alerted.").await;
        }
        Err(e) => {
            tracing::error!(owner_alert = true, "Worker classify error: {e}");
            if let Some(oid) = active_order_ids.first().copied() {
                let event = DomainEvent::ClarificationRequested { worker_id, order_id: oid };
                send_to_sender(state, sender,
                    "I couldn't process your message. The Owner has been alerted and will follow up.").await;
                append_order_event(state, event, oid).await?;
            } else {
                send_to_sender(state, sender,
                    "Something went wrong. The Owner has been alerted.").await;
            }
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

                    let _ = state.projections.update_worker_message(oid, text).await;
                    let _ = state.projections.increment_unread(oid).await;

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
                        candidates, &active_contexts, &disambig_store,
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