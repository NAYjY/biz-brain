//! T06: single binary, single Axum Router, single deploy artifact.
//! Merges api + web routes, attaches shared AppState, serves static assets.

use api::AppState;
use messaging::{LineAdapter, WhatsAppAdapter};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let db_url = std::env::var("DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&db_url).await?;
    sqlx::migrate!("../store/migrations").run(&pool).await?;

    let line = LineAdapter::new(
        std::env::var("LINE_CHANNEL_SECRET")?,
        std::env::var("LINE_CHANNEL_ACCESS_TOKEN")?,
    );
    let whatsapp = WhatsAppAdapter::new(
        std::env::var("WHATSAPP_VERIFY_TOKEN")?,
        std::env::var("WHATSAPP_ACCESS_TOKEN")?,
        std::env::var("WHATSAPP_PHONE_NUMBER_ID")?,
    );
    let claude_api_key = std::env::var("ANTHROPIC_API_KEY")?;

    let state = AppState::new(pool, line, whatsapp, claude_api_key);

    tokio::spawn(api::inbox_worker::run(state.clone()));

    let app = api::build_router()
        .merge(web::build_router())
        // D07: serve CSS/JS static assets from web/static/
        .nest_service("/static", ServeDir::new("web/static"))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("Biz-Brain listening on http://0.0.0.0:8080");
    axum::serve(listener, app).await?;

    Ok(())
}