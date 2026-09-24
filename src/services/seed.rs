//! Creates the two users the validation workflow needs.
//!
//! Exposed as an endpoint rather than a separate binary so the whole assignment
//! can be validated with an HTTP client and nothing else.

use crate::domain::user::{Role, User};
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::security::password;
use crate::state::AppState;

const MIN_PASSWORD_LEN: usize = 8;

/// Optional overrides for the seeded accounts; anything omitted falls back to the
/// configured defaults from `.env`.
#[derive(Debug, Clone, Default)]
pub struct SeedOverrides {
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub admin_full_name: Option<String>,
    pub staff_email: Option<String>,
    pub staff_password: Option<String>,
    pub staff_full_name: Option<String>,
}

/// The seeded pair, admin first.
#[derive(Debug, Clone)]
pub struct SeededUsers {
    pub admin: User,
    pub staff: User,
}

/// Creates (or refreshes) the admin and staff accounts.
///
/// Idempotent: calling it repeatedly leaves exactly two users and resets their
/// passwords to the documented values, so a reviewer can re-run it at any point
/// in the flow without hitting a unique-constraint error.
pub async fn seed_users(state: &AppState, overrides: SeedOverrides) -> AppResult<SeededUsers> {
    let defaults = &state.config.seed;

    let admin_email = pick(overrides.admin_email, &defaults.admin_email);
    let admin_password = pick(overrides.admin_password, &defaults.admin_password);
    let admin_full_name = pick(overrides.admin_full_name, &defaults.admin_full_name);
    let staff_email = pick(overrides.staff_email, &defaults.staff_email);
    let staff_password = pick(overrides.staff_password, &defaults.staff_password);
    let staff_full_name = pick(overrides.staff_full_name, &defaults.staff_full_name);

    validate_email(&admin_email)?;
    validate_email(&staff_email)?;
    validate_password(&admin_password)?;
    validate_password(&staff_password)?;

    if admin_email.trim().eq_ignore_ascii_case(staff_email.trim()) {
        return Err(AppError::validation(
            "the admin and staff accounts must use different email addresses",
        ));
    }

    let admin = repo::users::upsert(
        &state.db,
        &admin_full_name,
        &admin_email,
        &password::hash_secret_blocking(admin_password).await?,
        Role::Admin,
    )
    .await?;

    let staff = repo::users::upsert(
        &state.db,
        &staff_full_name,
        &staff_email,
        &password::hash_secret_blocking(staff_password).await?,
        Role::Staff,
    )
    .await?;

    tracing::info!(admin = %admin.email, staff = %staff.email, "seeded users");
    Ok(SeededUsers { admin, staff })
}

fn pick(override_value: Option<String>, default: &str) -> String {
    override_value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_owned())
}

/// Structural check only — enough to catch a typo, without pretending to
/// validate deliverability.
fn validate_email(email: &str) -> AppResult<()> {
    let email = email.trim();
    let valid = match email.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        None => false,
    };

    if valid {
        Ok(())
    } else {
        Err(AppError::validation(format!(
            "`{email}` is not a valid email address"
        )))
    }
}

fn validate_password(password: &str) -> AppResult<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AppError::validation(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plausible_addresses() {
        for email in ["admin@example.com", "james.bond@mi6.co.uk", "a@b.io"] {
            assert!(validate_email(email).is_ok(), "{email} should be valid");
        }
    }

    #[test]
    fn rejects_malformed_addresses() {
        for email in [
            "",
            "no-at-sign",
            "@example.com",
            "user@nodot",
            "user@.com",
            "user@com.",
        ] {
            assert!(validate_email(email).is_err(), "{email} should be rejected");
        }
    }

    #[test]
    fn rejects_short_passwords() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("longenough").is_ok());
    }

    #[test]
    fn overrides_fall_back_to_defaults_when_blank() {
        assert_eq!(pick(None, "default@example.com"), "default@example.com");
        assert_eq!(
            pick(Some("  ".into()), "default@example.com"),
            "default@example.com"
        );
        assert_eq!(
            pick(Some(" given@example.com ".into()), "default@example.com"),
            "given@example.com"
        );
    }
}
