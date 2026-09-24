//! Outbound email.
//!
//! Real delivery is out of scope for this assignment, so the transport writes
//! each message to the `email_logs` table and emits a `tracing` line. Both are
//! readable locally: the table through `GET /dev/email-logs/latest`, the log line
//! on the server's stdout.
//!
//! Swapping in SMTP means adding a type with the same `send` signature (for
//! example using `lettre`) and changing the field type on
//! [`crate::state::AppState`]; nothing in the service layer would change.

use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::email::EmailLog;
use crate::error::AppResult;
use crate::repo;

/// A message to deliver.
#[derive(Debug, Clone)]
pub struct OutboundEmail {
    pub to: String,
    pub subject: String,
    pub body: String,
    /// Extracted separately from `body` so the development endpoint can return a
    /// code without the caller parsing prose out of the message.
    pub verification_code: Option<String>,
    pub login_challenge_id: Option<Uuid>,
}

impl OutboundEmail {
    /// Builds the two-factor code email.
    pub fn two_factor_code(
        to: &str,
        code: &str,
        challenge_id: Uuid,
        valid_for_minutes: i64,
    ) -> Self {
        Self {
            to: to.to_owned(),
            subject: "Your verification code".to_owned(),
            body: format!(
                "Your verification code is {code}. It expires in {valid_for_minutes} minute(s) \
                 and can only be used once. If you did not try to sign in, ignore this email."
            ),
            verification_code: Some(code.to_owned()),
            login_challenge_id: Some(challenge_id),
        }
    }
}

/// Development mail transport backed by the `email_logs` table.
#[derive(Debug, Clone)]
pub struct DevMailer {
    db: PgPool,
}

impl DevMailer {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// "Sends" a message by persisting it and logging that it was sent.
    ///
    /// The verification code is logged at INFO so the console alone is enough to
    /// complete the login flow; this is gated on the build being a local
    /// development one, which the README states as an explicit assumption.
    pub async fn send(&self, message: OutboundEmail) -> AppResult<EmailLog> {
        let stored = repo::email_logs::insert(&self.db, &message).await?;

        tracing::info!(
            to = %stored.to_email,
            subject = %stored.subject,
            verification_code = stored.verification_code.as_deref().unwrap_or("-"),
            "development mailer recorded an outbound email"
        );

        Ok(stored)
    }
}
