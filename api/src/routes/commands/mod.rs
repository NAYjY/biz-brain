//! Owner command endpoints — split across three submodules.
//! This mod.rs owns only the shared helpers and re-exports everything
//! so callers in app.rs continue to use `routes::commands::assign_worker` etc.

pub mod order_state;
pub mod supply;
pub mod worker;

pub use order_state::{
    assign_worker, cancel_order, close_order, delete_order, edit_description,
    force_accepted, force_clarification, force_ready, force_unavailable,
    reassign_worker, reset_order, set_short_name,
};
pub use supply::approve_invoice;
pub use worker::{message_worker, resolve_clarification};

// ── Shared helpers ────────────────────────────────────────────────────────── //

use axum::http::StatusCode;
use domain::{BranchId, Channel, ChannelIdentity, DomainEvent, OrderId};
use messaging::ChannelAdapter;
use store::conversation_history::ConversationHistoryRepository;

use crate::{event_handler, state::AppState};

pub(super) fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub(super) fn bad_request(msg: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, msg.into())
}

pub(super) async fn send_message_to_worker(
    state: &AppState,
    worker_id: uuid::Uuid,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT channel, external_id FROM actor_directory \
         WHERE actor_id = $1 AND actor_type = 'worker' AND owner_confirmed = TRUE",
    )
    .bind(worker_id)
    .fetch_optional(&state.pool)
    .await?;

    let (channel_str, external_id) =
        row.ok_or("worker has no confirmed channel binding")?;

    let channel = parse_channel(&channel_str)?;
    let identity = ChannelIdentity { channel, external_id: external_id.clone() };

    match identity.channel {
        Channel::Line     => state.line.send_push(&identity, text).await?,
        Channel::WhatsApp => state.whatsapp.send_push(&identity, text).await?,
        Channel::Telegram => state.telegram.send_push(&identity, text).await?,
    }

    // F04: persist Owner reply in conversation_history so it appears in the
    // thread modal alongside the worker's messages.
    let sender_key = ConversationHistoryRepository::sender_key(
        &channel_str,
        &external_id,
    );
    let history_repo = ConversationHistoryRepository::new(state.pool.clone());
    let _ = history_repo.append(&sender_key, "assistant", text).await;

    Ok(())
}

pub(super) async fn append_and_project_order(
    state: &AppState,
    branch_id: BranchId,
    order_id: OrderId,
    event: DomainEvent,
) -> Result<StatusCode, (StatusCode, String)> {
    let seq = state
        .order_events
        .current_sequence(order_id)
        .await
        .map_err(internal)?;

    state
        .event_sourcing
        .append(branch_id, seq + 1, &event)
        .await
        .map_err(internal)?;

    event_handler::fan_out(state, &event).await;

    let signal = state
        .projection_worker
        .project_order(order_id)
        .await
        .map_err(internal)?;
    state.publish_sse(signal).await;

    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn parse_channel(s: &str) -> Result<Channel, Box<dyn std::error::Error>> {
    match s {
        "line"      => Ok(Channel::Line),
        "whats_app" => Ok(Channel::WhatsApp),
        "telegram"  => Ok(Channel::Telegram),
        other       => Err(format!("unknown channel: {other}").into()),
    }
}