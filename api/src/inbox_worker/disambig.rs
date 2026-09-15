//! Worker disambiguation helpers.
//! Handles the multi-turn yes/no and order-name matching flow
//! that runs when a worker's message could apply to multiple active orders.

use agent::classify::ActiveOrderContext;
use agent::WorkerAgent;
use domain::{DomainEvent, OrderId, WorkerId};
use store::conversation_history::ConversationHistoryRepository;
use store::disambiguation::DisambiguationStore;

use crate::state::AppState;

use super::helpers::{
    append_order_event, fetch_reply_template, is_affirmative,
    is_negative, order_display_fields, send_to_sender, to_history_msg,
};
use super::display_name;

// ── Worker disambiguation entry ───────────────────────────────────────────── //

pub(super) async fn handle_worker_disambiguation_reply(
    state: &AppState,
    sender: &domain::ChannelIdentity,
    worker_id: WorkerId,
    sender_key: &str,
    reply_text: &str,
    pending: &store::disambiguation::DisambiguationRow,
    history_repo: &ConversationHistoryRepository,
    disambig_store: &DisambiguationStore,
    worker_agent: &WorkerAgent,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check if reply contains an order name — resolve directly without yes/no.
    let candidates = pending.candidates();
    for &candidate_id in &candidates {
        let (short_name, desc) = order_display_fields(state, candidate_id).await?;
        let name = display_name(short_name.as_deref(), &desc);
        if reply_text.to_lowercase().contains(&name.to_lowercase())
            || reply_text.to_lowercase().contains(&desc.to_lowercase())
        {
            disambig_store.delete(sender_key).await?;
            return resolve_and_apply_worker_event(
                state, sender, worker_id, sender_key,
                pending, OrderId::new(candidate_id),
                history_repo, worker_agent,
            ).await;
        }
    }

    // Not an order name — check yes/no.
    if !is_affirmative(reply_text) && !is_negative(reply_text) {
        send_to_sender(state, sender,
            "Please reply Yes or No, or type the order name.").await;
        return Ok(());
    }

    if is_negative(reply_text) {
        // Claude path: advance sequential index (old behaviour preserved).
        // For new Gemini branches this code path is never reached.
        disambig_store.delete(sender_key).await?;
        if let Some(&oid) = candidates.first() {
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

    // Affirmative — use narrowed_to or first candidate.
    let confirmed_id = match pending.narrowed_to {
        Some(id) => OrderId::new(id),
        None => match candidates.first() {
            Some(&id) => OrderId::new(id),
            None => { disambig_store.delete(sender_key).await?; return Ok(()); }
        },
    };
    disambig_store.delete(sender_key).await?;
    resolve_and_apply_worker_event(
        state, sender, worker_id, sender_key,
        pending, confirmed_id,
        history_repo, worker_agent,
    ).await
}

pub(super) async fn resolve_and_apply_worker_event(
    state: &AppState,
    sender: &domain::ChannelIdentity,
    worker_id: WorkerId,
    sender_key: &str,
    pending: &store::disambiguation::DisambiguationRow,
    confirmed_id: OrderId,
    history_repo: &ConversationHistoryRepository,
    worker_agent: &WorkerAgent,
) -> Result<(), Box<dyn std::error::Error>> {
    let history_rows = history_repo.load(sender_key).await?;
    let history: Vec<agent::HistoryMessage> =
        history_rows.into_iter().map(to_history_msg).collect();

    let real_state: Option<(String,)> = sqlx::query_as(
        "SELECT state FROM order_current_state WHERE order_id = $1",
    )
    .bind(confirmed_id.into_inner())
    .fetch_optional(&state.pool)
    .await?;

    let state_str = real_state.map(|(s,)| s).unwrap_or_else(|| "worker_assigned".to_string());
    let (_, desc) = order_display_fields(state, confirmed_id.into_inner()).await?;

    let ctx = vec![ActiveOrderContext {
        order_id: confirmed_id.into_inner(),
        description: desc,
        state: state_str,
    }];

    let variant = worker_agent
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
    let _ = state.projections.increment_unread(confirmed_id).await;

    let reply = fetch_reply_template(state, confirmed_id, &event).await;
    send_to_sender(state, sender, &reply).await;
    append_order_event(state, event, confirmed_id).await?;

    Ok(())
}

pub(super) async fn start_worker_disambiguation(
    state: &AppState,
    sender: &domain::ChannelIdentity,
    sender_key: &str,
    original_text: &str,
    candidates: Vec<OrderId>,
    _contexts: &[ActiveOrderContext],
    disambig_store: &DisambiguationStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let candidate_ids: Vec<uuid::Uuid> = candidates.iter().map(|id| id.into_inner()).collect();

    disambig_store.create(sender_key, original_text, &candidate_ids, "order", None).await?;

    // Build numbered question using all candidates at once (not sequential yes/no)
    let mut lines = vec!["งานที่หมายถึงคืองานไหนครับ?".to_string()];
    for (i, &cid) in candidate_ids.iter().enumerate() {
        let (short_name, desc) = order_display_fields(state, cid).await
            .unwrap_or((None, cid.to_string()));
        let name = display_name(short_name.as_deref(), &desc);
        lines.push(format!("{}. {}", i + 1, name));
    }
    send_to_sender(state, sender, &lines.join("\n")).await;
    Ok(())
}