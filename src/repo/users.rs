use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::user::{Role, User, normalize_email};

const USER_COLUMNS: &str = "id, full_name, email, hashed_password, role, created_at, updated_at";

/// Inserts a user, or updates the existing row with the same email.
///
/// `POST /seed/users` is expected to be re-runnable — a reviewer will call it
/// more than once — so this upserts instead of failing on the unique index. The
/// password is rewritten too, which makes the documented seed credentials valid
/// again after any manual change.
pub async fn upsert(
    db: &PgPool,
    full_name: &str,
    email: &str,
    hashed_password: &str,
    role: Role,
) -> Result<User, sqlx::Error> {
    let sql = format!(
        "INSERT INTO users (id, full_name, email, hashed_password, role)
         VALUES ($1, $2, $3, $4, $5::user_role)
         ON CONFLICT (email) DO UPDATE
            SET full_name       = EXCLUDED.full_name,
                hashed_password = EXCLUDED.hashed_password,
                role            = EXCLUDED.role,
                updated_at      = now()
         RETURNING {USER_COLUMNS}"
    );

    sqlx::query_as::<_, User>(&sql)
        .bind(Uuid::new_v4())
        .bind(full_name)
        .bind(normalize_email(email))
        .bind(hashed_password)
        .bind(role)
        .fetch_one(db)
        .await
}

pub async fn find_by_email(db: &PgPool, email: &str) -> Result<Option<User>, sqlx::Error> {
    let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE email = $1");

    sqlx::query_as::<_, User>(&sql)
        .bind(normalize_email(email))
        .fetch_optional(db)
        .await
}

pub async fn find_by_id(db: &PgPool, id: Uuid) -> Result<Option<User>, sqlx::Error> {
    let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE id = $1");

    sqlx::query_as::<_, User>(&sql)
        .bind(id)
        .fetch_optional(db)
        .await
}
