//! Process configuration, read once from the environment at start-up.

use std::env::{self, VarError};
use std::time::Duration;

/// Everything the application needs from the environment.
///
/// Parsed once in `main` (and in the test harness) and then shared immutably
/// through [`crate::state::AppState`], so no handler ever touches `std::env`.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub bind_address: String,
    pub jwt_secret: String,
    /// Lifetime of an issued access token.
    pub jwt_ttl: Duration,
    /// How long a two-factor code stays valid. The assignment requires 5 minutes.
    pub two_factor_code_ttl: Duration,
    /// Wrong-code attempts allowed against a single challenge before it is locked.
    pub two_factor_max_attempts: i32,
    /// Per-user lifetime of a cached `view-my-tasks` payload.
    pub cache_ttl: Duration,
    /// Guards `GET /dev/email-logs/*`, which exposes verification codes.
    pub dev_endpoints_enabled: bool,
    pub seed: SeedConfig,
}

/// Default credentials created by `POST /seed/users`.
///
/// Kept in configuration rather than hard-coded so the validation flow is
/// reproducible from `.env.example` alone.
#[derive(Debug, Clone)]
pub struct SeedConfig {
    pub admin_full_name: String,
    pub admin_email: String,
    pub admin_password: String,
    pub staff_full_name: String,
    pub staff_email: String,
    pub staff_password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0} must be set")]
    Missing(&'static str),
    #[error("{key} is not valid: {source}")]
    Invalid {
        key: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("JWT_SECRET must be at least {MIN_JWT_SECRET_LEN} bytes long")]
    WeakJwtSecret,
}

/// Short secrets make HS256 signatures cheap to brute-force, so refuse to boot
/// with one rather than silently accepting it.
pub const MIN_JWT_SECRET_LEN: usize = 32;

impl AppConfig {
    /// Reads configuration from the process environment, applying the documented
    /// defaults for everything except `DATABASE_URL` and `JWT_SECRET`.
    pub fn from_env() -> Result<Self, ConfigError> {
        let jwt_secret = required("JWT_SECRET")?;
        if jwt_secret.len() < MIN_JWT_SECRET_LEN {
            return Err(ConfigError::WeakJwtSecret);
        }

        Ok(Self {
            database_url: required("DATABASE_URL")?,
            bind_address: optional("BIND_ADDRESS", "127.0.0.1:8080"),
            jwt_secret,
            jwt_ttl: Duration::from_secs(parsed("JWT_TTL_SECONDS", 3_600)?),
            two_factor_code_ttl: Duration::from_secs(parsed("TWO_FACTOR_CODE_TTL_SECONDS", 300)?),
            two_factor_max_attempts: parsed("TWO_FACTOR_MAX_ATTEMPTS", 5)?,
            // Deliberately generous: a reviewer stepping through the flow by hand
            // must still see `cache.hit = true` on the second call.
            cache_ttl: Duration::from_secs(parsed("CACHE_TTL_SECONDS", 300)?),
            dev_endpoints_enabled: parsed("ENABLE_DEV_ENDPOINTS", true)?,
            seed: SeedConfig {
                admin_full_name: optional("SEED_ADMIN_FULL_NAME", "Admin"),
                admin_email: optional("SEED_ADMIN_EMAIL", "admin@example.com"),
                admin_password: optional("SEED_ADMIN_PASSWORD", "AdminPass123!"),
                staff_full_name: optional("SEED_STAFF_FULL_NAME", "James Bond"),
                staff_email: optional("SEED_STAFF_EMAIL", "jamesbond@example.com"),
                staff_password: optional("SEED_STAFF_PASSWORD", "BondPass123!"),
            },
        })
    }
}

fn required(key: &'static str) -> Result<String, ConfigError> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) | Err(VarError::NotPresent) => Err(ConfigError::Missing(key)),
        Err(VarError::NotUnicode(_)) => Err(ConfigError::Invalid {
            key,
            source: "value is not valid UTF-8".into(),
        }),
    }
}

fn optional(key: &str, default: &str) -> String {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => default.to_owned(),
    }
}

fn parsed<T>(key: &'static str, default: T) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => {
            value.trim().parse().map_err(|source| ConfigError::Invalid {
                key,
                source: Box::new(source),
            })
        }
        _ => Ok(default),
    }
}
