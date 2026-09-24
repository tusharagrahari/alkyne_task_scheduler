//! HS256 access tokens.

use std::time::Duration;

use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::user::{Role, User};
use crate::error::{AppError, AppResult};

const ALGORITHM: Algorithm = Algorithm::HS256;

/// Access-token payload.
///
/// The role is carried in the token so authorising a request costs no database
/// round-trip. The trade-off is that a role change only takes effect once the
/// affected user logs in again; for a short-lived token that is an acceptable
/// staleness window, and it is called out in the README.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject — the user id.
    pub sub: Uuid,
    pub email: String,
    pub role: Role,
    /// Issued-at, seconds since the Unix epoch.
    pub iat: i64,
    /// Expiry, seconds since the Unix epoch.
    pub exp: i64,
}

/// A freshly minted token plus the metadata the login response reports.
#[derive(Debug, Clone)]
pub struct IssuedToken {
    pub token: String,
    pub expires_in_seconds: i64,
    pub expires_at: DateTime<Utc>,
}

/// Signs and verifies access tokens. Holds the derived keys so the HMAC key is
/// prepared once at start-up rather than per request.
pub struct JwtService {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
    ttl: Duration,
}

impl std::fmt::Debug for JwtService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the keys.
        f.debug_struct("JwtService")
            .field("ttl", &self.ttl)
            .finish()
    }
}

impl JwtService {
    pub fn new(secret: &str, ttl: Duration) -> Self {
        let mut validation = Validation::new(ALGORITHM);
        validation.validate_exp = true;
        validation.set_required_spec_claims(&["exp", "sub"]);
        // `jsonwebtoken` allows 60s of clock skew by default. This service issues
        // and verifies its own tokens against one clock, so expiry is enforced
        // exactly instead of approximately.
        validation.leeway = 0;

        Self {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
            validation,
            ttl,
        }
    }

    /// Issues a token for `user`, valid for the configured TTL.
    pub fn issue(&self, user: &User) -> AppResult<IssuedToken> {
        let issued_at = Utc::now();
        let ttl_seconds = i64::try_from(self.ttl.as_secs())
            .map_err(|_| AppError::internal("configured JWT TTL does not fit in i64"))?;
        let expires_at = issued_at + chrono::Duration::seconds(ttl_seconds);

        let claims = Claims {
            sub: user.id,
            email: user.email.clone(),
            role: user.role,
            iat: issued_at.timestamp(),
            exp: expires_at.timestamp(),
        };

        let token = jsonwebtoken::encode(&Header::new(ALGORITHM), &claims, &self.encoding)
            .map_err(|source| AppError::internal(format!("failed to sign token: {source}")))?;

        Ok(IssuedToken {
            token,
            expires_in_seconds: ttl_seconds,
            expires_at,
        })
    }

    /// Verifies signature, algorithm and expiry, returning the claims.
    ///
    /// Every failure collapses to one error so a caller cannot distinguish a bad
    /// signature from an expired token.
    pub fn verify(&self, token: &str) -> AppResult<Claims> {
        jsonwebtoken::decode::<Claims>(token, &self.decoding, &self.validation)
            .map(|data| data.claims)
            .map_err(|source| {
                tracing::debug!(error = %source, "rejected access token");
                AppError::InvalidAccessToken
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(role: Role) -> User {
        let now = Utc::now();
        User {
            id: Uuid::new_v4(),
            full_name: "Test User".into(),
            email: "test@example.com".into(),
            hashed_password: "unused".into(),
            role,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn round_trips_claims() {
        let service = JwtService::new("a-test-secret-that-is-long-enough", Duration::from_secs(60));
        let subject = user(Role::Admin);

        let issued = service.issue(&subject).expect("issue");
        let claims = service.verify(&issued.token).expect("verify");

        assert_eq!(claims.sub, subject.id);
        assert_eq!(claims.email, subject.email);
        assert_eq!(claims.role, Role::Admin);
        assert_eq!(issued.expires_in_seconds, 60);
    }

    #[test]
    fn rejects_token_signed_with_another_secret() {
        let issuer = JwtService::new("secret-number-one-long-enough-ok", Duration::from_secs(60));
        let verifier = JwtService::new("secret-number-two-long-enough-ok", Duration::from_secs(60));

        let issued = issuer.issue(&user(Role::Staff)).expect("issue");

        let error = verifier.verify(&issued.token).expect_err("must not verify");
        assert_eq!(error.code(), "invalid_access_token");
    }

    #[test]
    fn rejects_expired_token() {
        const SECRET: &str = "a-test-secret-that-is-long-enough";
        let service = JwtService::new(SECRET, Duration::from_secs(60));

        // Sign a token whose `exp` is an hour in the past. Built by hand because
        // `issue` can only mint tokens that are valid now.
        let subject = user(Role::Admin);
        let expired = Claims {
            sub: subject.id,
            email: subject.email,
            role: subject.role,
            iat: (Utc::now() - chrono::Duration::hours(2)).timestamp(),
            exp: (Utc::now() - chrono::Duration::hours(1)).timestamp(),
        };
        let token = jsonwebtoken::encode(
            &Header::new(ALGORITHM),
            &expired,
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .expect("encode");

        let error = service.verify(&token).expect_err("must not verify");
        assert_eq!(error.code(), "invalid_access_token");
    }
}
