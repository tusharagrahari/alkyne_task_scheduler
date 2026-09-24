use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

/// A message the application "sent". In this local build the mail transport is
/// the `email_logs` table plus a `tracing` line, which is what
/// `GET /dev/email-logs/latest` reads back.
#[derive(Debug, Clone, FromRow)]
pub struct EmailLog {
    pub id: Uuid,
    pub to_email: String,
    pub subject: String,
    pub body: String,
    pub verification_code: Option<String>,
    pub login_challenge_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}
