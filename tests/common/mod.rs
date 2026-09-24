//! Integration-test harness.
//!
//! # Isolation
//!
//! Every test gets its own PostgreSQL database (`alkyne_test_<uuid>`), created and
//! migrated in [`TestContext::new`] and dropped in [`TestContext::cleanup`]. That
//! makes the suite safe to run in parallel and lets each test assert on absolute
//! counts ("exactly 5 tasks", "exactly 3 assigned") without coordinating with its
//! neighbours.
//!
//! A test that fails before calling `cleanup` deliberately leaves its database
//! behind, so the state that produced the failure can still be inspected. List
//! leftovers with `psql -l | grep alkyne_test_`.

#![allow(dead_code)] // Helpers are shared; not every test binary uses all of them.

use std::sync::Arc;
use std::time::Duration;

use actix_http::Request;
use actix_web::body::MessageBody;
use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::{test, web};
use alkyne::cache::InMemoryTaskCache;
use alkyne::config::{AppConfig, SeedConfig};
use alkyne::state::AppState;
use serde_json::{Value, json};
use sqlx::{Connection, Executor, PgConnection, PgPool};
use uuid::Uuid;

/// Fallback used when `DATABASE_URL` is not set, matching `docker-compose.yml`.
const DEFAULT_DATABASE_URL: &str = "postgres://alkyne:alkyne@localhost:5432/alkyne";

pub const ADMIN_EMAIL: &str = "admin@example.com";
pub const ADMIN_PASSWORD: &str = "AdminPass123!";
pub const STAFF_EMAIL: &str = "jamesbond@example.com";
pub const STAFF_PASSWORD: &str = "BondPass123!";

/// One test's isolated database plus the application state wired to it.
pub struct TestContext {
    pub state: web::Data<AppState>,
    pub db: PgPool,
    /// The same cache instance held by `state`, typed concretely so tests can
    /// assert on entry counts.
    pub cache: Arc<InMemoryTaskCache>,
    database_name: String,
    maintenance_url: String,
}

impl TestContext {
    pub async fn new() -> Self {
        Self::with_config(|_| {}).await
    }

    /// Builds a context, letting the caller tweak the configuration first (for
    /// example to disable the development endpoints).
    pub async fn with_config(customize: impl FnOnce(&mut AppConfig)) -> Self {
        let base_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
        let maintenance_url = with_database(&base_url, "postgres");
        let database_name = format!("alkyne_test_{}", Uuid::new_v4().simple());

        let mut admin = PgConnection::connect(&maintenance_url)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "could not reach PostgreSQL at {maintenance_url}: {error}\n\
                     Start it with `docker compose up -d db`."
                )
            });
        // Identifier is a generated UUID, so quoting is enough; there is no user
        // input in this statement.
        admin
            .execute(format!(r#"CREATE DATABASE "{database_name}""#).as_str())
            .await
            .expect("create test database");
        admin.close().await.ok();

        let db = PgPool::connect(&with_database(&base_url, &database_name))
            .await
            .expect("connect to test database");
        alkyne::db::run_migrations(&db)
            .await
            .expect("run migrations on test database");

        let mut config = test_config(&with_database(&base_url, &database_name));
        customize(&mut config);

        let cache = Arc::new(InMemoryTaskCache::new(config.cache_ttl));
        let state = web::Data::new(AppState::new(
            db.clone(),
            cache.clone() as Arc<dyn alkyne::cache::TaskCache>,
            config,
        ));

        Self {
            state,
            db,
            cache,
            database_name,
            maintenance_url,
        }
    }

    /// Drops the test database. Call at the end of a passing test.
    pub async fn cleanup(self) {
        let Self {
            db,
            database_name,
            maintenance_url,
            ..
        } = self;

        // The pool must be closed first, or `DROP DATABASE` finds live sessions.
        db.close().await;

        if let Ok(mut admin) = PgConnection::connect(&maintenance_url).await {
            let _ = admin
                .execute(
                    format!(r#"DROP DATABASE IF EXISTS "{database_name}" WITH (FORCE)"#).as_str(),
                )
                .await;
            admin.close().await.ok();
        }
    }

    /// Forces a challenge to look expired, so expiry can be tested without waiting.
    pub async fn expire_challenge(&self, challenge_id: Uuid) {
        sqlx::query(
            "UPDATE login_challenges SET expires_at = now() - interval '1 minute' WHERE id = $1",
        )
        .bind(challenge_id)
        .execute(&self.db)
        .await
        .expect("expire challenge");
    }

    /// Reads a challenge's recorded failed-attempt count.
    pub async fn challenge_attempts(&self, challenge_id: Uuid) -> i32 {
        sqlx::query_scalar::<_, i32>("SELECT attempts FROM login_challenges WHERE id = $1")
            .bind(challenge_id)
            .fetch_one(&self.db)
            .await
            .expect("read attempts")
    }

    pub async fn user_count(&self) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users")
            .fetch_one(&self.db)
            .await
            .expect("count users")
    }

    pub async fn task_count(&self) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM tasks")
            .fetch_one(&self.db)
            .await
            .expect("count tasks")
    }
}

