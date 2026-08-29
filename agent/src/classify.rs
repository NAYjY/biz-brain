//! T01 / P01 / P13: AI classify clients with timeout + retry hardening.
//!
//! Both `ClaudeClassifier` and `GeminiClassifier` live here — they do the
//! same job (message → variant JSON) and share prompt construction, response
//! parsing, and the retry loop. Keeping them together avoids duplication and
//! makes it easy to see the differences at a glance (endpoint, auth, request
//! shape, response path).
//!
//! Retry policy (T01 resolution):
//!   - 8s timeout per attempt
//!   - 3 total attempts (1 + 2 retries) with 500ms / 1500ms backoff
//!   - Retry on: network error, timeout, HTTP 429, HTTP 5xx
//!   - Fail fast on: HTTP 400, 401, 422, parse error, unexpected variant
//!   - Exhausted retries → caller falls through to ClarificationRequested

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

use crate::outcome::InterpretationError;
use crate::provider::ClassifyClient;
use domain::DomainEventVariant;

// ── Shared prompt types ───────────────────────────────────────────────────── //

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

// ── Retry constants ───────────────────────────────────────────────────────── //

const CALL_TIMEOUT: Duration = Duration::from_secs(8);
const RETRY_DELAYS: [Duration; 2] = [
    Duration::from_millis(500),
    Duration::from_millis(1500),
];
const MAX_ATTEMPTS: usize = 3;

// ── Claude ────────────────────────────────────────────────────────────────── //

const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
const CLAUDE_MODEL: &str = "claude-sonnet-4-6";

pub struct ClaudeClassifier {
    http: reqwest::Client,
    api_key: String,
}

impl ClaudeClassifier {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { http: reqwest::Client::new(), api_key: api_key.into() }
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
            "model": CLAUDE_MODEL,
            "max_tokens": 200,
            "messages": messages,
        });

        let raw = with_retry(|| {
            let req = self
                .http
                .post(CLAUDE_API_URL)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            async move {
                let resp = req.send().await
                    .map_err(|e| InterpretationError::ClaudeApi(e.to_string()).mark_transient())?;
                let status = resp.status();
                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    let err = InterpretationError::ClaudeApi(format!("HTTP {status}: {text}"));
                    return Err(if status.as_u16() == 429 || status.is_server_error() {
                        err.mark_transient()
                    } else {
                        err
                    });
                }
                resp.json::<Value>().await
                    .map_err(|e| InterpretationError::ClaudeApi(e.to_string()))
            }
        })
        .await?;

        let text = raw["content"][0]["text"].as_str().ok_or_else(|| {
            InterpretationError::ParseFailed("no text block in Claude response".into())
        })?;

        parse_and_validate(text.trim(), allowed_variants)
    }
}

