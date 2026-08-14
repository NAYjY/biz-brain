//! T04's async processing step: drains `webhook_inbox`, resolves the sender
//! to a Worker/Supplier, hands the message to `agent` (T03), and appends any
//! resulting `DomainEvent`. This is the only place `agent` is invoked from.
//!
//! After appending each event:
//!   - Saves raw message text to order_current_state (Owner can see it)
//!   - Calls event_handler::fan_out (push notifications)
//!   - Sends Agent reply back to Worker/Supplier confirming the action

use std::time::Duration;

use agent::InterpretationOutcome;
use domain::{BranchId, Channel, ChannelIdentity, DomainEvent};
use messaging::ChannelAdapter;

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
    let channel = match row.channel.as_str() {
        "line"      => Channel::Line,
        "whats_app" => Channel::WhatsApp,
        "telegram"  => Channel::Telegram,
        _           => return Ok(()),
    };
    match channel {
        Channel::Line | Channel::Telegram => process_worker_message(state, &sender, text).await,
        Channel::WhatsApp                 => process_supplier_message(state, &sender, text).await,
    }
}

async fn process_worker_message(
    state: &AppState,
    sender: &ChannelIdentity,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match state.actors.resolve_worker(sender).await? {
        None => {
            tracing::error!(
                "OWNER ALERT (urgent): unrecognized sender {:?}. \
                 Registering as pending — confirm in dashboard.",
                sender.external_id
            );
            // Register so Owner sees it in Pending Bindings page.
            let _ = state.actors.register_pending(
                sender,
                store::actor_directory::ActorType::Worker,
            ).await;
            Ok(())
        }
        Some(worker_id) => {
            let outcome = {
                let mut threads = state.threads.lock().await;
                for order_id in state.actors.active_orders_for_worker(worker_id).await? {
                    threads.add_active_order(sender.clone(), order_id);
                }
                state.worker_agent.interpret(text, worker_id, sender, &threads).await?
            };

            match outcome {
                InterpretationOutcome::Event(event) => {
                    // Reply to Worker confirming their action
                    let reply = agent_reply_for_event(&event);
                    let _ = send_to_sender(state, sender, &reply).await;

                    append_order_event(state, event, text).await
                }
                InterpretationOutcome::NeedsOrderDisambiguation { question, .. } => {
                    // Ask Worker which order they mean
                    let _ = send_to_sender(state, sender, &question).await;
                    Ok(())
                }
                InterpretationOutcome::Unprocessed { reason } => {
                    tracing::warn!("OWNER ALERT (urgent): Worker message unprocessed — {reason}");
                    let _ = send_to_sender(
                        state,
                        sender,
                        "Sorry, I didn't understand that. Please try again or wait for the Owner to respond.",
                    ).await;
                    Ok(())
                }
            }
        }
    }
}

async fn process_supplier_message(
    state: &AppState,
    sender: &ChannelIdentity,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match state.actors.resolve_supplier(sender).await? {
        None => {
            tracing::error!(
                "OWNER ALERT (urgent): unrecognized supplier sender {:?}. \
                 Registering as pending — confirm in dashboard.",
                sender.external_id
            );
            let _ = state.actors.register_pending(
                sender,
                store::actor_directory::ActorType::Supplier,
            ).await;
            Ok(())
        }
        Some(_supplier_id) => {
            let outcome = state.supplier_agent.interpret(text).await?;
            match outcome {
                InterpretationOutcome::Event(event) => {
                    let _ = send_to_sender(
                        state,
                        sender,
                        "Got it ✓ Your message has been received.",
                    ).await;
                    append_supply_request_event(state, event).await
                }
                InterpretationOutcome::Unprocessed { reason } => {
                    tracing::warn!("OWNER ALERT (urgent): Supplier message unprocessed — {reason}");
                    let _ = send_to_sender(
                        state,
                        sender,
                        "Sorry, I didn't understand that. Please send your invoice details again.",
                    ).await;
                    Ok(())
                }
                InterpretationOutcome::NeedsOrderDisambiguation { .. } => {
                    unreachable!("SupplierAgent never disambiguates by Order")
                }
            }
        }
    }
}

// ── Agent reply text ──────────────────────────────────────────────────────── //

fn agent_reply_for_event(event: &DomainEvent) -> String {
    match event {
        DomainEvent::WorkerAccepted { .. } =>
            "Got it ✓ Order accepted.".to_string(),
        DomainEvent::WorkerUnavailable { .. } =>
            "Understood, you've been removed from this order.".to_string(),
        DomainEvent::WorkerCancelled { .. } =>
            "Noted, cancellation recorded. The Owner has been alerted.".to_string(),
        DomainEvent::ClarificationRequested { .. } =>
            "I've flagged your question to the Owner. They'll get back to you shortly.".to_string(),
        DomainEvent::WorkerReadyForPickup { .. } =>
            "Great, marked as ready for pickup ✓".to_string(),
        DomainEvent::OrderDone { .. } =>
            "Order closed. Well done!".to_string(),
        _ => "Done ✓".to_string(),
    }
}

// ── Send helpers ──────────────────────────────────────────────────────────── //

async fn send_to_sender(
    state: &AppState,
    sender: &ChannelIdentity,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match sender.channel {
        Channel::Line      => state.line.send_push(sender, text).await?,
        Channel::WhatsApp  => state.whatsapp.send_push(sender, text).await?,
        Channel::Telegram  => state.telegram.send_push(sender, text).await?,
    }
    Ok(())
}

// ── Event append + projection ─────────────────────────────────────────────── //

async fn append_order_event(
    state: &AppState,
    event: DomainEvent,
    raw_message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let order_id = event.order_id().expect("Worker-agent events are always Order-aggregate");
    let seq = state.order_events.current_sequence(order_id).await?;

    let branch_id: (uuid::Uuid,) =
        sqlx::query_as("SELECT branch_id FROM orders WHERE id = $1")
            .bind(order_id.into_inner())
            .fetch_one(&state.pool)
            .await?;

    state.event_sourcing.append(BranchId::new(branch_id.0), seq + 1, &event).await?;

    // Save raw message so Owner can read it and reply from dashboard
    let _ = state.projections.update_worker_message(order_id, raw_message).await;

    // Fan out notifications AFTER event is stored
    event_handler::fan_out(state, &event).await;

    let signal = state.projection_worker.project_order(order_id).await?;
    state.publish_sse(signal).await;
    Ok(())
}

async fn append_supply_request_event(
    state: &AppState,
    event: DomainEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    let supply_request_id = event
        .supply_request_id()
        .expect("Supplier-agent events are always SupplyRequest-aggregate");
    let seq = state.supply_request_events.current_sequence(supply_request_id).await?;

    let branch_id: (uuid::Uuid,) =
        sqlx::query_as("SELECT branch_id FROM supply_requests WHERE id = $1")
            .bind(supply_request_id.into_inner())
            .fetch_one(&state.pool)
            .await?;

    state.event_sourcing.append(BranchId::new(branch_id.0), seq + 1, &event).await?;

    event_handler::fan_out(state, &event).await;

    let signal = state.projection_worker.project_supply_request(supply_request_id).await?;
    state.publish_sse(signal).await;
    Ok(())
}
