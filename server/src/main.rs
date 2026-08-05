//! T06: single binary, single Axum Router, single deploy artifact. Merges
//! `api`'s routes (REST + commands + webhooks + same-process SSE source)
//! with `web`'s routes (SSR shell + browser-facing SSE relay per T07), then
//! attaches the one shared `AppState`.

use api::AppState;
use messaging::{LineAdapter, WhatsAppAdapter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok(); // .env is optional — fine if the vars are set another way (e.g. Railway)
    tracing_subscriber::fmt::init();

    let db_url = std::env::var("DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&db_url).await?;
    sqlx::migrate!("../store/migrations").run(&pool).await?;

    let line = LineAdapter::new(std::env::var("LINE_CHANNEL_SECRET")?, std::env::var("LINE_CHANNEL_ACCESS_TOKEN")?);
    let whatsapp = WhatsAppAdapter::new(
        std::env::var("WHATSAPP_VERIFY_TOKEN")?,
        std::env::var("WHATSAPP_ACCESS_TOKEN")?,
        std::env::var("WHATSAPP_PHONE_NUMBER_ID")?,
    );
    let claude_api_key = std::env::var("ANTHROPIC_API_KEY")?;

    let state = AppState::new(pool, line, whatsapp, claude_api_key);

    // T04: the only place `agent` is invoked from — never the webhook handler.
    tokio::spawn(api::inbox_worker::run(state.clone()));

    let app = api::build_router().merge(web::build_router()).with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("Biz-Brain listening on http://0.0.0.0:8080");
    axum::serve(listener, app).await?;

    Ok(())
}