#[async_trait]
impl ClassifyClient for ClaudeClassifier {
    async fn classify_worker(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_orders: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError> {
        let allowed = "worker_accepted, worker_unavailable, worker_cancelled, \
                       clarification_requested, worker_ready_for_pickup, order_done, none";
        self.classify(message, history, active_orders, allowed, "worker").await
    }

    async fn classify_supplier(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_supply_requests: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError> {
        let allowed = "invoice_received, supplier_confirmed, none";
        self.classify(message, history, active_supply_requests, allowed, "supplier").await
    }
}

// ── Gemini ────────────────────────────────────────────────────────────────── //

// Gemini 1.5 Flash — fast, cheap, good at structured JSON output.
const GEMINI_MODEL: &str = "gemini-3.5-flash-lite";
// URL template: key appended at call time.
const GEMINI_API_BASE: &str =
    "https://generativelanguage.googleapis.com/v1beta/models";

pub struct GeminiClassifier {
    http: reqwest::Client,
    api_key: String,
}

impl GeminiClassifier {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { http: reqwest::Client::new(), api_key: api_key.into() }
    }

    async fn classify(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_contexts: &[ActiveOrderContext],
        allowed_variants: &str,
        actor_role: &str,
    ) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError> {
        // Gemini uses a single "contents" array with role:user / role:model turns.
        let preamble = build_preamble(actor_role, allowed_variants, active_contexts);
        let mut contents: Vec<Value> = Vec::new();

        // System preamble as first user turn.
        contents.push(json!({ "role": "user", "parts": [{ "text": preamble }] }));
        contents.push(json!({
            "role": "model",
            "parts": [{ "text": "Understood. I will classify each message you send as JSON." }]
        }));

        // History window.
        let start = history.len().saturating_sub(10);
        for msg in &history[start..] {
            // Gemini uses "model" not "assistant".
            let role = if msg.role == "assistant" { "model" } else { "user" };
            contents.push(json!({ "role": role, "parts": [{ "text": msg.content }] }));
        }

        contents.push(json!({ "role": "user", "parts": [{ "text": message }] }));

        let body = json!({
            "contents": contents,
            "generationConfig": {
                "maxOutputTokens": 20000,
                "temperature": 0.0,
            }
        });

        let url = format!(
            "{GEMINI_API_BASE}/{GEMINI_MODEL}:generateContent?key={}",
            self.api_key
        );

        // 👇 ADD THIS — logs the full prompt body
    tracing::debug!(
        target: "gemini",
        prompt = %serde_json::to_string_pretty(&body).unwrap_or_default(),
        "Gemini request"
    );

        let raw = with_retry(|| {
            let req = self.http.post(&url).json(&body);
            async move {
                let resp = req.send().await
                    .map_err(|e| InterpretationError::ClaudeApi(
                        format!("Gemini network: {e}")).mark_transient())?;
                let status = resp.status();
                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    let err = InterpretationError::ClaudeApi(
                        format!("Gemini HTTP {status}: {text}"));
                    return Err(if status.as_u16() == 429 || status.is_server_error() {
                        err.mark_transient()
                    } else {
                        err
                    });
                }
                resp.json::<Value>().await
                    .map_err(|e| InterpretationError::ClaudeApi(
                        format!("Gemini parse: {e}")))
            }
        })
        .await?;

        // 👇 ADD THIS — logs the raw response
    tracing::debug!(
        target: "gemini",
        response = %serde_json::to_string_pretty(&raw).unwrap_or_default(),
        "Gemini response"
    );

        // Gemini response path: candidates[0].content.parts[0].text
        let text = raw["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| {
                InterpretationError::ParseFailed(
                    format!("unexpected Gemini response shape: {raw}"))
            })?;

        // 👇 ADD THIS — logs the extracted text and final parsed result
    tracing::info!(
        target: "gemini",
        message = %message,
        extracted_text = %text,
        "Gemini classified"
    );

        parse_and_validate(text.trim(), allowed_variants)
    }
}

#[async_trait]
impl ClassifyClient for GeminiClassifier {
    async fn classify_worker(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_orders: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError> {
        let allowed = "worker_accepted, worker_unavailable, worker_cancelled, \
                       clarification_requested, worker_ready_for_pickup, order_done, none";
        self.classify(message, history, active_orders, allowed, "worker").await
    }

    async fn classify_supplier(
        &self,
        message: &str,
        history: &[HistoryMessage],
        active_supply_requests: &[ActiveOrderContext],
    ) -> Result<Option<(DomainEventVariant, Option<Uuid>)>, InterpretationError> {
        let allowed = "invoice_received, supplier_confirmed, none";
        self.classify(message, history, active_supply_requests, allowed, "supplier").await
    }
}

// ── Retry loop ────────────────────────────────────────────────────────────── //

/// Calls `make_request` up to `MAX_ATTEMPTS` times with timeout + backoff.
/// Only transient errors (marked with `.mark_transient()`) are retried.
async fn with_retry<F, Fut>(mut make_request: F) -> Result<Value, InterpretationError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Value, InterpretationError>>,
{
    let mut last_err = InterpretationError::Timeout;

    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            sleep(RETRY_DELAYS[attempt - 1]).await;
        }

        match timeout(CALL_TIMEOUT, make_request()).await {
            Err(_elapsed) => {
                // Timed out — always retry.
                last_err = InterpretationError::Timeout;
            }
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(e)) => {
                if e.is_retryable() {
                    last_err = e;
                } else {
                    return Err(e);
                }
            }
        }
    }

    Err(last_err)
}

