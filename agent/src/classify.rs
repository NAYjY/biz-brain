//! P01/P13: Claude classify call — prompt harness with role framing, domain
//! context, current OrderState injection, Thai+English few-shot examples,
//! conversation history window, and structured output validation.
//!
//! Returns `{ variant, order_id }` from Claude (P13).
//! `order_id` is null when Claude cannot determine the order from context.

use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::outcome::InterpretationError;
use domain::DomainEventVariant;

const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
const MODEL: &str = "claude-sonnet-4-6";
/// Keep the last 10 turns per sender (= up to 20 messages in the array).
const MAX_HISTORY_WINDOW: usize = 10;

/// A single turn of conversation history.
#[derive(Debug, Clone)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
}

/// Active order/supply-request context injected into the classify prompt.
#[derive(Debug, Clone)]
pub struct ActiveOrderContext {
    pub order_id: Uuid,
    pub description: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
struct ClassifyResult {
    variant: String,
    order_id: Option<Uuid>,
}

pub struct ClaudeClassifier {
    http: reqwest::Client,
    api_key: String,
}

impl ClaudeClassifier {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { http: reqwest::Client::new(), api_key: api_key.into() }
    }

    /// Classify a Worker message (LINE / Telegram path).
    pub async fn classify_worker_message(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_orders: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError> {
        let allowed = "worker_accepted, worker_unavailable, worker_cancelled, \
                       clarification_requested, worker_ready_for_pickup, order_done, none";
        self.classify(message, history, active_orders, allowed, "worker").await
    }

    /// Classify a Supplier message (WhatsApp path).
    pub async fn classify_supplier_message(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_supply_requests: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError> {
        let allowed = "invoice_received, supplier_confirmed, none";
        self.classify(message, history, active_supply_requests, allowed, "supplier").await
    }

    async fn classify(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_contexts: &[ActiveOrderContext],
        allowed_variants: &str,
        actor_role: &str,
    ) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError> {
        let preamble = build_preamble(actor_role, allowed_variants, active_contexts);
        let messages = build_messages_array(&preamble, history, message);

        let body = json!({
            "model": MODEL,
            "max_tokens": 200,
            "messages": messages,
        });

        let response = self
            .http
            .post(CLAUDE_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| InterpretationError::ClaudeApi(e.to_string()))?;

        let raw: Value = response
            .json()
            .await
            .map_err(|e| InterpretationError::ClaudeApi(e.to_string()))?;

        let text = raw["content"][0]["text"].as_str().ok_or_else(|| {
            InterpretationError::ParseFailed("no text block in Claude response".into())
        })?;

        parse_and_validate(text.trim(), allowed_variants)
    }
}

// ── Prompt construction ──────────────────────────────────────────────────── //

fn build_preamble(
    actor_role: &str,
    allowed_variants: &str,
    active_contexts: &[ActiveOrderContext],
) -> String {
    let role_desc = match actor_role {
        "supplier" => "a Supplier communicating via WhatsApp about supply requests and invoices",
        _ => "a Worker (field technician) communicating via LINE or Telegram about assigned orders",
    };

    let context_list = if active_contexts.is_empty() {
        "  (none — sender has no active orders/requests)".to_string()
    } else {
        active_contexts
            .iter()
            .map(|o| format!(
                "  - id={} state={} desc=\"{}\"",
                o.order_id, o.state, o.description
            ))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let few_shot = if actor_role == "worker" { worker_few_shot() } else { supplier_few_shot() };

    format!(
        r#"You are a message classifier for Biz-Brain, a field-service coordination platform.

The sender is {role_desc}.

Active context for this sender:
{context_list}

Classify the LAST user message into exactly one of [{allowed_variants}].
If you can determine which specific order/request the message refers to from the context above,
include its UUID as order_id. Otherwise set order_id to null.
If the message is ambiguous or does not match any known action, use "none".

Respond ONLY with valid JSON — no markdown, no preamble:
{{"variant": "<value>", "order_id": "<uuid or null>"}}

{few_shot}"#
    )
}

fn worker_few_shot() -> &'static str {
    r#"Examples (Thai, English, and mixed messages are all common):
User: รับงานแล้วครับ      → {"variant":"worker_accepted","order_id":null}
User: accept              → {"variant":"worker_accepted","order_id":null}
User: โอเครับได้           → {"variant":"worker_accepted","order_id":null}
User: ไม่ว่างครับ          → {"variant":"worker_unavailable","order_id":null}
User: can't make it       → {"variant":"worker_unavailable","order_id":null}
User: ขอยกเลิกครับ        → {"variant":"worker_cancelled","order_id":null}
User: cancel this one     → {"variant":"worker_cancelled","order_id":null}
User: ไม่เข้าใจงานนี้       → {"variant":"clarification_requested","order_id":null}
User: question about job  → {"variant":"clarification_requested","order_id":null}
User: พร้อมรับของแล้ว      → {"variant":"worker_ready_for_pickup","order_id":null}
User: ready for pickup    → {"variant":"worker_ready_for_pickup","order_id":null}
User: เสร็จแล้วครับ        → {"variant":"order_done","order_id":null}
User: done                → {"variant":"order_done","order_id":null}
User: สวัสดี              → {"variant":"none","order_id":null}"#
}

fn supplier_few_shot() -> &'static str {
    r#"Examples:
User: ส่งใบเสนอราคาแล้วนะครับ → {"variant":"invoice_received","order_id":null}
User: here is the invoice       → {"variant":"invoice_received","order_id":null}
User: ยืนยันแล้วครับ             → {"variant":"supplier_confirmed","order_id":null}
User: confirmed                  → {"variant":"supplier_confirmed","order_id":null}
User: สวัสดี                      → {"variant":"none","order_id":null}"#
}

fn build_messages_array(
    preamble: &str,
    history: &[HistoryMessage],
    new_message: &str,
) -> Vec<Value> {
    let mut messages = Vec::with_capacity(MAX_HISTORY_WINDOW * 2 + 3);

    // Preamble as first user turn — no system prompt (P13 decision).
    messages.push(json!({ "role": "user", "content": preamble }));
    messages.push(json!({
        "role": "assistant",
        "content": "Understood. I will classify each message you send as JSON."
    }));

    // Sliding window of recent history.
    let start = history.len().saturating_sub(MAX_HISTORY_WINDOW);
    for msg in &history[start..] {
        messages.push(json!({ "role": msg.role, "content": msg.content }));
    }

    messages.push(json!({ "role": "user", "content": new_message }));
    messages
}

// ── Response parsing and validation ─────────────────────────────────────── //

fn parse_and_validate(
    text: &str,
    allowed_variants: &str,
) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError> {
    let cleaned = text
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let result: ClassifyResult = serde_json::from_str(cleaned).map_err(|e| {
        InterpretationError::ParseFailed(format!("JSON parse: {e} — raw: {cleaned}"))
    })?;

    // P01: validate against allowed set — never silently accept unexpected values.
    let allowed: Vec<&str> = allowed_variants.split(", ").collect();
    if !allowed.contains(&result.variant.as_str()) {
        return Err(InterpretationError::UnexpectedVariant {
            received: result.variant,
            allowed: allowed_variants.to_string(),
        });
    }

    if result.variant == "none" {
        return Ok(None);
    }

    let variant = sql_to_variant(&result.variant).ok_or_else(|| {
        InterpretationError::ParseFailed(format!("unrecognised variant: {}", result.variant))
    })?;

    Ok(Some((variant, result.order_id)))
}

fn sql_to_variant(s: &str) -> Option<DomainEventVariant> {
    Some(match s {
        "worker_assigned"         => DomainEventVariant::WorkerAssigned,
        "worker_accepted"         => DomainEventVariant::WorkerAccepted,
        "worker_unavailable"      => DomainEventVariant::WorkerUnavailable,
        "worker_cancelled"        => DomainEventVariant::WorkerCancelled,
        "clarification_requested" => DomainEventVariant::ClarificationRequested,
        "worker_ready_for_pickup" => DomainEventVariant::WorkerReadyForPickup,
        "order_done"              => DomainEventVariant::OrderDone,
        "invoice_received"        => DomainEventVariant::InvoiceReceived,
        "supplier_confirmed"      => DomainEventVariant::SupplierConfirmed,
        _                         => return None,
    })
}
