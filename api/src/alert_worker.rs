//! F05: Background alert worker.
//! Polls follow_up_alerts every 60 seconds.
//! For each due row, fetches the assigned worker's channel identity and
//! sends a push message. Then calls mark_fired().
//!
//! Uses display_name() from inbox_worker for consistent order naming.

use std::time::Duration;

use domain::{Channel, ChannelIdentity};
use messaging::ChannelAdapter;
use store::alerts::{AlertMode, AlertRepository};

use crate::{inbox_worker::display_name, state::AppState};

pub async fn run(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        if let Err(e) = fire_due_alerts(&state).await {
            tracing::error!("alert_worker error: {e}");
        }
    }
}

async fn fire_due_alerts(state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let repo = AlertRepository::new(state.pool.clone());
    let due = repo.fetch_due().await?;

    for alert in due {
        if let Err(e) = fire_alert(state, &repo, &alert).await {
            tracing::error!("failed to fire alert {}: {e}", alert.id);
        }
    }
    Ok(())
}

async fn fire_alert(
    state: &AppState,
    repo: &AlertRepository,
    alert: &store::alerts::AlertRow,
) -> Result<(), Box<dyn std::error::Error>> {
    // Fetch worker identity for this order.
    let identity = worker_identity_for_order(state, alert.order_id).await?;

    // Build message text.
    let text = if let Some(msg) = &alert.message {
        msg.clone()
    } else {
        let (short_name, desc) =
            order_display_fields(state, alert.order_id).await?;
        let name = display_name(short_name.as_deref(), &desc);
        let interval_label = match AlertMode::from_sql(&alert.alert_mode) {
            Some(AlertMode::Hourly)  => " [hourly reminder]",
            Some(AlertMode::Daily)   => " [daily reminder]",
            Some(AlertMode::Every3d) => " [3-day reminder]",
            _                        => "",
        };
        format!("🔔 Follow-up reminder{interval_label}: {name}")
    };

    send_to_identity(state, &identity, &text).await;

    let mode = AlertMode::from_sql(&alert.alert_mode)
        .ok_or_else(|| format!("unknown alert_mode: {}", alert.alert_mode))?;
    repo.mark_fired(alert.id, &mode).await?;

    tracing::info!(
        order_id = %alert.order_id,
        mode = %alert.alert_mode,
        "follow-up alert fired"
    );
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────── //

async fn worker_identity_for_order(
    state: &AppState,
    order_id: uuid::Uuid,
) -> Result<ChannelIdentity, Box<dyn std::error::Error>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT ad.channel, ad.external_id \
         FROM order_current_state ocs \
         JOIN actor_directory ad \
             ON ad.actor_id = ocs.worker_id \
             AND ad.actor_type = 'worker' \
             AND ad.owner_confirmed = TRUE \
         WHERE ocs.order_id = $1 AND ocs.worker_id IS NOT NULL",
    )
    .bind(order_id)
    .fetch_optional(&state.pool)
    .await?;

    let (ch, ext) = row.ok_or_else(|| {
        format!("no confirmed worker binding for order {order_id} — skipping alert")
    })?;

    let channel = match ch.as_str() {
        "line"      => Channel::Line,
        "whats_app" => Channel::WhatsApp,
        "telegram"  => Channel::Telegram,
        other       => return Err(format!("unknown channel {other}").into()),
    };
    Ok(ChannelIdentity { channel, external_id: ext })
}

async fn order_display_fields(
    state: &AppState,
    order_id: uuid::Uuid,
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
    .bind(order_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(row)
}

async fn send_to_identity(state: &AppState, identity: &ChannelIdentity, text: &str) {
    let result = match identity.channel {
        Channel::Line     => state.line.send_push(identity, text).await,
        Channel::WhatsApp => state.whatsapp.send_push(identity, text).await,
        Channel::Telegram => state.telegram.send_push(identity, text).await,
    };
    if let Err(e) = result {
        tracing::error!("alert push to {} failed: {e}", identity.external_id);
    }
}