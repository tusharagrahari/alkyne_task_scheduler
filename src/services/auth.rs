//! Email two-factor login.
//!
//! Step one validates the password and creates a challenge — deliberately *not*
//! a token. Step two exchanges a correct, unexpired, unused code for a JWT.

use chrono::{DateTime, Utc};
use rand::RngExt;
use uuid::Uuid;

use crate::domain::challenge::LoginChallenge;
use crate::domain::user::{User, normalize_email};
use crate::error::{AppError, AppResult};
use crate::mail::OutboundEmail;
use crate::repo;
use crate::security::{IssuedToken, password};
use crate::state::AppState;

/// Number of digits in a verification code.
const CODE_DIGITS: u32 = 6;

/// Outcome of step one.
#[derive(Debug, Clone)]
pub struct StartedLogin {
    pub challenge_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub email: String,
}

/// Outcome of step two.
#[derive(Debug, Clone)]
pub struct CompletedLogin {
    pub token: IssuedToken,
    pub user: User,
}

/// Validates credentials, opens a two-factor challenge and sends the code.
///
/// Returns [`AppError::InvalidCredentials`] for both an unknown email and a wrong
/// password: distinguishing them would let a caller enumerate registered
/// accounts.
pub async fn start_login(
    state: &AppState,
    email: &str,
    password_input: &str,
) -> AppResult<StartedLogin> {
    let email = normalize_email(email);

    let user = repo::users::find_by_email(&state.db, &email)
        .await?
        .ok_or(AppError::InvalidCredentials)?;

    let password_matches =
        password::verify_secret_blocking(password_input.to_owned(), user.hashed_password.clone())
            .await?;
    if !password_matches {
        return Err(AppError::InvalidCredentials);
    }

    let code = generate_code();
    let code_hash = password::hash_secret_blocking(code.clone()).await?;

    let ttl = chrono::Duration::from_std(state.config.two_factor_code_ttl)
        .map_err(|_| AppError::internal("configured two-factor TTL is out of range"))?;
    let expires_at = Utc::now() + ttl;

    let challenge = repo::challenges::insert(&state.db, user.id, &code_hash, expires_at).await?;

    state
        .mailer
        .send(OutboundEmail::two_factor_code(
            &user.email,
            &code,
            challenge.id,
            ttl.num_minutes().max(1),
        ))
        .await?;

    tracing::info!(
        user_id = %user.id,
        challenge_id = %challenge.id,
        "opened two-factor challenge"
    );

    Ok(StartedLogin {
        challenge_id: challenge.id,
        expires_at: challenge.expires_at,
        email: user.email,
    })
}

/// Exchanges a verification code for an access token.
///
/// Checks run in the order used/expired/rate-limited/incorrect so the caller gets
/// the most specific reason available, and the code is only compared once the
/// challenge is known to still be usable.
pub async fn verify_two_factor(
    state: &AppState,
    challenge_id: Uuid,
    code: &str,
) -> AppResult<CompletedLogin> {
    let challenge = repo::challenges::find_by_id(&state.db, challenge_id)
        .await?
        .ok_or(AppError::not_found("login challenge"))?;

    let now = Utc::now();
    ensure_challenge_usable(&challenge, now, state.config.two_factor_max_attempts)?;

    let code_matches =
        password::verify_secret_blocking(code.trim().to_owned(), challenge.code_hash.clone())
            .await?;
    if !code_matches {
        let attempts = repo::challenges::record_failed_attempt(&state.db, challenge.id).await?;
        tracing::warn!(
            challenge_id = %challenge.id,
            attempts,
            "incorrect two-factor code"
        );
        return Err(AppError::InvalidVerificationCode);
    }

    // Single use is enforced by the database, not by the read above: two requests
    // arriving with the same valid code race here, and only one `UPDATE` wins.
    if !repo::challenges::try_consume(&state.db, challenge.id, now).await? {
        return Err(AppError::VerificationCodeAlreadyUsed);
    }

    let user = repo::users::find_by_id(&state.db, challenge.user_id)
        .await?
        .ok_or(AppError::not_found("user"))?;

    let token = state.jwt.issue(&user)?;
    tracing::info!(user_id = %user.id, role = %user.role, "issued access token");

    Ok(CompletedLogin { token, user })
}

fn ensure_challenge_usable(
    challenge: &LoginChallenge,
    now: DateTime<Utc>,
    max_attempts: i32,
) -> AppResult<()> {
    if challenge.is_consumed() {
        return Err(AppError::VerificationCodeAlreadyUsed);
    }
    if challenge.is_expired(now) {
        return Err(AppError::VerificationCodeExpired);
    }
    if challenge.attempts >= max_attempts {
        return Err(AppError::TooManyVerificationAttempts);
    }
    Ok(())
}

/// Six digits from the OS CSPRNG, zero-padded so every code is the same length.
fn generate_code() -> String {
    let upper_bound = 10_u32.pow(CODE_DIGITS);
    let value = rand::rng().random_range(0..upper_bound);
    format!("{value:0width$}", width = CODE_DIGITS as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_codes_are_six_digits() {
        for _ in 0..500 {
            let code = generate_code();
            assert_eq!(code.len(), 6, "code {code} has the wrong length");
            assert!(
                code.chars().all(|c| c.is_ascii_digit()),
                "code {code} is not numeric"
            );
        }
    }

    fn challenge(attempts: i32, expires_in_secs: i64, consumed: bool) -> LoginChallenge {
        let now = Utc::now();
        LoginChallenge {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            code_hash: "unused".into(),
            attempts,
            expires_at: now + chrono::Duration::seconds(expires_in_secs),
            consumed_at: consumed.then_some(now),
            created_at: now,
        }
    }

    #[test]
    fn usable_challenge_passes() {
        assert!(ensure_challenge_usable(&challenge(0, 300, false), Utc::now(), 5).is_ok());
    }

    #[test]
    fn consumed_challenge_is_rejected_before_expiry_is_considered() {
        // Both consumed *and* expired: the reused-code error is the more useful one.
        let error = ensure_challenge_usable(&challenge(0, -10, true), Utc::now(), 5)
            .expect_err("must reject");
        assert_eq!(error.code(), "verification_code_already_used");
    }

    #[test]
    fn expired_challenge_is_rejected() {
        let error = ensure_challenge_usable(&challenge(0, -1, false), Utc::now(), 5)
            .expect_err("must reject");
        assert_eq!(error.code(), "verification_code_expired");
    }

    #[test]
    fn challenge_over_attempt_limit_is_rejected() {
        let error = ensure_challenge_usable(&challenge(5, 300, false), Utc::now(), 5)
            .expect_err("must reject");
        assert_eq!(error.code(), "too_many_verification_attempts");
    }
}
