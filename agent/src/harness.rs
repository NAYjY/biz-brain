//! Gemini harness — bidirectional, context-aware message processing.
//!
//! Two entry points:
//!   harness_fresh()        — first message, no disambiguation pending
//!   harness_continuation() — active disambiguation flow, worker replied
//!
//! Both return HarnessOutput. The caller (inbox_worker) routes the output:
//!   reply          → send to worker immediately (always)
//!   event_variant  → emit to event stream if resolved
//!   extracted_notes → accumulate in disambiguation state or flush to owner alert
//!   escalate       → set ⚠️ badge on dashboard
//!
//! Retry policy: same as existing classify.rs — 8s timeout, 3 attempts,
//! 500ms/1500ms backoff, retry on 429/5xx/timeout only.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

use crate::outcome::{HarnessOutput, InterpretationError, OwnerAlert};
use domain::DomainEventVariant;

// ── Constants ─────────────────────────────────────────────────────────────── //

const GEMINI_MODEL: &str = "gemini-3.5-flash-lite";
const GEMINI_API_BASE: &str =
    "https://generativelanguage.googleapis.com/v1beta/models";
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_DELAYS: [Duration; 2] = [
    Duration::from_millis(500),
    Duration::from_millis(1500),
];
const MAX_ATTEMPTS: usize = 3;

// ── Public context types ───────────────────────────────────────────────────── //

/// One active order passed to the harness as context.
#[derive(Debug, Clone)]
pub struct OrderContext {
    pub order_id: Uuid,
    pub short_name: Option<String>,
    pub description: String,
    pub state: String,
    /// ISO-8601 string of the last event — used for recency signal.
    pub last_event_at: Option<String>,
}

impl OrderContext {
    /// Display name: short_name if set, else first 40 chars of description.
    pub fn display_name(&self) -> String {
        match &self.short_name {
            Some(n) if !n.trim().is_empty() => n.clone(),
            _ => {
                let s: String = self.description.chars().take(40).collect();
                if self.description.chars().count() > 40 {
                    format!("{s}…")
                } else {
                    s
                }
            }
        }
    }
}

/// A single conversation turn passed as history.
#[derive(Debug, Clone)]
pub struct HistoryTurn {
    pub role: String,   // "user" | "assistant"
    pub content: String,
}

/// Current disambiguation state — read from disambiguation_pending row.
#[derive(Debug, Clone)]
pub struct DisambiguationContext {
    pub candidates: Vec<OrderContext>,
    pub original_intent: Option<String>,
    pub turns_elapsed: i32,
    pub extracted_notes_so_far: Vec<String>,
    pub last_question: Option<String>,
    pub narrowed_to: Option<Uuid>,
}

// ── Gemini client ─────────────────────────────────────────────────────────── //

pub struct GeminiHarness {
    http: reqwest::Client,
    api_key: String,
}

impl GeminiHarness {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
        }
    }

    /// First message from worker — no disambiguation pending.
    pub async fn harness_fresh(
        &self,
        worker_name: &str,
        message: &str,
        history: &[HistoryTurn],
        active_orders: &[OrderContext],
    ) -> Result<HarnessOutput, InterpretationError> {
        let system = build_fresh_system(worker_name, active_orders);
        let contents = build_contents(&system, history, message);
        let raw = self.call_gemini(contents).await?;
        parse_fresh_output(&raw)
    }

    /// Worker replied during an active disambiguation flow.
    pub async fn harness_continuation(
        &self,
        worker_name: &str,
        message: &str,
        history: &[HistoryTurn],
        ctx: &DisambiguationContext,
    ) -> Result<HarnessOutput, InterpretationError> {
        let system = build_continuation_system(worker_name, ctx);
        let contents = build_contents(&system, history, message);
        let raw = self.call_gemini(contents).await?;
        parse_continuation_output(&raw, ctx)
    }

    async fn call_gemini(&self, contents: Value) -> Result<Value, InterpretationError> {
        let body = json!({
            "contents": contents,
            "generationConfig": {
                "maxOutputTokens": 1000,
                "temperature": 0.1,
                "responseMimeType": "application/json"
            }
        });

        let url = format!(
            "{GEMINI_API_BASE}/{GEMINI_MODEL}:generateContent?key={}",
            self.api_key
        );

        with_retry(|| {
            let req = self.http.post(&url).json(&body);
            async move {
                let resp = req
                    .send()
                    .await
                    .map_err(|e| {
                        InterpretationError::ClaudeApi(format!("Gemini network: {e}"))
                            .mark_transient()
                    })?;

                let status = resp.status();
                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    let err = InterpretationError::ClaudeApi(
                        format!("Gemini HTTP {status}: {text}"),
                    );
                    return Err(if status.as_u16() == 429 || status.is_server_error() {
                        err.mark_transient()
                    } else {
                        err
                    });
                }

                resp.json::<Value>().await.map_err(|e| {
                    InterpretationError::ClaudeApi(format!("Gemini parse: {e}"))
                })
            }
        })
        .await
    }
}

