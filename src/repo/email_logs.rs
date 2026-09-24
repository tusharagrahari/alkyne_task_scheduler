use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::email::EmailLog;
use crate::domain::user::normalize_email;
use crate::mail::OutboundEmail;

const EMAIL_LOG_COLUMNS: &str =
    "id, to_email, subject, body, verification_code, login_challenge_id, created_at";

pub async fn insert(db: &PgPool, message: &OutboundEmail) -> Result<EmailLog, sqlx::Error> {
    let sql = format!(
        "INSERT INTO email_logs
             (id, to_email, subject, body, verification_code, login_challenge_id)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING {EMAIL_LOG_COLUMNS}"
    );

    sqlx::query_as::<_, EmailLog>(&sql)
        .bind(Uuid::new_v4())
        .bind(normalize_email(&message.to))
        .bind(&message.subject)
        .bind(&message.body)
        .bind(message.verification_code.as_deref())
        .bind(message.login_challenge_id)
        .fetch_one(db)
        .await
}

/// Most recent message, optionally narrowed to one recipient.
///
/// The recipient filter matters once both users have logged in: without it the
/// "latest" email is whichever user acted last.
pub async fn find_latest(
    db: &PgPool,
    recipient: Option<&str>,
) -> Result<Option<EmailLog>, sqlx::Error> {
    let sql = format!(
        "SELECT {EMAIL_LOG_COLUMNS}
           FROM email_logs
          WHERE ($1::text IS NULL OR to_email = $1)
          ORDER BY created_at DESC, id DESC
          LIMIT 1"
    );

    sqlx::query_as::<_, EmailLog>(&sql)
        .bind(recipient.map(normalize_email))
        .fetch_optional(db)
        .await
}

pub async fn list_recent(
    db: &PgPool,
    recipient: Option<&str>,
    limit: i64,
) -> Result<Vec<EmailLog>, sqlx::Error> {
    let sql = format!(
        "SELECT {EMAIL_LOG_COLUMNS}
           FROM email_logs
          WHERE ($1::text IS NULL OR to_email = $1)
          ORDER BY created_at DESC, id DESC
          LIMIT $2"
    );

    sqlx::query_as::<_, EmailLog>(&sql)
        .bind(recipient.map(normalize_email))
        .bind(limit)
        .fetch_all(db)
        .await
}
