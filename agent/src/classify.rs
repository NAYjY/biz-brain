//! T03: the one live Claude call per message, used only when the
//! keyword/regex pre-filter finds no match. One prompt per DomainEvent
//! *category* the sender role can produce (Worker events vs Supplier events),
//! not one prompt covering everything.

use serde::Deserialize;
use serde_json::json;

use crate::outcome::InterpretationError;
use domain::DomainEventVariant;

const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
const MODEL: &str = "claude-sonnet-4-6";

pub struct ClaudeClassifier {
    http: reqwest::Client,
    api_key: String,
}

#[derive(Deserialize)]
struct ClassifyResult {
    /// One of the DomainEventVariant SQL names, or "none" if ambiguous/unrecognized.
    variant: String,
}

impl ClaudeClassifier {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { http: reqwest::Client::new(), api_key: api_key.into() }
    }

    /// Classify a Worker message against the Worker-side event vocabulary.
    pub async fn classify_worker_message(&self, message: &str) -> Result<Option<DomainEventVariant>, InterpretationError> {
        self.classify(
            message,
            "worker_assigned, worker_accepted, worker_unavailable, worker_cancelled, \
             clarification_requested, worker_ready_for_pickup, order_done",
        )
        .await
    }

    /// Classify a Supplier message against the Supplier-side event vocabulary.
    pub async fn classify_supplier_message(&self, message: &str) -> Result<Option<DomainEventVariant>, InterpretationError> {
        self.classify(message, "invoice_received").await
    }

    async fn classify(&self, message: &str, allowed_variants: &str) -> Result<Option<DomainEventVariant>, InterpretationError> {
        let prompt = format!(
            "Classify this message into exactly one of [{allowed_variants}, none]. \
             Respond with ONLY JSON: {{\"variant\": \"<value>\"}}.\n\nMessage: {message}"
        );

        let response = self
            .http
            .post(CLAUDE_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": MODEL,
                "max_tokens": 100,
                "messages": [{ "role": "user", "content": prompt }],
            }))
            .send()
            .await
            .map_err(|e| InterpretationError::ClaudeApi(e.to_string()))?;

        let body: serde_json::Value =
            response.json().await.map_err(|e| InterpretationError::ClaudeApi(e.to_string()))?;

        let text = body["content"][0]["text"].as_str().ok_or_else(|| {
            InterpretationError::ParseFailed("no text block in Claude response".into())
        })?;

        let parsed: ClassifyResult =
            serde_json::from_str(text.trim()).map_err(|e| InterpretationError::ParseFailed(e.to_string()))?;

        Ok(sql_to_variant(&parsed.variant))
    }
}

fn sql_to_variant(s: &str) -> Option<DomainEventVariant> {
    Some(match s {
        "worker_assigned" => DomainEventVariant::WorkerAssigned,
        "worker_accepted" => DomainEventVariant::WorkerAccepted,
        "worker_unavailable" => DomainEventVariant::WorkerUnavailable,
        "worker_cancelled" => DomainEventVariant::WorkerCancelled,
        "clarification_requested" => DomainEventVariant::ClarificationRequested,
        "worker_ready_for_pickup" => DomainEventVariant::WorkerReadyForPickup,
        "order_done" => DomainEventVariant::OrderDone,
        "invoice_received" => DomainEventVariant::InvoiceReceived,
        _ => return None,
    })
}