// ── Prompt construction (shared) ──────────────────────────────────────────── //

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
        "  (none — sender has no active orders)".to_string()
    } else {
        active_contexts
            .iter()
            .map(|o| format!(
                "  - order_id=\"{}\" state=\"{}\" name=\"{}\"",
                o.order_id, o.state, o.description
            ))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let variant_guide = if actor_role == "worker" {
        r#"What each variant means and when it is valid:
  worker_accepted         — Worker agrees to take the order.
                            Valid when state = worker_assigned.
                            Signals: รับงาน, รับ, โอเค, ok, sure, ได้, รับทราบ (when bot just asked to accept/reject)
  worker_unavailable      — Worker cannot do this order.
                            Valid when state = worker_assigned.
                            Signals: ไม่ว่าง, ไม่รับ, can't, cannot, ไม่ได้ (when declining the order)
  worker_cancelled        — Worker backs out after already accepting.
                            Valid when state = worker_accepted, clarification_requested.
                            Signals: ยกเลิก, ขอยกเลิก, cancel
  clarification_requested — Worker has a question or does not understand the job.
                            Valid when state = worker_assigned, worker_accepted.
                            Signals: ถาม, ไม่เข้าใจ, question, ?, what is, ขอถาม
  worker_ready_for_pickup — Worker is physically ready to collect materials.
                            Valid when state = worker_accepted, clarification_requested.
                            Signals: พร้อม, พร้อมรับ, ready, on my way to pick up
  order_done              — Worker reports the job is complete.
                            Valid when state = worker_ready_for_pickup, worker_accepted.
                            Signals: เสร็จ, เสร็จแล้ว, เรียบร้อย, done, finished, complete
  none                    — Message is irrelevant: greeting, noise, or unrelated chatter.
                            Use when no variant fits or state does not allow any valid transition."#
    } else {
        r#"What each variant means and when it is valid:
  invoice_received   — Supplier is sending or referencing an invoice or price quote.
                       Signals: invoice, ใบเสนอราคา, ราคา, here is the quote
  supplier_confirmed — Supplier confirms they will fulfil the supply request.
                       Signals: ยืนยัน, ยืนยันแล้ว, confirmed, will do
  none               — Message is irrelevant or does not map to any variant."#
    };

    let tie_breaking = if actor_role == "worker" {
        r#"Tie-breaking rules for ambiguous messages:
  - รับทราบ / ทราบแล้ว / noted / acknowledged
      → If the last bot message asked the worker to accept or reject an order:
          classify as worker_accepted (worker is acknowledging they accept)
      → Otherwise: none
  - โอเค / ok / sure / ได้
      → If state = worker_assigned: worker_accepted
      → If state = worker_accepted and bot asked about pickup: worker_ready_for_pickup
      → Otherwise: none
  - เสร็จ / done / เรียบร้อย
      → If state = worker_ready_for_pickup or worker_accepted: order_done
      → Otherwise: none
  - พร้อม / ready
      → If state = worker_accepted: worker_ready_for_pickup
      → Otherwise: none
  - Worker replies with an order name or number (e.g. "Work005")
      → If the last bot message asked which order this is about:
          set order_id to that order and classify based on prior context
      → Otherwise: none"#
    } else {
        ""
    };

    let few_shot = if actor_role == "worker" {
        worker_few_shot()
    } else {
        supplier_few_shot()
    };

    format!(
        r#"You are a message classifier for Biz-Brain.

The sender is {role_desc}.

Their active orders RIGHT NOW:
{context_list}

{variant_guide}

{tie_breaking}

Your job:
1. Read the full conversation history to understand context before classifying
2. Classify the worker's LAST message into one of [{allowed_variants}]
3. Use the current order state and the last bot message to resolve ambiguous signals
4. If the message clearly refers to a specific order, set order_id to that order's UUID
5. If only ONE active order exists, assume the message is about that order
6. If multiple orders exist and it is unclear which one, set order_id to null

Respond ONLY with valid JSON, nothing else:
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
    let history_text = if history.is_empty() {
        String::new()
    } else {
        let start = history.len().saturating_sub(5);
        let lines: Vec<String> = history[start..]
            .iter()
            .map(|m| {
                // Clearly distinguish bot prompt from worker reply
                let role = match m.role.as_str() {
                    "user" => "Worker",
                    "assistant" | "bot" => "Bot",
                    _ => "Bot",
                };
                format!("{}: {}", role, m.content)
            })
            .collect();
        format!("\nRecent conversation:\n{}\n", lines.join("\n"))
    };

    let full_prompt = format!(
        "{}{}\nNow classify this message:",
        preamble, history_text
    );

    vec![
        json!({ "role": "user", "content": full_prompt }),
        json!({ "role": "assistant", "content": "Understood. I will classify using the full conversation context." }),
        json!({ "role": "user", "content": new_message }),
    ]
}

// ── Response parsing (shared) ─────────────────────────────────────────────── //

#[derive(Deserialize)]
struct ClassifyResult {
    variant: String,
    order_id: Option<Uuid>,
}

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