//! Harness routing for the Worker inbound message path.
//!
//! Replaces the old yes/no disambiguation loop in inbox_worker.rs.
//! Called by process_worker_message() for branches using the Gemini provider.
//!
//! Flow:
//!   1. Check disambiguation_pending (rich harness state)
//!      → if exists: harness_continuation()
//!      → if not: harness_fresh()
//!   2. Route HarnessOutput:
//!      → reply: send to worker immediately (always, before anything else)
//!      → event: emit if resolved
//!      → notes: accumulate in disambiguation state or flush to owner alert
//!      → escalate: set ⚠️ badge on order row
//!      → resolved: delete disambiguation_pending, flush all notes

use domain::{BranchId, Channel, ChannelIdentity, DomainEvent, OrderId, WorkerId};
use messaging::ChannelAdapter;
use store::conversation_history::ConversationHistoryRepository;
use store::disambiguation::{DisambiguationStore, DisambiguationUpdate};
use uuid::Uuid;

use agent::harness::{DisambiguationContext, GeminiHarness, HistoryTurn, OrderContext};
use agent::outcome::HarnessOutput;

use crate::event_handler;
use crate::inbox_worker::display_name;
use crate::state::AppState;

// ── Main entry point ──────────────────────────────────────────────────────── //

/// Process one inbound worker message through the Gemini harness.
/// Returns Ok(()) when done — all side effects (send, emit, update) handled internally.
pub async fn process_with_harness(
    state: &AppState,
    sender: &ChannelIdentity,
    worker_id: WorkerId,
    branch_id: Uuid,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let harness = GeminiHarness::new(state.gemini_api_key.clone());

    let sender_key = ConversationHistoryRepository::sender_key(
        sender.channel.as_sql(),
        &sender.external_id,
    );
    let history_repo = ConversationHistoryRepository::new(state.pool.clone());
    let disambig_store = DisambiguationStore::new(state.pool.clone());

    // Persist incoming message immediately
    history_repo.append(&sender_key, "user", text).await?;

    // Load history for harness context
    let history_rows = history_repo.load(&sender_key).await?;
    let history: Vec<HistoryTurn> = history_rows
        .into_iter()
        .map(|r| HistoryTurn { role: r.role, content: r.content })
        .collect();

    // Check for active disambiguation flow
    let pending = disambig_store.find(&sender_key).await?;

    let output = if let Some(ref row) = pending {
        // ── Continuation turn ────────────────────────────────────── //
        let candidates = build_order_contexts_from_ids(state, &row.candidates()).await?;

        let ctx = DisambiguationContext {
            candidates,
            original_intent: row.original_intent.clone(),
            turns_elapsed: row.turns_elapsed,
            extracted_notes_so_far: row.notes(),
            last_question: row.last_question.clone(),
            narrowed_to: row.narrowed_to,
        };

        let worker_name = worker_display_name(state, worker_id).await;

        match harness.harness_continuation(&worker_name, text, &history, &ctx).await {
            Ok(out) => out,
            Err(e) => {
                tracing::error!("harness_continuation failed: {e}");
                // Fallback: acknowledge and keep waiting
                HarnessOutput {
                    resolved: false,
                    order_id: None,
                    event_variant: None,
                    reply: "ขอโทษครับ ระบบมีปัญหาชั่วคราว กรุณาลองใหม่สักครู่".to_string(),
                    extracted_notes: vec![],
                    owner_alert: None,
                    needs_disambiguation: true,
                    narrowed_to: None,
                    disambiguation_question: None,
                    escalate_to_owner: false,
                    owner_escalation_summary: None,
                    confidence: 0.0,
                }
            }
        }
    } else {
        // ── Fresh turn ───────────────────────────────────────────── //
        let active_order_ids = state.actors.active_orders_for_worker(worker_id).await?;
        let active_orders = build_order_contexts_from_ids(state, &active_order_ids
            .iter()
            .map(|id| id.into_inner())
            .collect::<Vec<_>>()).await?;

        if active_orders.is_empty() {
            send_to_worker(state, sender, &sender_key, &history_repo,
                "คุณยังไม่มีงานที่รับอยู่ครับ").await;
            return Ok(());
        }

        // Attach to ThreadContextStore
        {
            let mut threads = state.threads.lock().await;
            for o in &active_orders {
                threads.add_active_order(sender.clone(), OrderId::new(o.order_id));
            }
        }

        let worker_name = worker_display_name(state, worker_id).await;

        match harness.harness_fresh(&worker_name, text, &history, &active_orders).await {
            Ok(out) => out,
            Err(e) => {
                tracing::error!("harness_fresh failed: {e}");
                HarnessOutput {
                    resolved: false,
                    order_id: None,
                    event_variant: None,
                    reply: "ขอโทษครับ ระบบมีปัญหาชั่วคราว กรุณาลองใหม่สักครู่".to_string(),
                    extracted_notes: vec![],
                    owner_alert: None,
                    needs_disambiguation: false,
                    narrowed_to: None,
                    disambiguation_question: None,
                    escalate_to_owner: false,
                    owner_escalation_summary: None,
                    confidence: 0.0,
                }
            }
        }
    };

    // ── Route the HarnessOutput ───────────────────────────────────────────── //

    // 1. Send reply immediately — always first
    send_to_worker(state, sender, &sender_key, &history_repo, &output.reply).await;

    // 2. If resolved — emit event, flush notes, clean up disambiguation
    if output.resolved {
        if let (Some(order_id), Some(variant)) = (output.order_id, output.event_variant) {
            let oid = OrderId::new(order_id);

            // Build and emit the domain event
            let event = build_worker_event(variant, worker_id, oid);

            // Remove from ThreadContextStore if terminal
            if event.is_terminal_for_worker() {
                let mut threads = state.threads.lock().await;
                threads.remove_active_order(sender, oid);
            }

            // Update projection with worker message
            let _ = state.projections.update_worker_message(oid, text).await;
            let _ = state.projections.increment_unread(oid).await;

            // Flag low confidence if needed
            if output.should_flag_low_confidence() {
                let _ = state.projections.flag_low_confidence(oid).await;
            }

            // Surface owner alert if any (notes + escalation summary)
            if let Some(alert) = &output.owner_alert {
                tracing::info!(
                    order_id = %order_id,
                    urgent = alert.urgent,
                    "owner alert: {}",
                    alert.message
                );
                // Flag ⚠️ on dashboard so owner knows to check thread
                let _ = state.projections.flag_low_confidence(oid).await;
            }

            // Emit event
            append_order_event(state, event, oid).await?;

            // Clean up disambiguation
            disambig_store.delete(&sender_key).await?;
        }
        return Ok(());
    }

    // 3. Escalation — owner ⚠️ badge, stop asking worker
    if output.escalate_to_owner {
        // Find most likely order for the badge — use narrowed_to or first candidate
        let target_order_id = output.narrowed_to
            .or_else(|| pending.as_ref()?.candidates().first().copied());

        if let Some(oid) = target_order_id {
            let _ = state.projections.flag_low_confidence(OrderId::new(oid)).await;
            let _ = state.projections.increment_unread(OrderId::new(oid)).await;

            // SSE to push ⚠️ badge to dashboard immediately
            let branch_id_typed: Option<Uuid> = sqlx::query_scalar(
                "SELECT branch_id FROM orders WHERE id = $1",
            )
            .bind(oid)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();

            if let Some(bid) = branch_id_typed {
                state.publish_sse(domain::SseSignal::OrderChanged {
                    order_id: OrderId::new(oid),
                    branch_id: domain::BranchId::new(bid),
                }).await;
            }
        }

        // Update disambiguation state with escalation
        disambig_store.update(&sender_key, DisambiguationUpdate {
            new_notes: output.extracted_notes.clone(),
            narrowed_to: output.narrowed_to,
            last_question: None,
            escalate: true,
        }).await?;

        return Ok(());
    }

    // 4. Needs disambiguation — create or update the pending state
    if output.needs_disambiguation {
        if let Some(ref row) = pending {
            // Update existing state
            disambig_store.update(&sender_key, DisambiguationUpdate {
                new_notes: output.extracted_notes.clone(),
                narrowed_to: output.narrowed_to,
                last_question: output.disambiguation_question.clone(),
                escalate: false,
            }).await?;
        } else {
            // Fresh disambiguation — determine candidates and original intent
            let active_order_ids = state.actors.active_orders_for_worker(worker_id).await?;
            let candidate_uuids: Vec<Uuid> = active_order_ids
                .iter()
                .map(|id| id.into_inner())
                .collect();

            let intent_str = output.event_variant.map(|v| v.as_sql().to_string());

            disambig_store.create(
                &sender_key,
                text,
                &candidate_uuids,
                "order",
                intent_str.as_deref(),
            ).await?;

            // If notes were extracted on turn 1, update immediately
            if !output.extracted_notes.is_empty() {
                disambig_store.update(&sender_key, DisambiguationUpdate {
                    new_notes: output.extracted_notes.clone(),
                    narrowed_to: output.narrowed_to,
                    last_question: output.disambiguation_question.clone(),
                    escalate: false,
                }).await?;
            }
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────── //

async fn send_to_worker(
    state: &AppState,
    sender: &ChannelIdentity,
    sender_key: &str,
    history_repo: &ConversationHistoryRepository,
    text: &str,
) {
    let result = match sender.channel {
        Channel::Line     => state.line.send_push(sender, text).await,
        Channel::WhatsApp => state.whatsapp.send_push(sender, text).await,
        Channel::Telegram => state.telegram.send_push(sender, text).await,
    };
    if let Err(e) = result {
        tracing::error!("push to {} failed: {e}", sender.external_id);
    }
    let _ = history_repo.append(sender_key, "assistant", text).await;
}

async fn build_order_contexts_from_ids(
    state: &AppState,
    ids: &[Uuid],
) -> Result<Vec<OrderContext>, Box<dyn std::error::Error>> {
    let mut out = Vec::with_capacity(ids.len());
    for &id in ids {
        let row: Option<(Option<String>, String, String, Option<chrono::DateTime<chrono::Utc>>)> =
            sqlx::query_as(
                "SELECT o.short_name,
                        COALESCE(
                            (SELECT new_description FROM order_description_edits
                             WHERE order_id = $1 ORDER BY id DESC LIMIT 1),
                            o.description
                        ),
                        ocs.state,
                        ocs.last_event_at
                 FROM orders o
                 JOIN order_current_state ocs ON ocs.order_id = o.id
                 WHERE o.id = $1 AND o.deleted_at IS NULL",
            )
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;

        if let Some((short_name, description, state_str, last_event_at)) = row {
            out.push(OrderContext {
                order_id: id,
                short_name,
                description,
                state: state_str,
                last_event_at: last_event_at.map(|d| d.to_rfc3339()),
            });
        }
    }
    Ok(out)
}

async fn worker_display_name(state: &AppState, worker_id: WorkerId) -> String {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT name FROM workers WHERE id = $1")
            .bind(worker_id.into_inner())
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    row.map(|(n,)| n).unwrap_or_else(|| "Worker".to_string())
}

fn build_worker_event(
    variant: domain::DomainEventVariant,
    worker_id: WorkerId,
    order_id: OrderId,
) -> DomainEvent {
    use domain::DomainEventVariant::*;
    match variant {
        WorkerAccepted         => DomainEvent::WorkerAccepted { worker_id, order_id },
        WorkerUnavailable      => DomainEvent::WorkerUnavailable { worker_id, order_id },
        WorkerCancelled        => DomainEvent::WorkerCancelled { worker_id, order_id },
        ClarificationRequested => DomainEvent::ClarificationRequested { worker_id, order_id },
        WorkerReadyForPickup   => DomainEvent::WorkerReadyForPickup { worker_id, order_id },
        OrderDone              => DomainEvent::OrderDone { order_id },
        _ => DomainEvent::ClarificationRequested { worker_id, order_id },
    }
}

async fn append_order_event(
    state: &AppState,
    event: DomainEvent,
    order_id: OrderId,
) -> Result<(), Box<dyn std::error::Error>> {
    let seq = state.order_events.current_sequence(order_id).await?;
    let (branch_id,): (Uuid,) =
        sqlx::query_as("SELECT branch_id FROM orders WHERE id = $1")
            .bind(order_id.into_inner())
            .fetch_one(&state.pool)
            .await?;

    state.event_sourcing
        .append(BranchId::new(branch_id), seq + 1, &event)
        .await?;

    event_handler::fan_out(state, &event).await;

    let signal = state.projection_worker.project_order(order_id).await?;
    state.publish_sse(signal).await;
    Ok(())
}