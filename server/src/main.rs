//! T06 / P08 / T01 / T08 / T05b / T05c: single binary, single Axum Router, single deploy artifact.
//!
//! T05b: `--migrate-only` flag — runs migrations then exits.
//!       Used by the K8s pre-upgrade migration Job so migrations run before
//!       the new Deployment rolls out. Failed migration = Job fails = Helm
//!       aborts = old pod keeps serving = no downtime.
//!
//! T05c: `--seed-owner` flag — creates the first Owner account + Branch
//!       inside a freshly provisioned namespace. Called by provision.sh via
//!       `kubectl exec`. Runs migrations first so it is safe to run on an
//!       empty DB. Idempotent: uses ON CONFLICT DO UPDATE on email.
//!
//! Normal startup (no flags): runs migrations + starts HTTP server.

use api::{login_rate_limiter, AppState};
use messaging::{LineAdapter, TelegramAdapter, WhatsAppAdapter};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();

    // ── T05b: migration-only mode ──────────────────────────────────────── //
    // Called by the Helm pre-upgrade Job:
    //   biz-brain-server --migrate-only
    if args.iter().any(|a| a == "--migrate-only") {
        let db_url = std::env::var("DATABASE_URL")?;
        let pool = sqlx::PgPool::connect(&db_url).await?;
        sqlx::migrate!("../store/migrations").run(&pool).await?;
        println!("Migrations complete.");
        return Ok(());
    }

    // ── T05c: seed-owner mode ──────────────────────────────────────────── //
    // Called by provision.sh via kubectl exec:
    //   biz-brain-server --seed-owner --email owner@example.com \
    //                    --password s3cur3 --branch "Acme Corp"
    if args.iter().any(|a| a == "--seed-owner") {
        return run_seed_owner(&args).await;
    }

    // ── Normal server startup ──────────────────────────────────────────── //

    let db_url = std::env::var("DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&db_url).await?;
    sqlx::migrate!("../store/migrations").run(&pool).await?;

    // ── Adapters ──────────────────────────────────────────────────────── //

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
    let gemini_api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();

    // ── T08: rate limiters ─────────────────────────────────────────────── //
    let login_lim = login_rate_limiter();

    // ── P11: register Telegram webhook on startup if URL provided ─────── //
    if let Ok(webhook_url) = std::env::var("TELEGRAM_WEBHOOK_URL") {
        tracing::info!("Registering Telegram webhook at {webhook_url}");
        if let Err(e) = telegram.set_webhook(&webhook_url).await {
            tracing::warn!("Telegram webhook registration failed: {e}");
        }
    }

    let state = AppState::new(
        pool,
        line,
        whatsapp,
        telegram,
        claude_api_key,
        gemini_api_key,
        login_lim,
    );

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
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}

// ── T05c: --seed-owner implementation ─────────────────────────────────────── //

/// Parse `--key value` pairs from args. Simple, no external dep needed.
fn parse_flag(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].clone())
}

async fn run_seed_owner(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let email = parse_flag(args, "--email")
        .ok_or("--email required for --seed-owner")?;
    let password = parse_flag(args, "--password")
        .ok_or("--password required for --seed-owner")?;
    let branch_name = parse_flag(args, "--branch")
        .ok_or("--branch required for --seed-owner")?;

    if password.len() < 8 {
        return Err("--password must be at least 8 characters".into());
    }

    let db_url = std::env::var("DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&db_url).await?;

    // Always run migrations first — safe to call on already-migrated DB.
    sqlx::migrate!("../store/migrations").run(&pool).await?;

    let hash = bcrypt::hash(&password, 12)?;

    // Upsert owner — idempotent. Re-running provision.sh is safe.
    let owner_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, role, token_version) \
         VALUES ($1, $2, 'owner', 0) \
         ON CONFLICT (email) DO UPDATE \
           SET password_hash = EXCLUDED.password_hash \
         RETURNING id",
    )
    .bind(&email)
    .bind(&hash)
    .fetch_one(&pool)
    .await?;

    // Upsert branch — idempotent by (created_by_user_id, name).
    let branch_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO branches (created_by_user_id, name) \
         VALUES ($1, $2) \
         ON CONFLICT DO NOTHING \
         RETURNING id",
    )
    .bind(owner_id)
    .bind(&branch_name)
    .fetch_optional(&pool)
    .await?
    .unwrap_or_else(|| {
        // Branch already exists — fetch its id.
        // We handle this synchronously below.
        uuid::Uuid::nil()
    });

    // If branch already existed (nil returned from optional), fetch it.
    let branch_id = if branch_id == uuid::Uuid::nil() {
        sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT id FROM branches WHERE created_by_user_id = $1 AND name = $2",
        )
        .bind(owner_id)
        .bind(&branch_name)
        .fetch_one(&pool)
        .await?
    } else {
        branch_id
    };

    // Seed default reply templates for this branch.
    let template_repo = store::ReplyTemplateRepository::new(pool);
    template_repo.seed_defaults(branch_id).await?;

    println!("✓ Owner:    {} (id: {})", email, owner_id);
    println!("✓ Branch:   {} (id: {})", branch_name, branch_id);
    println!("✓ Templates seeded.");
    println!("Done.");

    Ok(())
}