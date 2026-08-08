//! D01/D02: Owner persistence. S04: token_version for logout invalidation.

use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct OwnerRow {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub token_version: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct BranchRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
}

pub struct OwnerRepository {
    pool: PgPool,
}

impl OwnerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<OwnerRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, email, password_hash, token_version FROM owners WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
    }

    /// S04: verify token_version claim matches DB. Stale = rejected even if sig+exp valid.
    pub async fn verify_token_version(
        &self,
        owner_id: Uuid,
        claimed_version: i32,
    ) -> Result<bool, sqlx::Error> {
        let row: Option<(i32,)> =
            sqlx::query_as("SELECT token_version FROM owners WHERE id = $1")
                .bind(owner_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(v,)| v == claimed_version).unwrap_or(false))
    }

    /// S04: bump token_version on logout / password change -> invalidates all outstanding JWTs.
    pub async fn bump_token_version(&self, owner_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE owners SET token_version = token_version + 1 WHERE id = $1",
        )
        .bind(owner_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn branches_for_owner(&self, owner_id: Uuid) -> Result<Vec<BranchRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, owner_id, name FROM branches WHERE owner_id = $1 ORDER BY created_at ASC",
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
    }

    /// D02: create a new Branch for an Owner (in-dashboard flow, Branch #2+).
    pub async fn create_branch(&self, owner_id: Uuid, name: &str) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO branches (id, owner_id, name) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(owner_id)
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }
}