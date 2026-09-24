use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::challenge::LoginChallenge;

const CHALLENGE_COLUMNS: &str =
    "id, user_id, code_hash, attempts, expires_at, consumed_at, created_at";

pub async fn insert(
    db: &PgPool,
    user_id: Uuid,
    code_hash: &str,
    expires_at: DateTime<Utc>,
) -> Result<LoginChallenge, sqlx::Error> {
    let sql = format!(
        "INSERT INTO login_challenges (id, user_id, code_hash, expires_at)
         VALUES ($1, $2, $3, $4)
         RETURNING {CHALLENGE_COLUMNS}"
    );

    sqlx::query_as::<_, LoginChallenge>(&sql)
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(code_hash)
        .bind(expires_at)
        .fetch_one(db)
        .await
}

pub async fn find_by_id(db: &PgPool, id: Uuid) -> Result<Option<LoginChallenge>, sqlx::Error> {
    let sql = format!("SELECT {CHALLENGE_COLUMNS} FROM login_challenges WHERE id = $1");

    sqlx::query_as::<_, LoginChallenge>(&sql)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Records a failed verification attempt and returns the new attempt count.
pub async fn record_failed_attempt(db: &PgPool, id: Uuid) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar::<_, i32>(
        "UPDATE login_challenges
            SET attempts = attempts + 1
          WHERE id = $1
          RETURNING attempts",
    )
    .bind(id)
    .fetch_one(db)
    .await
}

/// Atomically marks a challenge as used.
///
/// `WHERE consumed_at IS NULL` makes single-use enforcement a property of the
/// database rather than of a read-then-write sequence in the service: two
/// requests racing with the same valid code cannot both be issued a token,
/// because only one `UPDATE` matches a row. `Ok(false)` means the challenge had
/// already been consumed.
pub async fn try_consume(db: &PgPool, id: Uuid, now: DateTime<Utc>) -> Result<bool, sqlx::Error> {
    let consumed = sqlx::query_scalar::<_, Uuid>(
        "UPDATE login_challenges
            SET consumed_at = $2
          WHERE id = $1
            AND consumed_at IS NULL
          RETURNING id",
    )
    .bind(id)
    .bind(now)
    .fetch_optional(db)
    .await?;

    Ok(consumed.is_some())
}
