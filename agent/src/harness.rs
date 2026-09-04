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
    /// Tries to resolve using state/content/recency, or starts a
    /// disambiguation flow with a natural numbered question.
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
    /// Acknowledges their reply, extracts notes, narrows or confirms,
    /// or escalates after 3 turns.
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
                "  {}. name=\"{}\" state=\"{}\" last_active=\"{}\" description=\"{}\"",
                i + 1,
                o.display_name(),
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

ACTIVE ORDERS:
{order_list}

=== RESOLUTION RULES (apply in order, stop when resolved) ===

RULE 1 — STATE FILTER:
Remove orders where the detected event is impossible given state.
  "เสร็จแล้ว / done / เรียบร้อย" → requires state: ACCEPTED or READY_FOR_PICKUP
  "รับงาน / accept / โอเค" → requires state: ASSIGNED
  "ไม่ว่าง / unavailable / can't" → requires state: ASSIGNED
  "ยกเลิก / cancel" → requires state: ACCEPTED or ASSIGNED
If only ONE order remains after filter → RESOLVED (set resolved=true).

RULE 2 — CONTENT MATCH:
Look for specific keywords in the message that match order descriptions.
Equipment: แอร์/AC/air → air conditioning orders
Locations: ห้อง/ชั้น + number → match description with same room/floor
Materials: ท่อ/สาย/น้ำ/ไฟ → match plumbing/electrical/water orders
Strong content match → RESOLVED.
Weak or ambiguous match → do NOT resolve, fall through.

RULE 3 — RECENCY (weak signal only):
Note the most recently active order.
Do NOT resolve on recency alone.
Use only to inform the narrowed_to field if still ambiguous.

RULE 4 — EXTRACTION (always, regardless of resolution):
Always extract secondary information from the message.
Look for problems, complaints, materials used, observations.
Trigger words: "แต่...", "อีกอย่าง...", "ลูกค้า...", "ของหมด...", "เสีย...", "แตก..."
Extract as short Thai phrases.

=== IF NOT RESOLVED ===
Ask ONE natural Thai question listing ALL candidates as a numbered list.
Include the order description in parentheses so worker recognises their job.
Example: "งานที่เสร็จคืองานไหนครับ?\n1. AC-B3 (ซ่อม AC ห้อง 302)\n2. ท่อชั้น2 (เดินท่อ)\n3. งานสี (ทาสีห้อง)"
NEVER ask yes/no. NEVER use UUIDs. NEVER mention "order ID".

=== REPLY RULES ===
- reply is ALWAYS required — worker always gets a response.
- If extracted_notes is non-empty: acknowledge the note FIRST, then ask about order.
  Example: "รับทราบเรื่องน้ำยาหมดครับ ✓\nขอถามหน่อยนะครับ — งานที่เสร็จคืองานไหนครับ?..."
- If resolved: confirm clearly what was recorded.
  Example: "เยี่ยมเลยครับ ✓ บันทึกงาน AC-B3 เสร็จแล้ว"
- Keep replies short — workers read on mobile while on the job.

=== OUTPUT FORMAT ===
Respond with VALID JSON ONLY. No markdown. No explanation outside the JSON.

{{
  "resolved": <bool>,
  "order_id": "<uuid string or null>",
  "event_variant": "<one of: order_done, worker_accepted, worker_unavailable, worker_cancelled, clarification_requested, worker_ready_for_pickup, or null>",
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

    // Escalation instruction changes at turn 3
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
ORIGINAL MESSAGE (turn 1): "{}"
ORIGINAL INTENT DETECTED: {original_intent}

CANDIDATES (worker must declare one of these):
{candidate_list}

CURRENT STATE:
  turns_elapsed: {}
  narrowed_to: {narrowed_str}
  notes collected so far:
{notes_so_far}
  last question asked: "{last_q}"

=== RULES FOR THIS TURN ===

1. ACKNOWLEDGE first — always respond to what they just said naturally.
   Even if they ignored the question, acknowledge any new content.

2. EXTRACT new operational notes from their message.
   Add to new_notes_extracted (these will be appended to the running buffer).

3. TRY TO RESOLVE using their reply:
   Direct answer: "2", "อันแรก", "AC-B3", order name → RESOLVED, set resolved=true
   Indirect clue: "อันที่ทำเมื่อเช้า", "งาน AC" → narrow + confirm, set narrowed_to
   Unrelated update: extract notes, ask again with different phrasing

4. IF narrowed_to is already set (from previous turn):
   Ask confirmation for THAT specific order only.
   Example: "AC-B3 ใช่ไหมครับ? (ตอบ ใช่ หรือ ไม่ใช่ ได้เลย)"

5. REPHRASE — never repeat the exact same question as last_question.
   Vary the structure, not just the wording.

6. {escalation_rule}

=== OUTPUT FORMAT ===
Respond with VALID JSON ONLY. No markdown. No explanation outside the JSON.

{{
  "resolved": <bool>,
  "order_id": "<uuid or null — must be one of the candidate UUIDs if resolved>",
  "event_variant": "<original_intent variant or null>",
  "reply": "<Thai message to send to worker>",
  "new_notes_extracted": ["<Thai note>", ...],
  "narrowed_to": "<uuid or null>",
  "declaration_confirmed": <bool>,
  "escalate_to_owner": <bool>,
  "owner_escalation_summary": "<English summary of all notes + unresolved candidates, or null>",
  "confidence": <float 0.0-1.0>
}}"#,
        ctx.candidates
            .first()
            .map(|_| "")
            .unwrap_or(""),
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
        "parts": [{ "text": "Understood. I will follow these rules and respond with valid JSON only." }]
    }));

    // Last 10 history turns
    let start = history.len().saturating_sub(10);
    for turn in &history[start..] {
        let role = if turn.role == "assistant" { "model" } else { "user" };
        contents.push(json!({
            "role": role,
            "parts": [{ "text": turn.content }]
        }));
    }

    // Current message
    contents.push(json!({
        "role": "user",
        "parts": [{ "text": message }]
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

    Ok(HarnessOutput {
        resolved: j.resolved,
        order_id: j.order_id.as_deref().and_then(|s| s.parse().ok()),
        event_variant: j.event_variant.as_deref().and_then(parse_variant),
        reply: j.reply,
        extracted_notes: j.extracted_notes,
        owner_alert: j.owner_alert.map(|a| OwnerAlert {
            urgent: a.urgent,
            message: a.message,
        }),
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

    // If resolved, use the original_intent as the event variant
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

    // Build owner alert for escalation — includes all accumulated notes
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
        // Resolution with notes — flush to owner
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
        disambiguation_question: None, // continuation doesn't store this separately
        escalate_to_owner: j.escalate_to_owner,
        owner_escalation_summary: None, // already folded into owner_alert above
        confidence: j.confidence.clamp(0.0, 1.0),
    })
}

// ── Retry loop (same policy as existing classify.rs) ─────────────────────── //

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