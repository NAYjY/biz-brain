//! D08-7: seed script. Creates one Owner + one Branch in a single pass.
//! Usage: cargo run --bin seed -- <email> <password> <branch_name>
//! Reads DATABASE_URL from env (or .env via dotenvy).

use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: seed <email> <password> <branch_name>");
        std::process::exit(1);
    }
    let email = &args[1];
    let password = &args[2];
    let branch_name = &args[3];

    let db_url = env::var("DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&db_url).await?;
    sqlx::migrate!("../../store/migrations").run(&pool).await?;

    // D08-7: hash password with bcrypt (cost 12 — matches prod security posture)
    let hash = bcrypt::hash(password, 12)?;

    let owner_id = sqlx::query_scalar::<_, uuid::Uuid>(
        "INSERT INTO owners (email, password_hash, token_version) \
         VALUES ($1, $2, 0) \
         ON CONFLICT (email) DO UPDATE SET password_hash = EXCLUDED.password_hash \
         RETURNING id",
    )
    .bind(email)
    .bind(&hash)
    .fetch_one(&pool)
    .await?;

    let branch_id = sqlx::query_scalar::<_, uuid::Uuid>(
        "INSERT INTO branches (owner_id, name) VALUES ($1, $2) RETURNING id",
    )
    .bind(owner_id)
    .bind(branch_name)
    .fetch_one(&pool)
    .await?;

    println!("✓ Owner:  {} (id: {})", email, owner_id);
    println!("✓ Branch: {} (id: {})", branch_name, branch_id);
    println!("Done. Log in at /login with the provided credentials.");

    Ok(())
}