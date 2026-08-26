//! T06 / P08 / T01: single binary, single Axum Router, single deploy artifact.
//!
//! T01: reads GEMINI_API_KEY (optional — falls back to empty string, which
//! causes Gemini calls to fail gracefully if a branch is misconfigured but
//! the key isn't set).

use api::AppState;
use messaging::{LineAdapter, TelegramAdapter, WhatsAppAdapter};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let db_url = std::env::var("DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&db_url).await?;
    sqlx::migrate!("../store/migrations").run(&pool).await?;

    // ── Adapters ──────────────────────────────────────────────────── //

    let line = LineAdapter::new(
        std::env::var("LINE_CHANNEL_SECRET")?,
        std::env::var("LINE_CHANNEL_ACCESS_TOKEN")?,
    );
    let whatsapp = WhatsAppAdapter::new(
        std::env::var("WHATSAPP_VERIFY_TOKEN")?,
        std::env::var("WHATSAPP_ACCESS_TOKEN")?,
        std::env::var("WHATSAPP_PHONE_NUMBER_ID")?,
    );
    let telegram = TelegramAdapter::new(
        std::env::var("TELEGRAM_SECRET_TOKEN")?,
        std::env::var("TELEGRAM_BOT_TOKEN")?,
    );

    let claude_api_key = std::env::var("ANTHROPIC_API_KEY")?;
    // T01: Gemini key is optional — empty string causes graceful failure
    // on Gemini-configured branches if the key isn't provided.
    let gemini_api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();

    // ── P11: register Telegram webhook on startup if URL provided ─── //
    if let Ok(webhook_url) = std::env::var("TELEGRAM_WEBHOOK_URL") {
        tracing::info!("Registering Telegram webhook at {webhook_url}");
        if let Err(e) = telegram.set_webhook(&webhook_url).await {
            tracing::warn!("Telegram webhook registration failed: {e}");
        }
    }

    let state = AppState::new(pool, line, whatsapp, telegram, claude_api_key, gemini_api_key);

    tokio::spawn(api::inbox_worker::run(state.clone()));
    tokio::spawn(api::alert_worker::run(state.clone()));

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "web/static".to_string());

    let app = api::build_router()
        .merge(web::build_router())
        .nest_service("/static", ServeDir::new(&static_dir))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let bind_addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("Biz-Brain listening on http://{bind_addr}");
    axum::serve(listener, app).await?;

    Ok(())
}