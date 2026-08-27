//! D01 / D02 / S04 / T20: User (Owner + Manager) persistence.
//! T20: `owners` table renamed to `users`; `role` column added.
//!      `branches.owner_id` renamed to `created_by_user_id`.

use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub role: String,
    pub token_version: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct BranchRow {
    pub id: Uuid,
    pub created_by_user_id: Uuid,
    pub name: String,
}

pub struct OwnerRepository {
    pool: PgPool,
}

impl OwnerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<UserRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, email, password_hash, role, token_version FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
    }

    /// S04: verify token_version claim matches DB.
    pub async fn verify_token_version(
        &self,
        user_id: Uuid,
        claimed_version: i32,
    ) -> Result<bool, sqlx::Error> {
        let row: Option<(i32,)> =
            sqlx::query_as("SELECT token_version FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(v,)| v == claimed_version).unwrap_or(false))
    }

    /// S04: bump token_version on logout / password change.
    pub async fn bump_token_version(&self, user_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE users SET token_version = token_version + 1 WHERE id = $1",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// T20: Owners see ALL branches; Managers see only their granted ones.
    /// Callers pass the role so this repo doesn't re-query it.
    pub async fn branches_for_user(
        &self,
        user_id: Uuid,
        role: &str,
    ) -> Result<Vec<BranchRow>, sqlx::Error> {
        if role == "owner" {
            sqlx::query_as(
                "SELECT id, created_by_user_id, name FROM branches ORDER BY created_at ASC",
            )
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as(
                "SELECT b.id, b.created_by_user_id, b.name \
                 FROM branches b \
                 JOIN user_branch_access uba ON uba.branch_id = b.id \
                 WHERE uba.user_id = $1 \
                 ORDER BY uba.granted_at ASC",
            )
            .bind(user_id)
            .fetch_all(&self.pool)
            .await
        }
    }

    /// D02: create a new Branch (any Owner can create branches).
    pub async fn create_branch(
        &self,
        created_by_user_id: Uuid,
        name: &str,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO branches (id, created_by_user_id, name) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(created_by_user_id)
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    // ── T20: Manager management (Owner-only actions) ──────────────── //

    /// Create a Manager account. Called by Owner via the API.
    pub async fn create_manager(
        &self,
        email: &str,
        password_hash: &str,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, role) VALUES ($1, $2, $3, 'manager')",
        )
        .bind(id)
        .bind(email)
        .bind(password_hash)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// Grant a Manager access to a branch.
    pub async fn grant_branch_access(
        &self,
        manager_id: Uuid,
        branch_id: Uuid,
        granted_by: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO user_branch_access (user_id, branch_id, granted_by) \
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(manager_id)
        .bind(branch_id)
        .bind(granted_by)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Revoke a Manager's access to a branch.
    /// Also bumps their token_version so existing JWTs are invalidated.
    pub async fn revoke_branch_access(
        &self,
        manager_id: Uuid,
        branch_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM user_branch_access WHERE user_id = $1 AND branch_id = $2",
        )
        .bind(manager_id)
        .bind(branch_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() > 0 {
            // Force re-login so the Manager's JWT refreshes without the revoked branch.
            self.bump_token_version(manager_id).await?;
        }

        Ok(result.rows_affected() > 0)
    }

    /// List all Managers (for the Owner management UI).
    pub async fn list_managers(&self) -> Result<Vec<UserRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, email, password_hash, role, token_version \
             FROM users WHERE role = 'manager' ORDER BY email ASC",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// List Managers for a specific branch.
    pub async fn managers_for_branch(&self, branch_id: Uuid) -> Result<Vec<UserRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT u.id, u.email, u.password_hash, u.role, u.token_version \
             FROM users u \
             JOIN user_branch_access uba ON uba.user_id = u.id \
             WHERE uba.branch_id = $1 AND u.role = 'manager' \
             ORDER BY u.email ASC",
        )
        .bind(branch_id)
        .fetch_all(&self.pool)
        .await
    }
}
