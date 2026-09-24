use actix_web::{HttpResponse, post, web};

use crate::api::dto::{SeedUsersRequest, SeedUsersResponse, SeededCredential, UserResponse};
use crate::error::AppResult;
use crate::services::seed::{self, SeedOverrides};
use crate::state::AppState;

/// `POST /seed/users` — creates the Admin and James Bond accounts.
///
/// The body is optional: posting `{}` (or no body at all) uses the credentials
/// from configuration. Safe to call more than once.
#[post("/seed/users")]
pub async fn seed_users(
    state: web::Data<AppState>,
    body: Option<web::Json<SeedUsersRequest>>,
) -> AppResult<HttpResponse> {
    let request = body.map(web::Json::into_inner).unwrap_or_default();

    // Remember the effective passwords before they are hashed, so the response
    // can echo the credentials the reviewer should log in with.
    let defaults = &state.config.seed;
    let admin_password = request
        .admin_password
        .clone()
        .unwrap_or_else(|| defaults.admin_password.clone());
    let staff_password = request
        .staff_password
        .clone()
        .unwrap_or_else(|| defaults.staff_password.clone());

    let seeded = seed::seed_users(
        &state,
        SeedOverrides {
            admin_email: request.admin_email,
            admin_password: request.admin_password,
            admin_full_name: request.admin_full_name,
            staff_email: request.staff_email,
            staff_password: request.staff_password,
            staff_full_name: request.staff_full_name,
        },
    )
    .await?;

    Ok(HttpResponse::Ok().json(SeedUsersResponse {
        users: vec![
            UserResponse::from(&seeded.admin),
            UserResponse::from(&seeded.staff),
        ],
        credentials: vec![
            SeededCredential {
                email: seeded.admin.email.clone(),
                password: admin_password,
                role: seeded.admin.role,
            },
            SeededCredential {
                email: seeded.staff.email.clone(),
                password: staff_password,
                role: seeded.staff.role,
            },
        ],
    }))
}
