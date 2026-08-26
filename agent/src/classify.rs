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

Your job:
1. Classify the worker's LAST message into one of [{allowed_variants}]
2. If the message clearly refers to a specific order (by name, description, or context), set order_id to that order's UUID
3. If only ONE active order exists, assume the message is about that order
4. If multiple orders exist and it's unclear which one, set order_id to null

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
    // Build recent history as plain text context, not role-play turns
    let history_text = if history.is_empty() {
        String::new()
    } else {
        let start = history.len().saturating_sub(5); // last 5 messages only
        let lines: Vec<String> = history[start..]
            .iter()
            .map(|m| {
                let role = if m.role == "user" { "Worker" } else { "Owner" };
                format!("{}: {}", role, m.content)
            })
            .collect();
        format!("\nRecent conversation:\n{}\n", lines.join("\n"))
    };

    let full_prompt = format!("{}{}\nNow classify this message:", preamble, history_text);

    // Single user turn with everything in it, one model reply
    vec![
        json!({ "role": "user", "parts": [{ "text": full_prompt }] }),
        json!({ "role": "model", "parts": [{ "text": "Understood." }] }),
        json!({ "role": "user", "parts": [{ "text": new_message }] }),
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