// ── Prompt builders ───────────────────────────────────────────────────────── //

fn build_fresh_system(worker_name: &str, orders: &[OrderContext]) -> String {
    let order_list = orders
        .iter()
        .enumerate()
        .map(|(i, o)| {
            format!(
                "  {}. name=\"{}\" id=\"{}\" state=\"{}\" last_active=\"{}\" description=\"{}\"",
                i + 1,
                o.display_name(),
                o.order_id,
                o.state,
                o.last_event_at.as_deref().unwrap_or("unknown"),
                o.description,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"You are the operations assistant for a field service business.
You communicate with field workers via LINE/Telegram in Thai.
You are helpful, warm, and professional. Workers are busy — be concise.

WORKER: {worker_name}

ACTIVE ORDERS (each has a number, name, uuid, state, description):
{order_list}

=== STATE FILTER — APPLY FIRST, MANDATORY ===

Match the worker's message to an event type, then remove orders where that event is impossible.

EVENT → VALID STATES:
  order_done              → ACCEPTED or READY_FOR_PICKUP only
  worker_accepted         → ASSIGNED only
  worker_unavailable      → ASSIGNED only
  worker_cancelled        → ACCEPTED or ASSIGNED only
  worker_ready_for_pickup → ACCEPTED only
  clarification_requested → ASSIGNED or ACCEPTED only

After removing invalid-state orders:
  - 0 remain → set resolved=false, event_variant=null, reply apologising you cannot match
  - 1 remains → set resolved=true immediately with that order's uuid. DO NOT ASK.
  - 2+ remain → proceed to content matching below

=== CONTENT MATCHING (only when 2+ remain after state filter) ===

Look for keywords in the worker's message matching order descriptions.
Equipment: แอร์/AC/air → air conditioning; ท่อ → pipe/plumbing; ไฟ → electrical/wiring; สี → paint
Locations: ห้อง/ชั้น + number → match room/floor in description
Strong unique match → resolved=true with that order's uuid.
Ambiguous → proceed to ask.

=== RECENCY (weak signal only) ===

Note the most recently active order. Use only for narrowed_to, NOT to auto-resolve.

=== NOTE EXTRACTION (always, regardless of resolution) ===

Always extract secondary information from the message.
Problems, complaints, materials used, observations.
Triggers: "แต่...", "อีกอย่าง...", "ลูกค้า...", "ของหมด...", "เสีย...", "แตก..."
Extract as short Thai phrases.

=== WHEN ASKING FOR DISAMBIGUATION ===

List ONLY the remaining candidates after state filtering (not all active orders).
Number them 1, 2, 3... starting from 1 with NO gaps.
Use order display name + short description in parentheses.
Example: "งานที่หมายถึงคืองานไหนครับ?\n1. AC-B3 (ซ่อม AC ห้อง 302)\n2. ท่อชั้น2 (เดินท่อ)"
NEVER ask yes/no on first turn. NEVER use UUIDs. NEVER say "order ID".

=== REPLY RULES ===

reply is ALWAYS required — worker always gets a response.
If notes extracted: acknowledge the note FIRST, then handle order.
  Example: "รับทราบเรื่องน้ำยาหมดครับ ✓\nขอถามหน่อยนะครับ — งานที่หมายถึงคืออะไรครับ?..."
If resolved: confirm clearly.
  Example: "เยี่ยมเลยครับ ✓ บันทึกงาน AC-B3 รับงานเรียบร้อยแล้ว"
Keep replies short — workers read on mobile.

=== OUTPUT FORMAT ===
Respond with VALID JSON ONLY. No markdown. No explanation outside the JSON.

CRITICAL RULES FOR JSON OUTPUT:
- When resolved=true: order_id MUST be a uuid string (never null), event_variant MUST be a string (never null)
- When resolved=false: order_id and event_variant should be null
- event_variant MUST be one of the exact strings listed below — pick the one matching the detected event type

{{
  "resolved": <bool>,
  "order_id": "<uuid string matching one of the order ids above — REQUIRED when resolved=true>",
  "event_variant": "<REQUIRED when resolved=true: exactly one of: order_done, worker_accepted, worker_unavailable, worker_cancelled, clarification_requested, worker_ready_for_pickup>",
  "reply": "<Thai message to send to worker>",
  "extracted_notes": ["<Thai note>", ...],
  "owner_alert": null or {{"urgent": <bool>, "message": "<English summary for owner>"}},
  "needs_disambiguation": <bool>,
  "narrowed_to": "<uuid of most likely candidate or null>",
  "disambiguation_question": "<Thai numbered question or null>",
  "confidence": <float 0.0-1.0>
}}"#
    )
}

fn build_continuation_system(worker_name: &str, ctx: &DisambiguationContext) -> String {
    let candidate_list = ctx
        .candidates
        .iter()
        .enumerate()
        .map(|(i, o)| {
            format!(
                "  {}. name=\"{}\" id=\"{}\" description=\"{}\"",
                i + 1,
                o.display_name(),
                o.order_id,
                o.description,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let notes_so_far = if ctx.extracted_notes_so_far.is_empty() {
        "  (none yet)".to_string()
    } else {
        ctx.extracted_notes_so_far
            .iter()
            .map(|n| format!("  • {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let narrowed_name = ctx.narrowed_to.and_then(|id| {
        ctx.candidates
            .iter()
            .find(|o| o.order_id == id)
            .map(|o| o.display_name())
    });

    let narrowed_str = narrowed_name
        .as_deref()
        .unwrap_or("none — still ambiguous between all candidates");

    let last_q = ctx
        .last_question
        .as_deref()
        .unwrap_or("(none recorded)");

    let original_intent = ctx
        .original_intent
        .as_deref()
        .unwrap_or("unknown");

    // Build candidate uuid mapping string for easy reference
    let uuid_map = ctx
        .candidates
        .iter()
        .enumerate()
        .map(|(i, o)| format!("  {} -> id={}", i + 1, o.order_id))
        .collect::<Vec<_>>()
        .join("\n");

    let escalation_rule = if ctx.turns_elapsed >= 3 {
        r#"ESCALATION: turns_elapsed has reached 3.
Set escalate_to_owner=true.
Set reply to: "โอเคครับ ได้แจ้งเจ้าของให้ช่วยดูให้ครับ"
Set owner_escalation_summary with all accumulated notes and candidate names."#
    } else {
        &format!(
            "After this turn, turns_elapsed will be {}. At 3 the owner is notified.",
            ctx.turns_elapsed + 1
        )
    };

    format!(
        r#"You are managing an active order declaration conversation.
Worker has not yet declared which order they are referring to.

WORKER: {worker_name}
ORIGINAL MESSAGE (turn 1): (stored internally)
ORIGINAL INTENT DETECTED: {original_intent}

CANDIDATES (worker must pick one):
{candidate_list}

CANDIDATE NUMBER → UUID MAPPING:
{uuid_map}

CURRENT STATE:
  turns_elapsed: {}
  narrowed_to: {narrowed_str}
  notes collected so far:
{notes_so_far}
  last question asked: "{last_q}"

=== CRITICAL NUMBER PARSING RULE ===

When worker sends a single digit or number, it selects a candidate by position number.
"1" = candidate 1 (first in list above)
"2" = candidate 2 (second in list above)
"3" = candidate 3 (third in list above)
ONLY look at the worker's CURRENT message. IGNORE all numbers from previous bot messages.
If worker says "1", order_id MUST be the uuid of candidate 1.
If worker says "2", order_id MUST be the uuid of candidate 2.
A number from worker = RESOLVED immediately. Set resolved=true.

=== RULES FOR THIS TURN ===

1. ACKNOWLEDGE first — respond naturally to what they said.

2. EXTRACT new operational notes from their message.

3. CHECK FOR DIRECT SELECTION:
   - Worker says a number (1, 2, 3...) → RESOLVED with that candidate's uuid
   - Worker says the order name or display name → RESOLVED with that order's uuid
   - Worker says ใช่/yes/correct when narrowed_to is set → RESOLVED with narrowed_to uuid
   - Worker says ไม่/no → NOT that order, remove it from candidates in your thinking

4. CHECK FOR INDIRECT CLUE (only if no direct selection):
   - Partial name, description keyword, location → narrow to one candidate
   - Set narrowed_to to that uuid, ask confirmation for ONLY that order

5. IF narrowed_to already set from previous turn:
   Ask confirmation for THAT specific order only.
   Example: "AC-B3 ใช่ไหมครับ? (ตอบ ใช่ หรือ ไม่ใช่)"

6. REPHRASE — never repeat the exact wording of last_question.

7. {escalation_rule}

=== OUTPUT FORMAT ===
Respond with VALID JSON ONLY. No markdown. No explanation outside the JSON.

CRITICAL RULES FOR JSON OUTPUT:
- When resolved=true: order_id MUST be the uuid from the CANDIDATE NUMBER → UUID MAPPING (never null), event_variant MUST match original_intent (never null)
- event_variant when resolved = same as original_intent (e.g. if original_intent is "worker_accepted" then event_variant = "worker_accepted")

{{
  "resolved": <bool>,
  "order_id": "<uuid from CANDIDATE NUMBER → UUID MAPPING — REQUIRED when resolved=true, null otherwise>",
  "event_variant": "<REQUIRED when resolved=true: use original_intent value, e.g. worker_accepted, order_done, etc. null when not resolved>",
  "reply": "<Thai message to send to worker>",
  "new_notes_extracted": ["<Thai note>", ...],
  "narrowed_to": "<uuid or null>",
  "declaration_confirmed": <bool>,
  "escalate_to_owner": <bool>,
  "owner_escalation_summary": "<English summary of all notes + unresolved candidates, or null>",
  "confidence": <float 0.0-1.0>
}}"#,
        ctx.turns_elapsed,
    )
}

fn build_contents(system: &str, history: &[HistoryTurn], message: &str) -> Value {
    let mut contents: Vec<Value> = Vec::new();

    // System as first user turn + model ack (Gemini pattern)
    contents.push(json!({
        "role": "user",
        "parts": [{ "text": system }]
    }));
    contents.push(json!({
        "role": "model",
        "parts": [{ "text": "Understood. I will follow these rules exactly and respond with valid JSON only." }]
    }));

    // Last 10 history turns — label clearly to avoid number confusion
    let start = history.len().saturating_sub(10);
    for turn in &history[start..] {
        let role = if turn.role == "assistant" { "model" } else { "user" };
        contents.push(json!({
            "role": role,
            "parts": [{ "text": turn.content }]
        }));
    }

    // Current message — mark explicitly
    contents.push(json!({
        "role": "user",
        "parts": [{ "text": format!("[CURRENT MESSAGE TO CLASSIFY]: {}", message) }]
    }));

    json!(contents)
}

// ── Output parsers ────────────────────────────────────────────────────────── //

#[derive(Deserialize)]
struct FreshJson {
    resolved: bool,
    order_id: Option<String>,
    event_variant: Option<String>,
    reply: String,
    #[serde(default)]
    extracted_notes: Vec<String>,
    owner_alert: Option<AlertJson>,
    needs_disambiguation: bool,
    narrowed_to: Option<String>,
    disambiguation_question: Option<String>,
    confidence: f32,
}

#[derive(Deserialize)]
struct ContinuationJson {
    resolved: bool,
    order_id: Option<String>,
    event_variant: Option<String>,
    reply: String,
    #[serde(default)]
    new_notes_extracted: Vec<String>,
    narrowed_to: Option<String>,
    declaration_confirmed: bool,
    escalate_to_owner: bool,
    owner_escalation_summary: Option<String>,
    confidence: f32,
}

#[derive(Deserialize)]
struct AlertJson {
    urgent: bool,
    message: String,
}

fn extract_text(raw: &Value) -> Result<&str, InterpretationError> {
    raw["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or_else(|| {
            InterpretationError::ParseFailed(format!(
                "unexpected Gemini response shape: {raw}"
            ))
        })
}

fn clean_json(text: &str) -> &str {
    text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

fn parse_variant(s: &str) -> Option<DomainEventVariant> {
    Some(match s {
        "order_done"               => DomainEventVariant::OrderDone,
        "worker_accepted"          => DomainEventVariant::WorkerAccepted,
        "worker_unavailable"       => DomainEventVariant::WorkerUnavailable,
        "worker_cancelled"         => DomainEventVariant::WorkerCancelled,
        "clarification_requested"  => DomainEventVariant::ClarificationRequested,
        "worker_ready_for_pickup"  => DomainEventVariant::WorkerReadyForPickup,
        "invoice_received"         => DomainEventVariant::InvoiceReceived,
        "supplier_confirmed"       => DomainEventVariant::SupplierConfirmed,
        _ => return None,
    })
}

fn parse_fresh_output(raw: &Value) -> Result<HarnessOutput, InterpretationError> {
    let text = extract_text(raw)?;
    let j: FreshJson = serde_json::from_str(clean_json(text)).map_err(|e| {
        InterpretationError::ParseFailed(format!(
            "harness_fresh JSON parse: {e} — raw: {text}"
        ))
    })?;

    if j.reply.trim().is_empty() {
        return Err(InterpretationError::ParseFailed(
            "harness returned empty reply".into(),
        ));
    }

    // Build owner_alert: prefer Gemini's explicit alert, then synthesise from
    // extracted_notes when resolved (so owner sees notes in thread/log).
    let owner_alert = j.owner_alert
        .map(|a| OwnerAlert { urgent: a.urgent, message: a.message })
        .or_else(|| {
            if j.resolved && !j.extracted_notes.is_empty() {
                Some(OwnerAlert {
                    urgent: false,
                    message: format!("Worker notes: {}", j.extracted_notes.join(" | ")),
                })
            } else {
                None
            }
        });

    Ok(HarnessOutput {
        resolved: j.resolved,
        order_id: j.order_id.as_deref().and_then(|s| s.parse().ok()),
        event_variant: j.event_variant.as_deref().and_then(parse_variant),
        reply: j.reply,
        extracted_notes: j.extracted_notes,
        owner_alert,
        needs_disambiguation: j.needs_disambiguation,
        narrowed_to: j.narrowed_to.as_deref().and_then(|s| s.parse().ok()),
        disambiguation_question: j.disambiguation_question,
        escalate_to_owner: false,
        owner_escalation_summary: None,
        confidence: j.confidence.clamp(0.0, 1.0),
    })
}

fn parse_continuation_output(
    raw: &Value,
    ctx: &DisambiguationContext,
) -> Result<HarnessOutput, InterpretationError> {
    let text = extract_text(raw)?;
    let j: ContinuationJson = serde_json::from_str(clean_json(text)).map_err(|e| {
        InterpretationError::ParseFailed(format!(
            "harness_continuation JSON parse: {e} — raw: {text}"
        ))
    })?;

    if j.reply.trim().is_empty() {
        return Err(InterpretationError::ParseFailed(
            "continuation harness returned empty reply".into(),
        ));
    }

    // If resolved, use event_variant from JSON or fall back to original_intent
    let event_variant = if j.resolved {
        j.event_variant
            .as_deref()
            .and_then(parse_variant)
            .or_else(|| {
                ctx.original_intent
                    .as_deref()
                    .and_then(parse_variant)
            })
    } else {
        None
    };

    // Build owner alert for escalation
    let owner_alert = if j.escalate_to_owner {
        let all_notes: Vec<String> = ctx
            .extracted_notes_so_far
            .iter()
            .cloned()
            .chain(j.new_notes_extracted.iter().cloned())
            .collect();

        let candidate_names: Vec<String> = ctx
            .candidates
            .iter()
            .map(|o| o.display_name())
            .collect();

        let summary = j.owner_escalation_summary.unwrap_or_else(|| {
            format!(
                "Worker did not declare which order after {} attempts. Candidates: {}. Notes: {}",
                ctx.turns_elapsed + 1,
                candidate_names.join(", "),
                if all_notes.is_empty() {
                    "none".to_string()
                } else {
                    all_notes.join(", ")
                }
            )
        });

        Some(OwnerAlert {
            urgent: false,
            message: summary,
        })
    } else if j.resolved && !j.new_notes_extracted.is_empty() {
        let all_notes: Vec<String> = ctx
            .extracted_notes_so_far
            .iter()
            .cloned()
            .chain(j.new_notes_extracted.iter().cloned())
            .collect();

        Some(OwnerAlert {
            urgent: false,
            message: format!(
                "Worker notes on order: {}",
                all_notes.join(" | ")
            ),
        })
    } else {
        None
    };

    Ok(HarnessOutput {
        resolved: j.resolved,
        order_id: j.order_id.as_deref().and_then(|s| s.parse().ok()),
        event_variant,
        reply: j.reply,
        extracted_notes: j.new_notes_extracted,
        owner_alert,
        needs_disambiguation: !j.resolved && !j.escalate_to_owner,
        narrowed_to: j.narrowed_to.as_deref().and_then(|s| s.parse().ok()),
        disambiguation_question: None,
        escalate_to_owner: j.escalate_to_owner,
        owner_escalation_summary: None,
        confidence: j.confidence.clamp(0.0, 1.0),
    })
}

// ── Retry loop ────────────────────────────────────────────────────────────── //

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
            Err(_) => {
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