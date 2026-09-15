//! Inbox worker — drains webhook_inbox rows and routes each message to the
//! correct path (worker or supplier). All logic lives in submodules.
//!
//! P14: unified routing flow.
//! T01: per-branch AI provider selection via state.classifier_for_branch().
//! F01: display_name() is pub — called from event_handler and inbox_worker_harness.

use std::time::Duration;

use domain::Channel;

use crate::state::AppState;

mod disambig;
mod helpers;
mod supplier_path;
mod worker_path;

// ── Public API ────────────────────────────────────────────────────────────── //

/// Short display name for an order: short_name if set, else first 30 chars of description.
/// `pub` because event_handler.rs and inbox_worker_harness.rs both call this.
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

/// Main loop: polls webhook_inbox every 2 seconds and processes unread rows.
pub async fn run(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    loop {
        interval.tick().await;
        if let Err(e) = process_batch(&state).await {
            tracing::error!("inbox processing batch failed: {e}");
        }
    }
}

// ── Private routing ───────────────────────────────────────────────────────── //

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
    let sender: domain::ChannelIdentity =
        serde_json::from_value(row.raw_payload["sender"].clone())?;
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
            worker_path::process_worker_message(state, &sender, text).await
        }
        Channel::WhatsApp => {
            supplier_path::process_supplier_message(state, &sender, text, media_id.as_deref()).await
        }
    }
}