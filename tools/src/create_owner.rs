//! T13 / T20: create-owner CLI bin.
//! Creates an Owner account in the `users` table.
//! Must be run after the DB is seeded with an Admin (cargo run --bin seed).
//!
//! Usage: cargo run --bin create-owner -- <email> <password>

use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: create-owner <email> <password>");
        std::process::exit(1);
    }
    let email = &args[1];
    let password = &args[2];

    let db_url = env::var("DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&db_url).await?;
    sqlx::migrate!("../store/migrations").run(&pool).await?;

    let hash = bcrypt::hash(password, 12)?;

    let owner_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, role) \
         VALUES ($1, $2, 'owner') \
         ON CONFLICT (email) DO UPDATE SET password_hash = EXCLUDED.password_hash \
         RETURNING id",
    )
    .bind(email)
    .bind(&hash)
    .fetch_one(&pool)
    .await?;

    println!("✓ Owner: {} (id: {})", email, owner_id);
    println!("  Log in at /login");
    println!("  Owners see all branches; create Managers from the branch settings page.");

    Ok(())
}
