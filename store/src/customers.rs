//! D04: Customer read/write. Minimal — no full profile, no history.

use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct CustomerRow {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub name: String,
}

pub struct CustomerRepository {
    pool: PgPool,
}

impl CustomerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list_by_branch(&self, branch_id: Uuid) -> Result<Vec<CustomerRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, branch_id, name FROM customers WHERE branch_id = $1 ORDER BY name ASC",
        )
        .bind(branch_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn create(&self, branch_id: Uuid, name: &str) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO customers (id, branch_id, name) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(branch_id)
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }
}