/// Test configuration. Built directly rather than through `AppConfig::from_env`
/// so tests never mutate shared process environment while running in parallel.
fn test_config(database_url: &str) -> AppConfig {
    AppConfig {
        database_url: database_url.to_owned(),
        bind_address: "127.0.0.1:0".to_owned(),
        jwt_secret: "integration-test-secret-at-least-32-bytes".to_owned(),
        jwt_ttl: Duration::from_secs(3_600),
        two_factor_code_ttl: Duration::from_secs(300),
        two_factor_max_attempts: 5,
        cache_ttl: Duration::from_secs(300),
        dev_endpoints_enabled: true,
        seed: SeedConfig {
            admin_full_name: "Admin".to_owned(),
            admin_email: ADMIN_EMAIL.to_owned(),
            admin_password: ADMIN_PASSWORD.to_owned(),
            staff_full_name: "James Bond".to_owned(),
            staff_email: STAFF_EMAIL.to_owned(),
            staff_password: STAFF_PASSWORD.to_owned(),
        },
    }
}

/// Rewrites the database name in a PostgreSQL connection URL, preserving
/// credentials, host, port and any query string.
pub fn with_database(url: &str, database: &str) -> String {
    let (scheme, rest) = url
        .split_once("://")
        .unwrap_or_else(|| panic!("`{url}` is not a URL"));

    // The authority ends at the first `/`, `?` or end-of-string.
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let tail = &rest[authority_end..];
    let query = tail.find('?').map(|at| &tail[at..]).unwrap_or("");

    format!("{scheme}://{authority}/{database}{query}")
}

// --------------------------------------------------------------- HTTP helpers
//
// Bounds are spelled out on each helper because `test::init_service` returns an
// opaque service type that cannot be stored in the context struct.

/// Sends a request and decodes the response as `(status, json)`.
pub async fn call_json<S, B>(app: &S, req: Request) -> (StatusCode, Value)
where
    S: Service<Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    let response = test::call_service(app, req).await;
    let status = response.status();
    let bytes = test::read_body(response).await;

    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            panic!(
                "response body was not JSON ({error}): {}",
                String::from_utf8_lossy(&bytes)
            )
        })
    };

    (status, body)
}

/// `POST /seed/users` with the configured defaults.
pub async fn seed_users<S, B>(app: &S) -> Value
where
    S: Service<Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    let (status, body) = call_json(
        app,
        test::TestRequest::post()
            .uri("/seed/users")
            .set_json(json!({}))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "seeding failed: {body}");
    body
}

/// Step one of login. Returns the challenge id.
pub async fn start_login<S, B>(app: &S, email: &str, password: &str) -> Uuid
where
    S: Service<Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    let (status, body) = call_json(
        app,
        test::TestRequest::post()
            .uri("/auth/login")
            .set_json(json!({ "email": email, "password": password }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "login failed: {body}");
    assert!(
        body.get("access_token").is_none(),
        "step one must not return a token: {body}"
    );

    body["login_challenge_id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("no login_challenge_id in {body}"))
}

/// Reads the verification code for `email` back out of the development mailbox.
pub async fn latest_verification_code<S, B>(app: &S, email: &str) -> String
where
    S: Service<Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    let (status, body) = call_json(
        app,
        test::TestRequest::get()
            .uri(&format!("/dev/email-logs/latest?email={email}"))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "email log lookup failed: {body}");
    body["verification_code"]
        .as_str()
        .unwrap_or_else(|| panic!("no verification_code in {body}"))
        .to_owned()
}

/// Step two of login. Returns the access token.
pub async fn verify_two_factor<S, B>(app: &S, challenge_id: Uuid, code: &str) -> String
where
    S: Service<Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    let (status, body) = call_json(
        app,
        test::TestRequest::post()
            .uri("/auth/verify-2fa")
            .set_json(json!({ "login_challenge_id": challenge_id, "code": code }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "2FA verification failed: {body}");
    body["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("no access_token in {body}"))
        .to_owned()
}

/// The whole login flow: password, emailed code, token.
pub async fn authenticate<S, B>(app: &S, email: &str, password: &str) -> String
where
    S: Service<Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    let challenge_id = start_login(app, email, password).await;
    let code = latest_verification_code(app, email).await;
    verify_two_factor(app, challenge_id, &code).await
}

/// Creates one task as an admin and returns its id.
pub async fn create_task<S, B>(app: &S, token: &str, title: &str, priority: &str) -> Uuid
where
    S: Service<Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    let (status, body) = call_json(
        app,
        test::TestRequest::post()
            .uri("/tasks")
            .insert_header(("Authorization", format!("Bearer {token}")))
            .set_json(json!({
                "title": title,
                "description": format!("{title} — created by the integration suite"),
                "priority": priority,
            }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "task creation failed: {body}");
    body["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("no task id in {body}"))
}

/// `GET /tasks/view-my-tasks` as the given token holder.
pub async fn view_my_tasks<S, B>(app: &S, token: &str) -> (StatusCode, Value)
where
    S: Service<Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    call_json(
        app,
        test::TestRequest::get()
            .uri("/tasks/view-my-tasks")
            .insert_header(("Authorization", format!("Bearer {token}")))
            .to_request(),
    )
    .await
}

/// Reads `error.code` from an error response body.
pub fn error_code(body: &Value) -> &str {
    body["error"]["code"]
        .as_str()
        .unwrap_or_else(|| panic!("no error.code in {body}"))
}
