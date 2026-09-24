use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

/// A pending email two-factor login attempt.
///
/// `code_hash` is an Argon2id PHC string — the six-digit code is never stored in
/// plain text here. Single use is enforced by `consumed_at`, expiry by
/// `expires_at`, and brute-force resistance by `attempts`.
#[derive(Debug, Clone, FromRow)]
pub struct LoginChallenge {
    pub id: Uuid,
    pub user_id: Uuid,
    pub code_hash: String,
    pub attempts: i32,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl LoginChallenge {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }

    pub fn is_consumed(&self) -> bool {
        self.consumed_at.is_some()
    }
}
