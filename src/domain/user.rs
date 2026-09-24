use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Authorisation role. The `snake_case` renames on both derives keep the JSON
/// value (`"admin"` / `"staff"`) identical to the PostgreSQL `user_role` labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "user_role", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Staff,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Staff => "staff",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A user row. Intentionally **not** `Serialize`: `hashed_password` must never
/// reach a response body. Handlers convert to a DTO instead.
#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: Uuid,
    pub full_name: String,
    pub email: String,
    pub hashed_password: String,
    pub role: Role,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Normalises an email for storage and lookup so `Admin@Example.com` and
/// `admin@example.com` resolve to the same account.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}
