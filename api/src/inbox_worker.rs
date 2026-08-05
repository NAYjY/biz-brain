//! T04's async processing step: drains `webhook_inbox`, resolves the sender
//! to a Worker/Supplier, hands the message to `agent` (T03), and appends any
//! resulting `DomainEvent`. This is the only place `agent` is invoked from —
//! never the webhook handler itself (T04's sync-vs-async resolution).

use std::time::Duration;

use agent::InterpretationOutcome;
use domain::{BranchId, Channel, ChannelIdentity, DomainEvent};
use messaging::ChannelAdapter;

use crate::state::AppState;

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
            // T03: drop, don't retry — still mark processed so it isn't retried forever.
        }
        let _ = state.inbox.mark_processed(row.id).await;
    }

    Ok(())
}

async fn process_row(state: &AppState, row: &store::webhook_inbox::InboxRow) -> Result<(), Box<dyn std::error::Error>> {
    let sender: ChannelIdentity = serde_json::from_value(row.raw_payload["sender"].clone())?;
    let text = row.raw_payload["text"].as_str().unwrap_or_default();
    let channel = if row.channel == "line" { Channel::Line } else { Channel::WhatsApp };

    match channel {
        Channel::Line => process_worker_message(state, &sender, text).await,
        Channel::WhatsApp => process_supplier_message(state, &sender, text).await,
    }
}

async fn process_worker_message(state: &AppState, sender: &ChannelIdentity, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let Some(worker_id) = state.actors.resolve_worker(sender).await? else {
        tracing::warn!("message from unknown Worker identity {sender:?}, dropping");
        return Ok(());
    };

    let outcome = {
        let mut threads = state.threads.lock().await;
        // Refresh active-Order set from the store before interpreting (T03).
        for order_id in state.actors.active_orders_for_worker(worker_id).await? {
            threads.add_active_order(sender.clone(), order_id);
        }
        state.worker_agent.interpret(text, worker_id, sender, &threads).await?
    };

    match outcome {
        InterpretationOutcome::Event(event) => append_order_event(state, event).await,
        InterpretationOutcome::NeedsOrderDisambiguation { question, .. } => {
            state.line.send_push(sender, &question).await?;
            Ok(())
        }
        InterpretationOutcome::Unprocessed { reason } => {
            tracing::warn!("Owner alert (urgent): message from Worker unprocessed — {reason}");
            Ok(())
        }
    }
}

async fn process_supplier_message(state: &AppState, sender: &ChannelIdentity, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let Some(_supplier_id) = state.actors.resolve_supplier(sender).await? else {
        tracing::warn!("message from unknown Supplier identity {sender:?}, dropping");
        return Ok(());
    };

    let outcome = state.supplier_agent.interpret(text).await?;
    match outcome {
        InterpretationOutcome::Event(event) => append_supply_request_event(state, event).await,
        InterpretationOutcome::Unprocessed { reason } => {
            // InvoiceReceived needs invoice-detail extraction beyond text
            // classification (line-items, totals) — flagged in agent's
            // SupplierAgent as a follow-up, not built in this pass.
            tracing::warn!("Owner alert (urgent): message from Supplier unprocessed — {reason}");
            Ok(())
        }
        InterpretationOutcome::NeedsOrderDisambiguation { .. } => unreachable!("SupplierAgent never disambiguates by Order"),
    }
}

async fn append_order_event(state: &AppState, event: DomainEvent) -> Result<(), Box<dyn std::error::Error>> {
    let order_id = event.order_id().expect("Worker-agent events are always Order-aggregate events");
    let seq = state.order_events.current_sequence(order_id).await?;

    let branch_id: (uuid::Uuid,) =
        sqlx::query_as("SELECT branch_id FROM orders WHERE id = $1").bind(order_id.into_inner()).fetch_one(&state.pool).await?;

    state.event_sourcing.append(BranchId::new(branch_id.0), seq + 1, &event).await?;
    let signal = state.projection_worker.project_order(order_id).await?;
    state.publish_sse(signal).await;
    Ok(())
}

async fn append_supply_request_event(state: &AppState, event: DomainEvent) -> Result<(), Box<dyn std::error::Error>> {
    let supply_request_id = event.supply_request_id().expect("Supplier-agent events are always SupplyRequest-aggregate events");
    let seq = state.supply_request_events.current_sequence(supply_request_id).await?;

    let branch_id: (uuid::Uuid,) = sqlx::query_as("SELECT branch_id FROM supply_requests WHERE id = $1")
        .bind(supply_request_id.into_inner())
        .fetch_one(&state.pool)
        .await?;

    state.event_sourcing.append(BranchId::new(branch_id.0), seq + 1, &event).await?;
    let signal = state.projection_worker.project_supply_request(supply_request_id).await?;
    state.publish_sse(signal).await;
    Ok(())
}
