//! Request extractors that enforce authentication and role-based access.
//!
//! Putting the checks in extractors rather than in handler bodies means a
//! handler's signature *is* its access-control policy: `AdminUser` in the
//! argument list is the guarantee that non-admins never reach the body.

use std::future::{Ready, ready};

use actix_web::http::header;
use actix_web::{FromRequest, HttpRequest, dev::Payload, web};
use uuid::Uuid;

use crate::domain::user::Role;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// A caller holding a valid access token. Extracting this yields `401` when the
/// `Authorization` header is missing, malformed, or carries a bad/expired token.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: Uuid,
    pub email: String,
    pub role: Role,
}

impl AuthenticatedUser {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}

/// A caller holding a valid access token **whose role is `admin`**.
///
/// Ordering matters for the assignment: an absent or invalid token is `401`,
/// while a valid staff token is `403 Forbidden`.
#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthenticatedUser);

impl std::ops::Deref for AdminUser {
    type Target = AuthenticatedUser;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRequest for AuthenticatedUser {
    type Error = AppError;
    type Future = Ready<AppResult<Self>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        ready(authenticate(req))
    }
}

impl FromRequest for AdminUser {
    type Error = AppError;
    type Future = Ready<AppResult<Self>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        ready(authenticate(req).and_then(|user| {
            if user.is_admin() {
                Ok(Self(user))
            } else {
                Err(AppError::InsufficientRole {
                    required: Role::Admin,
                })
            }
        }))
    }
}

/// Parses `Authorization: Bearer <token>` and verifies the token.
fn authenticate(req: &HttpRequest) -> AppResult<AuthenticatedUser> {
    let state = req
        .app_data::<web::Data<AppState>>()
        .ok_or_else(|| AppError::internal("AppState is not registered as application data"))?;

    let raw = req
        .headers()
        .get(header::AUTHORIZATION)
        .ok_or(AppError::MissingBearerToken)?
        .to_str()
        .map_err(|_| AppError::MissingBearerToken)?;

    // The scheme is case-insensitive per RFC 7235.
    let (scheme, token) = raw.split_once(' ').ok_or(AppError::MissingBearerToken)?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(AppError::MissingBearerToken);
    }

    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::MissingBearerToken);
    }

    let claims = state.jwt.verify(token)?;
    Ok(AuthenticatedUser {
        id: claims.sub,
        email: claims.email,
        role: claims.role,
    })
}
