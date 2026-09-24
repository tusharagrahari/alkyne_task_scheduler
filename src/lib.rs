//! Task management API with email two-factor login, role-based access control and
//! per-user caching of the assigned-task view.
//!
//! # Layering
//!
//! ```text
//! api       HTTP: extractors, request/response DTOs, status codes
//!   ↓
//! services  use cases: 2FA login, admin-only writes, cache policy
//!   ↓
//! repo      every SQL statement in the application
//!   ↓
//! domain    entities and read models, no framework types
//! ```
//!
//! `security`, `cache` and `mail` are cross-cutting: they are used by services but
//! own no business rules themselves.
//!
//! Both the binary and the integration tests build their server through
//! [`build_app`], so the tests exercise the same middleware and routing table
//! that `main` serves.

pub mod api;
pub mod cache;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod mail;
pub mod repo;
pub mod security;
pub mod services;
pub mod state;

use actix_web::body::MessageBody;
use actix_web::dev::{ServiceFactory, ServiceRequest, ServiceResponse};
use actix_web::{App, Error, middleware, web};

use crate::error::AppError;

pub use state::AppState;

/// Builds the configured Actix application.
pub fn build_app(
    state: web::Data<AppState>,
) -> App<
    impl ServiceFactory<
        ServiceRequest,
        Config = (),
        Response = ServiceResponse<impl MessageBody>,
        Error = Error,
        InitError = (),
    >,
> {
    App::new()
        .app_data(state)
        // Actix's default extractor errors are plain-text bodies. Routing them
        // through `AppError` keeps *every* failure on the same JSON envelope, so a
        // client never has to special-case malformed input.
        .app_data(
            // Also caps body size, so an oversized payload is rejected rather than buffered.
            web::JsonConfig::default()
                .limit(64 * 1024)
                .error_handler(|error, _| {
                    AppError::validation(format!("invalid JSON body: {error}")).into()
                }),
        )
        .app_data(web::QueryConfig::default().error_handler(|error, _| {
            AppError::validation(format!("invalid query string: {error}")).into()
        }))
        .app_data(web::PathConfig::default().error_handler(|error, _| {
            AppError::validation(format!("invalid path parameter: {error}")).into()
        }))
        .wrap(middleware::NormalizePath::trim())
        .wrap(middleware::Logger::default())
        .configure(api::configure)
}
