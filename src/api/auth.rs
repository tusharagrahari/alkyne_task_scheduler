use actix_web::{HttpResponse, post, web};

use crate::api::dto::{
    AccessTokenResponse, LoginChallengeResponse, LoginRequest, UserResponse, VerifyTwoFactorRequest,
};
use crate::error::{AppError, AppResult};
use crate::services::auth;
use crate::state::AppState;

/// `POST /auth/login` — step one of two-factor login.
///
/// Returns `200` with a `login_challenge_id` and never an access token.
#[post("/login")]
pub async fn login(
    state: web::Data<AppState>,
    body: web::Json<LoginRequest>,
) -> AppResult<HttpResponse> {
    let body = body.into_inner();
    if body.email.trim().is_empty() || body.password.is_empty() {
        return Err(AppError::validation("email and password are required"));
    }

    let started = auth::start_login(&state, &body.email, &body.password).await?;

    Ok(HttpResponse::Ok().json(LoginChallengeResponse {
        login_challenge_id: started.challenge_id,
        two_factor_required: true,
        expires_at: started.expires_at,
        message: format!(
            "A verification code was sent to {}. Submit it to /auth/verify-2fa \
             together with login_challenge_id.",
            started.email
        ),
    }))
}

/// `POST /auth/verify-2fa` — step two. Exchanges the emailed code for a JWT.
#[post("/verify-2fa")]
pub async fn verify_two_factor(
    state: web::Data<AppState>,
    body: web::Json<VerifyTwoFactorRequest>,
) -> AppResult<HttpResponse> {
    let body = body.into_inner();
    if body.code.trim().is_empty() {
        return Err(AppError::validation("code is required"));
    }

    let completed = auth::verify_two_factor(&state, body.login_challenge_id, &body.code).await?;

    Ok(HttpResponse::Ok().json(AccessTokenResponse {
        access_token: completed.token.token,
        token_type: "Bearer",
        expires_in: completed.token.expires_in_seconds,
        expires_at: completed.token.expires_at,
        user: UserResponse::from(&completed.user),
    }))
}
