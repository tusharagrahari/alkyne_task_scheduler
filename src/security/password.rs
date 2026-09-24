//! Argon2id hashing for both account passwords and two-factor codes.
//!
//! The same primitive is used for both on purpose. A six-digit code has only
//! 10^6 possible values, so a fast hash (SHA-256 and friends) would be trivially
//! reversible from a database dump; a memory-hard KDF keeps the stored code
//! useless to anyone who reads the `login_challenges` table.

use actix_web::web;
use argon2::password_hash::Error as PasswordHashError;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use crate::error::{AppError, AppResult};

/// Hashes `secret` with Argon2id using the crate's recommended parameters and a
/// fresh random salt, returning a PHC string (`$argon2id$v=19$m=...`).
pub fn hash_secret(secret: &str) -> AppResult<String> {
    Argon2::default()
        .hash_password(secret.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|source| AppError::internal(format!("failed to hash secret: {source}")))
}

/// Constant-time comparison of `secret` against a stored PHC string.
///
/// A wrong secret is `Ok(false)`, not an error — only a malformed stored hash or
/// a KDF failure is exceptional.
pub fn verify_secret(secret: &str, phc_hash: &str) -> AppResult<bool> {
    let expected = PasswordHash::new(phc_hash).map_err(|source| {
        AppError::internal(format!("stored hash is not a valid PHC string: {source}"))
    })?;

    match Argon2::default().verify_password(secret.as_bytes(), &expected) {
        Ok(()) => Ok(true),
        Err(PasswordHashError::PasswordInvalid) => Ok(false),
        Err(source) => Err(AppError::internal(format!(
            "failed to verify secret: {source}"
        ))),
    }
}

// Argon2 is deliberately CPU- and memory-hard: a single call costs tens of
// milliseconds. Running that directly inside an async handler would block a
// worker thread and stall every other request it is driving, so the two helpers
// below move the work onto Actix's blocking thread pool.

pub async fn hash_secret_blocking(secret: String) -> AppResult<String> {
    web::block(move || hash_secret(&secret))
        .await
        .map_err(|source| AppError::internal(format!("hashing task failed: {source}")))?
}

pub async fn verify_secret_blocking(secret: String, phc_hash: String) -> AppResult<bool> {
    web::block(move || verify_secret(&secret, &phc_hash))
        .await
        .map_err(|source| AppError::internal(format!("verification task failed: {source}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_is_salted_and_verifies_round_trip() {
        let first = hash_secret("correct horse battery staple").expect("hash");
        let second = hash_secret("correct horse battery staple").expect("hash");

        // Distinct salts mean identical inputs must not produce identical hashes.
        assert_ne!(first, second);
        assert!(first.starts_with("$argon2id$"));

        assert!(verify_secret("correct horse battery staple", &first).expect("verify"));
        assert!(!verify_secret("wrong password", &first).expect("verify"));
    }

    #[test]
    fn malformed_stored_hash_is_an_internal_error() {
        let error = verify_secret("whatever", "not-a-phc-string").expect_err("should fail");
        assert_eq!(error.code(), "internal_error");
    }
}
