use actix_web::{HttpResponse, get, web};

use crate::api::dto::HealthResponse;
use crate::error::AppResult;
use crate::state::AppState;

/// Liveness plus a real database round-trip, so a green response means the API can
/// actually serve traffic rather than merely that the process is up.
#[get("/health")]
pub async fn health(state: web::Data<AppState>) -> AppResult<HttpResponse> {
    let database = match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
    {
        Ok(_) => "up",
        Err(error) => {
            tracing::warn!(error = %error, "health check could not reach the database");
            "down"
        }
    };

    Ok(HttpResponse::Ok().json(HealthResponse {
        status: if database == "up" { "ok" } else { "degraded" },
        database,
        cached_task_lists: state.cache.entry_count(),
    }))
}
