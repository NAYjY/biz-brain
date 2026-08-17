//! Shared application state threaded through every axum handler.
//!
//! P01: `reply_templates` added.

use std::collections::HashMap;
use std::sync::Arc;

use agent::{ClaudeClassifier, SupplierAgent, WorkerAgent};
use domain::SseSignal;
use messaging::{LineAdapter, TelegramAdapter, WhatsAppAdapter};
use sqlx::PgPool;
use store::{
    ActorDirectory, EventSourcing, OrderEventRepository, ProjectionTables, ProjectionWorker,
    ReplyTemplateRepository, SupplyRequestEventRepository, WebhookInbox,
};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use axum::extract::FromRef;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub event_sourcing: Arc<EventSourcing>,
    pub order_events: Arc<OrderEventRepository>,
    pub supply_request_events: Arc<SupplyRequestEventRepository>,
    pub projections: Arc<ProjectionTables>,
    pub projection_worker: Arc<ProjectionWorker>,
    pub inbox: Arc<WebhookInbox>,
    pub actors: Arc<ActorDirectory>,
    pub reply_templates: Arc<ReplyTemplateRepository>,
    pub line: Arc<LineAdapter>,
    pub whatsapp: Arc<WhatsAppAdapter>,
    pub telegram: Arc<TelegramAdapter>,
    pub worker_agent: Arc<WorkerAgent>,
    pub supplier_agent: Arc<SupplierAgent>,
    pub threads: Arc<Mutex<agent::ThreadContextStore>>,
    /// T07: one broadcast channel per Branch, lazily created.
    pub sse_branches: Arc<Mutex<HashMap<Uuid, broadcast::Sender<SseSignal>>>>,
}

impl FromRef<AppState> for PgPool {
    fn from_ref(state: &AppState) -> PgPool {
        state.pool.clone()
    }
}

impl AppState {
    pub fn new(
        pool: PgPool,
        line: LineAdapter,
        whatsapp: WhatsAppAdapter,
        telegram: TelegramAdapter,
        claude_api_key: impl Into<String>,
    ) -> Self {
        let claude_api_key = claude_api_key.into();
        Self {
            event_sourcing: Arc::new(EventSourcing::new(pool.clone())),
            order_events: Arc::new(OrderEventRepository::new(pool.clone())),
            supply_request_events: Arc::new(SupplyRequestEventRepository::new(pool.clone())),
            projections: Arc::new(ProjectionTables::new(pool.clone())),
            projection_worker: Arc::new(ProjectionWorker::new(pool.clone())),
            inbox: Arc::new(WebhookInbox::new(pool.clone())),
            actors: Arc::new(ActorDirectory::new(pool.clone())),
            reply_templates: Arc::new(ReplyTemplateRepository::new(pool.clone())),
            line: Arc::new(line),
            whatsapp: Arc::new(whatsapp),
            telegram: Arc::new(telegram),
            worker_agent: Arc::new(WorkerAgent::new(ClaudeClassifier::new(claude_api_key.clone()))),
            supplier_agent: Arc::new(SupplierAgent::new(ClaudeClassifier::new(claude_api_key))),
            threads: Arc::new(Mutex::new(agent::ThreadContextStore::new())),
            sse_branches: Arc::new(Mutex::new(HashMap::new())),
            pool,
        }
    }

    /// Publish a T07 invalidation signal. No-op if nobody is listening.
    pub async fn publish_sse(&self, signal: SseSignal) {
        let branches = self.sse_branches.lock().await;
        if let Some(tx) = branches.get(&signal.branch_id().into_inner()) {
            let _ = tx.send(signal);
        }
    }

    pub async fn sse_receiver(&self, branch_id: Uuid) -> broadcast::Receiver<SseSignal> {
        let mut branches = self.sse_branches.lock().await;
        branches
            .entry(branch_id)
            .or_insert_with(|| broadcast::channel(64).0)
            .subscribe()
    }
}
