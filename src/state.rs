//! Shared application state, registered once as Actix application data.

use std::sync::Arc;

use sqlx::PgPool;

use crate::cache::TaskCache;
use crate::config::AppConfig;
use crate::mail::DevMailer;
use crate::security::JwtService;

/// Everything a handler may need, assembled at start-up.
///
/// Wrapped in `web::Data` (an `Arc`) by the caller, so cloning per request is a
/// pointer bump.
#[derive(Debug)]
pub struct AppState {
    pub db: PgPool,
    pub cache: Arc<dyn TaskCache>,
    pub mailer: DevMailer,
    pub jwt: JwtService,
    pub config: AppConfig,
}

impl AppState {
    pub fn new(db: PgPool, cache: Arc<dyn TaskCache>, config: AppConfig) -> Self {
        let jwt = JwtService::new(&config.jwt_secret, config.jwt_ttl);
        Self {
            mailer: DevMailer::new(db.clone()),
            db,
            cache,
            jwt,
            config,
        }
    }
}
