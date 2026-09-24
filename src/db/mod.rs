//! Connection-pool construction and migrations.

use std::time::Duration;

use sqlx::PgPool;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;

/// Migrations are embedded in the binary at compile time, so the release image
/// does not need the `migrations/` directory on disk.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Opens a pool, retrying while PostgreSQL refuses connections.
///
/// Under `docker compose` the API container can win the race against the
/// database even with a `service_healthy` dependency (the health check passes as
/// soon as `pg_isready` succeeds, which is slightly before the server is ready
/// for application connections). Retrying here turns that race into a few
/// seconds of start-up delay instead of a crash loop.
pub async fn connect(database_url: &str, max_attempts: u32) -> Result<PgPool, sqlx::Error> {
    let mut attempt = 1;

    loop {
        let result = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(database_url)
            .await;

        match result {
            Ok(pool) => return Ok(pool),
            Err(error) if attempt < max_attempts => {
                tracing::warn!(
                    attempt,
                    max_attempts,
                    error = %error,
                    "database not ready, retrying"
                );
                attempt += 1;
                actix_web::rt::time::sleep(Duration::from_millis(500)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Applies any outstanding migrations. Idempotent.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}
