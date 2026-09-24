//! The single error type every handler returns.
//!
//! Each variant owns its HTTP status and its stable machine-readable `code`, so
//! the status a client sees is decided in one place instead of at each call
//! site. Clients can branch on `error.code`; humans read `error.message`.

use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;

use crate::domain::user::Role;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),

    #[error("invalid email or password")]
    InvalidCredentials,

    #[error("missing or malformed `Authorization: Bearer <token>` header")]
    MissingBearerToken,

    #[error("access token is invalid or has expired")]
    InvalidAccessToken,

    /// The caller authenticated successfully but lacks the required role.
    /// Distinct from the 401 variants above: this is the 403 the assignment
    /// requires when James Bond tries to create a task.
    #[error("this action requires the `{required}` role")]
    InsufficientRole { required: Role },

    #[error("verification code is incorrect")]
    InvalidVerificationCode,

    #[error("verification code has expired")]
    VerificationCodeExpired,

    #[error("verification code has already been used")]
    VerificationCodeAlreadyUsed,

    #[error("too many incorrect attempts for this login challenge")]
    TooManyVerificationAttempts,

    #[error("{entity} not found")]
    NotFound { entity: &'static str },

    #[error("{0}")]
    Conflict(String),

    #[error("development-only endpoint is disabled (set ENABLE_DEV_ENDPOINTS=true)")]
    DevEndpointDisabled,

    /// Database failures are logged in full but reported generically: a SQL
    /// string or constraint name in a response body is an information leak.
    #[error("database error")]
    Database(#[from] sqlx::Error),

    #[error("internal error")]
    Internal(String),
}

impl AppError {
    /// Stable identifier clients may match on. Kept separate from the
    /// human-readable `Display` text, which is free to change.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "validation_error",
            Self::InvalidCredentials => "invalid_credentials",
            Self::MissingBearerToken => "missing_bearer_token",
            Self::InvalidAccessToken => "invalid_access_token",
            Self::InsufficientRole { .. } => "insufficient_role",
            Self::InvalidVerificationCode => "invalid_verification_code",
            Self::VerificationCodeExpired => "verification_code_expired",
            Self::VerificationCodeAlreadyUsed => "verification_code_already_used",
            Self::TooManyVerificationAttempts => "too_many_verification_attempts",
            Self::NotFound { .. } => "not_found",
            Self::Conflict(_) => "conflict",
            Self::DevEndpointDisabled => "dev_endpoint_disabled",
            Self::Database(_) | Self::Internal(_) => "internal_error",
        }
    }

    pub fn not_found(entity: &'static str) -> Self {
        Self::NotFound { entity }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::InvalidCredentials
            | Self::MissingBearerToken
            | Self::InvalidAccessToken
            | Self::InvalidVerificationCode
            | Self::VerificationCodeExpired
            | Self::VerificationCodeAlreadyUsed => StatusCode::UNAUTHORIZED,
            Self::InsufficientRole { .. } | Self::DevEndpointDisabled => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::TooManyVerificationAttempts => StatusCode::TOO_MANY_REQUESTS,
            Self::Database(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        // Log the real cause server-side; the client only ever sees the
        // sanitised message from `Display`.
        match self {
            Self::Database(source) => tracing::error!(error = %source, "database error"),
            Self::Internal(detail) => tracing::error!(detail = %detail, "internal error"),
            other => tracing::debug!(code = other.code(), error = %other, "request rejected"),
        }

        HttpResponse::build(self.status_code()).json(ErrorBody {
            error: ErrorDetail {
                code: self.code(),
                message: self.to_string(),
            },
        })
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
}
