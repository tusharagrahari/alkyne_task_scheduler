//! HTTP layer: request/response shapes and route registration.
//!
//! Handlers stay thin on purpose — decode, delegate to a service, serialise. The
//! rules live in [`crate::services`].

pub mod auth;
pub mod dev;
pub mod dto;
pub mod health;
pub mod seed;
pub mod tasks;

use actix_web::web;

/// Registers every route.
///
/// Shared by `main` and the integration tests, so the tests exercise the same
/// routing table the binary serves.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(health::health)
        .service(seed::seed_users)
        .service(
            web::scope("/auth")
                .service(auth::login)
                .service(auth::verify_two_factor),
        )
        .service(
            web::scope("/dev")
                .service(dev::latest_email_log)
                .service(dev::list_email_logs),
        )
        .service(
            // `/tasks/view-my-tasks` is registered before `/tasks/{id}` so the
            // literal path always wins over the parameterised one.
            web::scope("/tasks")
                .service(tasks::view_my_tasks)
                .service(tasks::assign_tasks)
                .service(tasks::create_task)
                .service(tasks::list_tasks)
                .service(tasks::update_task),
        );
}
