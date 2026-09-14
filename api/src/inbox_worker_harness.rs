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
//!      → event: emit if resolved AND both order_id + event_variant present
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
use crate::state::AppState;

// ── Main entry point ──────────────────────────────────────────────────────── //

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

    // 1. Send reply immediately — always first, before any event emission
    send_to_worker(state, sender, &sender_key, &history_repo, &output.reply).await;

    // If harness resolved cleanly on a fresh turn (no prior pending row),
    // make sure any stale old disambiguation row is wiped so it can't interfere.
    if output.resolved && pending.is_none() {
        let _ = disambig_store.delete(&sender_key).await;
    }

    // 2. If resolved — emit event. If event_variant is None, infer from order state.
    if output.resolved {
        // Resolve variant: use Gemini's answer, or infer from order state in DB.
        let resolved_variant = match output.event_variant {
            Some(v) => Some(v),
            None => {
                if let Some(oid) = output.order_id {
                    infer_variant_from_state(state, oid).await
                } else {
                    None
                }
            }
        };

        match (output.order_id, resolved_variant) {
            (Some(order_id), Some(variant)) => {
                let oid = OrderId::new(order_id);

                let event = build_worker_event(variant, worker_id, oid);

                // Remove from ThreadContextStore if terminal
                if event.is_terminal_for_worker() {
                    let mut threads = state.threads.lock().await;
                    threads.remove_active_order(sender, oid);
                }

                // Update projection with worker message
                let _ = state.projections.update_worker_message(oid, text).await;
                let _ = state.projections.increment_unread(oid).await;

                // Emit domain event — updates projection + fires SSE
                if let Err(e) = append_order_event(state, event, oid).await {
                    tracing::error!("append_order_event failed for order {oid}: {e}");
                }

                // Flag low confidence AFTER projection upsert so it isn't overwritten,
                // then fire a second SSE so dashboard picks up the badge.
                if output.should_flag_low_confidence() {
                    let _ = state.projections.flag_low_confidence(oid).await;
                }

                // Surface owner alert if any — log always, badge for all alerts
                if let Some(alert) = &output.owner_alert {
                    tracing::info!(
                        order_id = %order_id,
                        urgent = alert.urgent,
                        "owner alert: {}",
                        alert.message
                    );
                    let flag_result = state.projections.flag_low_confidence(oid).await;
                    tracing::info!(
                        order_id = %order_id,
                        flag_ok = flag_result.is_ok(),
                        "flag_low_confidence called after append"
                    );
                    // Fire SSE after flagging so dashboard re-fetches with badge set
                    let branch_id_for_sse: Option<uuid::Uuid> = sqlx::query_scalar(
                        "SELECT branch_id FROM orders WHERE id = $1",
                    )
                    .bind(order_id)
                    .fetch_optional(&state.pool)
                    .await
                    .ok()
                    .flatten();
                    if let Some(bid) = branch_id_for_sse {
                        state.publish_sse(domain::SseSignal::OrderChanged {
                            order_id: oid,
                            branch_id: domain::BranchId::new(bid),
                        }).await;
                    }
                }

                // Clean up disambiguation state
                if let Err(e) = disambig_store.delete(&sender_key).await {
                    tracing::warn!("failed to delete disambiguation state: {e}");
                }
            }
            (order_id, event_variant) => {
                // resolved=true but missing order_id or event_variant — harness bug
                // Don't delete disambiguation; let worker retry
                tracing::warn!(
                    sender = %sender.external_id,
                    order_id = ?order_id,
                    event_variant = ?event_variant,
                    "Harness returned resolved=true but incomplete output — no event emitted"
                );
            }
        }
        return Ok(());
    }

    // 3. Escalation — set ⚠️ badge, stop asking worker
    if output.escalate_to_owner {
        let target_order_id = output.narrowed_to
            .or_else(|| pending.as_ref().and_then(|r| r.candidates().first().copied()));

        if let Some(oid) = target_order_id {
            let _ = state.projections.flag_low_confidence(OrderId::new(oid)).await;
            let _ = state.projections.increment_unread(OrderId::new(oid)).await;

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

        if let Err(e) = disambig_store.update(&sender_key, DisambiguationUpdate {
            new_notes: output.extracted_notes.clone(),
            narrowed_to: output.narrowed_to,
            last_question: None,
            escalate: true,
        }).await {
            tracing::warn!("failed to update disambiguation for escalation: {e}");
        }

        return Ok(());
    }

    // 4. Needs disambiguation — create or update pending state
    if output.needs_disambiguation {
        if pending.is_some() {
            // Update existing state
            if let Err(e) = disambig_store.update(&sender_key, DisambiguationUpdate {
                new_notes: output.extracted_notes.clone(),
                narrowed_to: output.narrowed_to,
                last_question: output.disambiguation_question.clone(),
                escalate: false,
            }).await {
                tracing::warn!("failed to update disambiguation state: {e}");
            }
        } else {
            // Fresh disambiguation flow
            let active_order_ids = state.actors.active_orders_for_worker(worker_id).await?;
            let candidate_uuids: Vec<Uuid> = active_order_ids
                .iter()
                .map(|id| id.into_inner())
                .collect();

            let intent_str = output.event_variant.map(|v| v.as_sql().to_string());

            if let Err(e) = disambig_store.create(
                &sender_key,
                text,
                &candidate_uuids,
                "order",
                intent_str.as_deref(),
            ).await {
                tracing::warn!("failed to create disambiguation state: {e}");
            }

            // If notes extracted on turn 1, persist them immediately
            if !output.extracted_notes.is_empty() || output.narrowed_to.is_some() {
                if let Err(e) = disambig_store.update(&sender_key, DisambiguationUpdate {
                    new_notes: output.extracted_notes.clone(),
                    narrowed_to: output.narrowed_to,
                    last_question: output.disambiguation_question.clone(),
                    escalate: false,
                }).await {
                    tracing::warn!("failed to update disambiguation with initial notes: {e}");
                }
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
        // Fallback for any unexpected variant — open clarification so owner can see
        _ => {
            tracing::warn!("unexpected variant {:?} in build_worker_event, using ClarificationRequested", variant);
            DomainEvent::ClarificationRequested { worker_id, order_id }
        }
    }
}

/// When Gemini resolves an order but omits event_variant, infer the most
/// likely event from the current DB state. This handles the common case where
/// the model says "yes that order" but forgets to emit the variant field.
async fn infer_variant_from_state(
    state: &AppState,
    order_id: Uuid,
) -> Option<domain::DomainEventVariant> {
    use domain::DomainEventVariant::*;

    let row: Option<(String,)> = sqlx::query_as(
        "SELECT state FROM order_current_state WHERE order_id = $1",
    )
    .bind(order_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();

    let state_str = row?.0;

    // Map DB state to the most natural next event a worker would send
    let variant = match state_str.as_str() {
        "ASSIGNED"              => WorkerAccepted,       // worker saying they accept
        "ACCEPTED"              => WorkerReadyForPickup, // worker saying they're ready
        "READY_FOR_PICKUP"      => OrderDone,            // worker saying job done
        "PENDING_CLARIFICATION" => ClarificationRequested,
        _ => {
            tracing::warn!(
                order_id = %order_id,
                state = %state_str,
                "Cannot infer event_variant from state — skipping event"
            );
            return None;
        }
    };

    tracing::info!(
        order_id = %order_id,
        state = %state_str,
        inferred_variant = ?variant,
        "Inferred event_variant from order state (Gemini omitted it)"
    );

    Some(variant)
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