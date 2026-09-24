//! Development-only endpoints.
//!
//! These expose verification codes, so every handler here is gated on
//! `ENABLE_DEV_ENDPOINTS` and returns `403` when it is off. That flag is the one
//! switch to flip before this code could go anywhere near a real deployment.

use actix_web::{HttpResponse, get, web};

use crate::api::dto::{EmailLogListResponse, EmailLogQuery, EmailLogResponse};
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::state::AppState;

/// Hard ceiling on `?limit=`, so a caller cannot ask for the whole table.
const MAX_EMAIL_LOG_LIMIT: i64 = 100;
const DEFAULT_EMAIL_LOG_LIMIT: i64 = 20;

/// `GET /dev/email-logs/latest` — the most recently "sent" email.
///
/// Optional `?email=` narrows it to one recipient, which is what you want once
/// both users have started a login.
#[get("/email-logs/latest")]
pub async fn latest_email_log(
    state: web::Data<AppState>,
    query: web::Query<EmailLogQuery>,
) -> AppResult<HttpResponse> {
    ensure_dev_endpoints_enabled(&state)?;

    let log = repo::email_logs::find_latest(&state.db, query.email.as_deref())
        .await?
        .ok_or(AppError::not_found("email log"))?;

    Ok(HttpResponse::Ok().json(EmailLogResponse::from(&log)))
}

/// `GET /dev/email-logs` — recent emails, newest first.
#[get("/email-logs")]
pub async fn list_email_logs(
    state: web::Data<AppState>,
    query: web::Query<EmailLogQuery>,
) -> AppResult<HttpResponse> {
    ensure_dev_endpoints_enabled(&state)?;

    let limit = query
        .limit
        .unwrap_or(DEFAULT_EMAIL_LOG_LIMIT)
        .clamp(1, MAX_EMAIL_LOG_LIMIT);

    let logs = repo::email_logs::list_recent(&state.db, query.email.as_deref(), limit).await?;
    let emails: Vec<EmailLogResponse> = logs.iter().map(EmailLogResponse::from).collect();

    Ok(HttpResponse::Ok().json(EmailLogListResponse {
        total: emails.len(),
        emails,
    }))
}

fn ensure_dev_endpoints_enabled(state: &AppState) -> AppResult<()> {
    if state.config.dev_endpoints_enabled {
        Ok(())
    } else {
        Err(AppError::DevEndpointDisabled)
    }
}
