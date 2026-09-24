//! Binary entry point: load configuration, connect, migrate, serve.

use std::sync::Arc;

use actix_web::{HttpServer, web};
use alkyne::cache::InMemoryTaskCache;
use alkyne::config::AppConfig;
use alkyne::{AppState, build_app, db};
use tracing_subscriber::EnvFilter;

/// Attempts made while waiting for PostgreSQL to accept connections at start-up.
const DB_CONNECT_ATTEMPTS: u32 = 30;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // A missing .env is fine: container deployments pass real environment variables.
    let _ = dotenvy::dotenv();
    init_tracing();

    let config = AppConfig::from_env()
        .map_err(|error| std::io::Error::other(format!("invalid configuration: {error}")))?;

    let pool = db::connect(&config.database_url, DB_CONNECT_ATTEMPTS)
        .await
        .map_err(|error| {
            std::io::Error::other(format!("could not connect to database: {error}"))
        })?;

    // Migrating on boot keeps `docker compose up` a single step. In a deployment
    // with more than one replica this belongs in a release job instead, so the
    // replicas do not race each other; sqlx takes an advisory lock, so even then
    // it would be correct, just serialised.
    db::run_migrations(&pool)
        .await
        .map_err(|error| std::io::Error::other(format!("migrations failed: {error}")))?;
    tracing::info!("migrations are up to date");

    let cache = Arc::new(InMemoryTaskCache::new(config.cache_ttl));
    let bind_address = config.bind_address.clone();
    let dev_endpoints_enabled = config.dev_endpoints_enabled;
    let state = web::Data::new(AppState::new(pool, cache, config));

    tracing::info!(
        address = %bind_address,
        dev_endpoints_enabled,
        "starting task management API"
    );
    if dev_endpoints_enabled {
        tracing::warn!(
            "development endpoints are enabled: GET /dev/email-logs/latest exposes \
             two-factor verification codes. Set ENABLE_DEV_ENDPOINTS=false to disable."
        );
    }

    HttpServer::new(move || build_app(state.clone()))
        .bind(&bind_address)?
        .run()
        .await
}

/// `RUST_LOG` wins; otherwise log at info and keep sqlx's per-statement chatter out.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx::query=warn"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
