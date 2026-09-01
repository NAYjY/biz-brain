//! Shared application state threaded through every axum handler.
//!
//! T01: holds both `ClaudeClassifier` and `GeminiClassifier` as
//! `Arc<dyn ClassifyClient>`. `classifier_for_branch()` reads the branch's
//! `ai_provider` column and returns the right one.
//!
//! T08: `login_limiter` added to AppState so `web::routes::login` can apply
//! the rate limiter without a global. Constructed once in `server/src/main.rs`
//! via `api::login_rate_limiter()` and stored here.
//!
//! `WorkerAgent` and `SupplierAgent` are constructed per-classify-call inside
//! inbox_worker (cheap — they hold only an Arc) rather than stored on AppState,
//! so provider switching takes effect immediately without a restart.

use std::collections::HashMap;
use std::sync::Arc;

use agent::{AiProvider, ClassifyClient, ClaudeClassifier, GeminiClassifier};
use domain::SseSignal;
use messaging::{LineAdapter, TelegramAdapter, WhatsAppAdapter};
use sqlx::PgPool;
use store::{
    ActorDirectory, BranchConfigRepository, EventSourcing, OrderEventRepository,
    ProjectionTables, ProjectionWorker, ReplyTemplateRepository,
    SupplyRequestEventRepository, WebhookInbox,
};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use axum::extract::FromRef;

use crate::rate_limit::KeyedLimiter;

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
    pub branch_config: Arc<BranchConfigRepository>,
    pub line: Arc<LineAdapter>,
    pub whatsapp: Arc<WhatsAppAdapter>,
    pub telegram: Arc<TelegramAdapter>,
    /// Both classifiers pre-built; chosen per-branch at classify time.
    pub claude_classifier: Arc<dyn ClassifyClient>,
    pub gemini_classifier: Arc<dyn ClassifyClient>,
    pub threads: Arc<Mutex<agent::ThreadContextStore>>,
    /// T07: one broadcast channel per Branch, lazily created.
    pub sse_branches: Arc<Mutex<HashMap<Uuid, broadcast::Sender<SseSignal>>>>,
    /// T08: shared login rate limiter — 5 req / 15 min per IP.
    /// Used by `web::routes::login::handle_login` via middleware.
    pub login_limiter: Arc<KeyedLimiter>,
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
        gemini_api_key: impl Into<String>,
        login_limiter: Arc<KeyedLimiter>,
    ) -> Self {
        let claude_api_key = claude_api_key.into();
        let gemini_api_key = gemini_api_key.into();

        Self {
            event_sourcing: Arc::new(EventSourcing::new(pool.clone())),
            order_events: Arc::new(OrderEventRepository::new(pool.clone())),
            supply_request_events: Arc::new(SupplyRequestEventRepository::new(pool.clone())),
            projections: Arc::new(ProjectionTables::new(pool.clone())),
            projection_worker: Arc::new(ProjectionWorker::new(pool.clone())),
            inbox: Arc::new(WebhookInbox::new(pool.clone())),
            actors: Arc::new(ActorDirectory::new(pool.clone())),
            reply_templates: Arc::new(ReplyTemplateRepository::new(pool.clone())),
            branch_config: Arc::new(BranchConfigRepository::new(pool.clone())),
            line: Arc::new(line),
            whatsapp: Arc::new(whatsapp),
            telegram: Arc::new(telegram),
            claude_classifier: Arc::new(ClaudeClassifier::new(claude_api_key)),
            gemini_classifier: Arc::new(GeminiClassifier::new(gemini_api_key)),
            threads: Arc::new(Mutex::new(agent::ThreadContextStore::new())),
            sse_branches: Arc::new(Mutex::new(HashMap::new())),
            login_limiter,
            pool,
        }
    }

    /// Returns the classifier configured for this branch.
    /// Falls back to Claude on any DB error.
    pub async fn classifier_for_branch(&self, branch_id: Uuid) -> Arc<dyn ClassifyClient> {
        let provider_str: String = self.branch_config.ai_provider(branch_id).await;
        match AiProvider::from_sql(&provider_str) {
            AiProvider::Gemini => Arc::clone(&self.gemini_classifier),
            AiProvider::Claude => Arc::clone(&self.claude_classifier),
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