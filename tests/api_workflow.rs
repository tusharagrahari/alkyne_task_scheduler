//! Integration tests over the real HTTP surface and a real PostgreSQL database.
//!
//! Each test builds the same application the binary serves (via
//! `alkyne::build_app`) against its own throwaway database. Together they cover
//! every bullet in the assignment's "Testing Expectations", and
//! [`end_to_end_validation_workflow`] walks the full eleven-step flow.
//!
//! Requires PostgreSQL: `docker compose up -d db`.

mod common;

use actix_http::Request;
use actix_web::body::MessageBody;
use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::test;
use alkyne::cache::TaskCache;
use common::{
    ADMIN_EMAIL, ADMIN_PASSWORD, STAFF_EMAIL, STAFF_PASSWORD, TestContext, authenticate, call_json,
    create_task, error_code, latest_verification_code, seed_users, start_login, verify_two_factor,
    view_my_tasks,
};
use serde_json::json;
use uuid::Uuid;

/// The five tasks the validation flow creates, and the three that get assigned.
const TASKS: [(&str, &str); 5] = [
    ("Infiltrate the casino", "high"),
    ("Recover the stolen ledger", "medium"),
    ("File the expense report", "low"),
    ("Service the Aston Martin", "medium"),
    ("Brief Q on the new gadget", "high"),
];

// ---------------------------------------------------------------- users / seed

#[actix_web::test]
async fn seed_creates_admin_and_james_bond() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;

    let body = seed_users(&app).await;

    let users = body["users"].as_array().expect("users array");
    assert_eq!(users.len(), 2);
    assert_eq!(users[0]["email"], ADMIN_EMAIL);
    assert_eq!(users[0]["role"], "admin");
    assert_eq!(users[1]["email"], STAFF_EMAIL);
    assert_eq!(users[1]["role"], "staff");
    assert_eq!(users[1]["full_name"], "James Bond");
    assert_eq!(ctx.user_count().await, 2);

    // Hashes must never be serialised.
    assert!(
        !body.to_string().contains("argon2"),
        "response leaked a password hash: {body}"
    );

    ctx.cleanup().await;
}

#[actix_web::test]
async fn seeding_twice_is_idempotent() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;

    let first = seed_users(&app).await;
    let second = seed_users(&app).await;

    assert_eq!(ctx.user_count().await, 2, "re-seeding must not add users");
    assert_eq!(
        first["users"][0]["id"], second["users"][0]["id"],
        "re-seeding must update the existing row, not replace it"
    );

    // And the credentials still work afterwards.
    let token = authenticate(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    assert!(!token.is_empty());

    ctx.cleanup().await;
}

// ------------------------------------------------------------- 2FA login rules

#[actix_web::test]
async fn login_creates_challenge_and_does_not_return_a_jwt() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/auth/login")
            .set_json(json!({ "email": ADMIN_EMAIL, "password": ADMIN_PASSWORD }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["login_challenge_id"].is_string(), "{body}");
    assert_eq!(body["two_factor_required"], true);
    assert!(
        body.get("access_token").is_none() && body.get("token").is_none(),
        "step one leaked a token: {body}"
    );

    // The code reached the development mailbox rather than the response body.
    let code = latest_verification_code(&app, ADMIN_EMAIL).await;
    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|c| c.is_ascii_digit()), "code was {code}");
    assert!(
        !body.to_string().contains(&code),
        "login response must not contain the code: {body}"
    );

    ctx.cleanup().await;
}

#[actix_web::test]
async fn login_rejects_wrong_password_and_unknown_email_identically() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    for (email, password) in [
        (ADMIN_EMAIL, "not-the-password"),
        ("nobody@example.com", ADMIN_PASSWORD),
    ] {
        let (status, body) = call_json(
            &app,
            test::TestRequest::post()
                .uri("/auth/login")
                .set_json(json!({ "email": email, "password": password }))
                .to_request(),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED, "{email}: {body}");
        // Identical code for both, so accounts cannot be enumerated.
        assert_eq!(error_code(&body), "invalid_credentials");
    }

    ctx.cleanup().await;
}

#[actix_web::test]
async fn correct_two_factor_code_returns_a_jwt() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    let challenge_id = start_login(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let code = latest_verification_code(&app, ADMIN_EMAIL).await;

    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/auth/verify-2fa")
            .set_json(json!({ "login_challenge_id": challenge_id, "code": code }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(body["user"]["email"], ADMIN_EMAIL);
    assert_eq!(body["user"]["role"], "admin");

    // A JWT is three dot-separated segments.
    let token = body["access_token"].as_str().expect("token");
    assert_eq!(token.split('.').count(), 3, "not a JWT: {token}");

    // And it actually authorises a request.
    let (status, _) = view_my_tasks(&app, token).await;
    assert_eq!(status, StatusCode::OK);

    ctx.cleanup().await;
}

#[actix_web::test]
async fn incorrect_two_factor_code_is_rejected_and_counted() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    let challenge_id = start_login(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;

    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/auth/verify-2fa")
            .set_json(json!({ "login_challenge_id": challenge_id, "code": "000000" }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(error_code(&body), "invalid_verification_code");
    assert_eq!(ctx.challenge_attempts(challenge_id).await, 1);

    // The challenge is not burned: the real code still works.
    let code = latest_verification_code(&app, ADMIN_EMAIL).await;
    let token = verify_two_factor(&app, challenge_id, &code).await;
    assert!(!token.is_empty());

    ctx.cleanup().await;
}

#[actix_web::test]
async fn expired_two_factor_code_is_rejected() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    let challenge_id = start_login(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let code = latest_verification_code(&app, ADMIN_EMAIL).await;

    // Rewind the clock on the challenge instead of sleeping for five minutes.
    ctx.expire_challenge(challenge_id).await;

    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/auth/verify-2fa")
            .set_json(json!({ "login_challenge_id": challenge_id, "code": code }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(error_code(&body), "verification_code_expired");

    ctx.cleanup().await;
}

#[actix_web::test]
async fn reused_two_factor_code_is_rejected() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    let challenge_id = start_login(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let code = latest_verification_code(&app, ADMIN_EMAIL).await;

    let token = verify_two_factor(&app, challenge_id, &code).await;
    assert!(!token.is_empty(), "first use must succeed");

    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/auth/verify-2fa")
            .set_json(json!({ "login_challenge_id": challenge_id, "code": code }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(error_code(&body), "verification_code_already_used");

    ctx.cleanup().await;
}

#[actix_web::test]
async fn challenge_locks_after_too_many_wrong_codes() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    let challenge_id = start_login(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let real_code = latest_verification_code(&app, ADMIN_EMAIL).await;

    // Exhaust the five permitted attempts with a code that cannot be the real one.
    let wrong_code = if real_code == "000000" {
        "111111"
    } else {
        "000000"
    };
    for _ in 0..5 {
        let (status, body) = call_json(
            &app,
            test::TestRequest::post()
                .uri("/auth/verify-2fa")
                .set_json(json!({ "login_challenge_id": challenge_id, "code": wrong_code }))
                .to_request(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    }

    // The sixth attempt is refused before the code is even compared — so even the
    // correct code no longer works.
    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/auth/verify-2fa")
            .set_json(json!({ "login_challenge_id": challenge_id, "code": real_code }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(error_code(&body), "too_many_verification_attempts");

    ctx.cleanup().await;
}

#[actix_web::test]
async fn malformed_bodies_use_the_same_error_envelope() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;

    // Unknown field: `deny_unknown_fields` catches a typo or a smuggled attribute.
    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/auth/login")
            .set_json(json!({ "email": "a@b.com", "password": "x", "role": "admin" }))
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "validation_error");

    // Missing field.
    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/auth/login")
            .set_json(json!({ "email": "a@b.com" }))
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "validation_error");

    // Non-UUID path parameter.
    let admin_token = {
        seed_users(&app).await;
        authenticate(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await
    };
    let (status, body) = call_json(
        &app,
        test::TestRequest::patch()
            .uri("/tasks/not-a-uuid")
            .insert_header(("Authorization", format!("Bearer {admin_token}")))
            .set_json(json!({ "status": "done" }))
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    ctx.cleanup().await;
}

#[actix_web::test]
async fn unknown_challenge_id_is_not_found() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;

    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/auth/verify-2fa")
            .set_json(json!({ "login_challenge_id": Uuid::new_v4(), "code": "123456" }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    ctx.cleanup().await;
}

// -------------------------------------------------------- role-based access

#[actix_web::test]
async fn admin_can_create_five_tasks() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;
    let admin_token = authenticate(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;

    for (title, priority) in TASKS {
        create_task(&app, &admin_token, title, priority).await;
    }

    assert_eq!(ctx.task_count().await, 5);

    let (status, body) = call_json(
        &app,
        test::TestRequest::get()
            .uri("/tasks")
            .insert_header(("Authorization", format!("Bearer {admin_token}")))
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 5);

    ctx.cleanup().await;
}

#[actix_web::test]
async fn james_bond_cannot_create_a_task() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;
    let staff_token = authenticate(&app, STAFF_EMAIL, STAFF_PASSWORD).await;

    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/tasks")
            .insert_header(("Authorization", format!("Bearer {staff_token}")))
            .set_json(json!({ "title": "Promote myself", "priority": "high" }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "insufficient_role");
    assert_eq!(ctx.task_count().await, 0, "no task may have been written");

    // Assignment is admin-only too.
    let (status, _) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/tasks/assign")
            .insert_header(("Authorization", format!("Bearer {staff_token}")))
            .set_json(json!({ "task_ids": [Uuid::new_v4()], "assignee_email": STAFF_EMAIL }))
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    ctx.cleanup().await;
}

#[actix_web::test]
async fn missing_or_invalid_tokens_are_unauthorised() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    // No header at all.
    let (status, body) = call_json(
        &app,
        test::TestRequest::get()
            .uri("/tasks/view-my-tasks")
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "missing_bearer_token");

    // Wrong scheme.
    let (status, body) = call_json(
        &app,
        test::TestRequest::get()
            .uri("/tasks/view-my-tasks")
            .insert_header(("Authorization", "Basic YWRtaW46YWRtaW4="))
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "missing_bearer_token");

    // Well-formed header, junk token.
    let (status, body) = call_json(
        &app,
        test::TestRequest::get()
            .uri("/tasks/view-my-tasks")
            .insert_header(("Authorization", "Bearer not.a.jwt"))
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "invalid_access_token");

    ctx.cleanup().await;
}

// -------------------------------------------------------------- assignment

#[actix_web::test]
async fn admin_can_assign_exactly_three_tasks_to_james_bond() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;
    let admin_token = authenticate(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;

    let mut task_ids = Vec::new();
    for (title, priority) in TASKS {
        task_ids.push(create_task(&app, &admin_token, title, priority).await);
    }

    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/tasks/assign")
            .insert_header(("Authorization", format!("Bearer {admin_token}")))
            .set_json(json!({
                "task_ids": [task_ids[0], task_ids[1], task_ids[2]],
                "assignee_email": STAFF_EMAIL,
            }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["assigned_count"], 3);
    assert_eq!(body["assignee"]["email"], STAFF_EMAIL);

    // Two of the five remain unassigned.
    let unassigned =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM tasks WHERE assigned_to_id IS NULL")
            .fetch_one(&ctx.db)
            .await
            .expect("count unassigned");
    assert_eq!(unassigned, 2);

    ctx.cleanup().await;
}

#[actix_web::test]
async fn assignment_is_atomic_when_a_task_id_is_unknown() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;
    let admin_token = authenticate(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;

    let real = create_task(&app, &admin_token, "Real task", "high").await;

    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/tasks/assign")
            .insert_header(("Authorization", format!("Bearer {admin_token}")))
            .set_json(json!({
                "task_ids": [real, Uuid::new_v4()],
                "assignee_email": STAFF_EMAIL,
            }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    // The valid half of the request must not have been applied.
    let assigned =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM tasks WHERE assigned_to_id IS NOT NULL")
            .fetch_one(&ctx.db)
            .await
            .expect("count assigned");
    assert_eq!(assigned, 0, "partial assignment leaked through");

    ctx.cleanup().await;
}

// ------------------------------------------------------------------ caching

#[actix_web::test]
async fn james_bond_sees_only_his_tasks_and_the_second_call_hits_the_cache() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    let admin_token = authenticate(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let mut task_ids = Vec::new();
    for (title, priority) in TASKS {
        task_ids.push(create_task(&app, &admin_token, title, priority).await);
    }

    let (status, _) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/tasks/assign")
            .insert_header(("Authorization", format!("Bearer {admin_token}")))
            .set_json(json!({
                "task_ids": [task_ids[0], task_ids[1], task_ids[2]],
                "assignee_email": STAFF_EMAIL,
            }))
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Assignment must not warm the cache, or the next call would already hit.
    assert_eq!(
        ctx.cache.entry_count(),
        0,
        "write paths must invalidate, never populate"
    );

    let staff_token = authenticate(&app, STAFF_EMAIL, STAFF_PASSWORD).await;

    let (status, first) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["cache"]["hit"], false, "first call must miss");
    assert_eq!(first["summary"]["total_assigned_tasks"], 3);
    assert_eq!(first["tasks"].as_array().expect("tasks").len(), 3);

    let (status, second) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["cache"]["hit"], true, "second call must hit");

    // A hit and a miss must describe the same tasks.
    assert_eq!(first["tasks"], second["tasks"]);
    assert_eq!(first["user"], second["user"]);

    // The admin created all five but is assigned none of them.
    let (status, admin_view) = view_my_tasks(&app, &admin_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(admin_view["summary"]["total_assigned_tasks"], 0);
    assert_eq!(admin_view["cache"]["hit"], false, "cache is per user");

    ctx.cleanup().await;
}

#[actix_web::test]
async fn assigning_another_task_invalidates_the_cached_view() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    let admin_token = authenticate(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let first_task = create_task(&app, &admin_token, "First", "high").await;
    let second_task = create_task(&app, &admin_token, "Second", "low").await;

    assign(&app, &admin_token, &[first_task], STAFF_EMAIL).await;
    let staff_token = authenticate(&app, STAFF_EMAIL, STAFF_PASSWORD).await;

    // Warm the cache.
    let (_, body) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(body["cache"]["hit"], false);
    let (_, body) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(body["cache"]["hit"], true);

    // Assigning a second task must drop the stale entry.
    assign(&app, &admin_token, &[second_task], STAFF_EMAIL).await;
    assert_eq!(ctx.cache.entry_count(), 0);

    let (_, body) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(body["cache"]["hit"], false, "stale entry was served");
    assert_eq!(body["summary"]["total_assigned_tasks"], 2);

    ctx.cleanup().await;
}

#[actix_web::test]
async fn updating_a_task_invalidates_the_cached_view() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;

    let admin_token = authenticate(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let task_id = create_task(&app, &admin_token, "Tail the courier", "low").await;
    assign(&app, &admin_token, &[task_id], STAFF_EMAIL).await;

    let staff_token = authenticate(&app, STAFF_EMAIL, STAFF_PASSWORD).await;
    let (_, warm) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(warm["cache"]["hit"], false);
    assert_eq!(warm["tasks"][0]["status"], "todo");
    let (_, warm) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(warm["cache"]["hit"], true);

    let (status, body) = call_json(
        &app,
        test::TestRequest::patch()
            .uri(&format!("/tasks/{task_id}"))
            .insert_header(("Authorization", format!("Bearer {admin_token}")))
            .set_json(json!({ "status": "in_progress", "priority": "high" }))
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "in_progress");

    let (_, after) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(after["cache"]["hit"], false, "update must invalidate");
    assert_eq!(after["tasks"][0]["status"], "in_progress");
    assert_eq!(after["tasks"][0]["priority"], "high");

    ctx.cleanup().await;
}

#[actix_web::test]
async fn reassignment_invalidates_both_the_old_and_the_new_assignee() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    let seeded = seed_users(&app).await;
    assert_eq!(seeded["users"].as_array().expect("users").len(), 2);

    let admin_token = authenticate(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let task_id = create_task(&app, &admin_token, "Hand-over dossier", "medium").await;

    // Starts with James Bond, who caches it...
    assign(&app, &admin_token, &[task_id], STAFF_EMAIL).await;
    let staff_token = authenticate(&app, STAFF_EMAIL, STAFF_PASSWORD).await;
    let (_, body) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(body["summary"]["total_assigned_tasks"], 1);
    let (_, body) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(body["cache"]["hit"], true);

    // ...then moves to the admin. Both cached views are now wrong.
    assign(&app, &admin_token, &[task_id], ADMIN_EMAIL).await;
    assert_eq!(
        ctx.cache.entry_count(),
        0,
        "both assignees must be invalidated"
    );

    let (_, staff_view) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(staff_view["cache"]["hit"], false);
    assert_eq!(staff_view["summary"]["total_assigned_tasks"], 0);

    let (_, admin_view) = view_my_tasks(&app, &admin_token).await;
    assert_eq!(admin_view["summary"]["total_assigned_tasks"], 1);
    assert_eq!(admin_view["tasks"][0]["assigned_to"], ADMIN_EMAIL);

    ctx.cleanup().await;
}

// ---------------------------------------------------------- dev endpoint gate

#[actix_web::test]
async fn development_endpoints_can_be_switched_off() {
    let ctx = TestContext::with_config(|config| config.dev_endpoints_enabled = false).await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;
    seed_users(&app).await;
    start_login(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;

    let (status, body) = call_json(
        &app,
        test::TestRequest::get()
            .uri("/dev/email-logs/latest")
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "dev_endpoint_disabled");

    ctx.cleanup().await;
}

// ------------------------------------------------- the full validation flow

/// The eleven-step workflow from the assignment, in order, with the exact final
/// response shape asserted field by field.
#[actix_web::test]
async fn end_to_end_validation_workflow() {
    let ctx = TestContext::new().await;
    let app = test::init_service(alkyne::build_app(ctx.state.clone())).await;

    // 1. Create two users: Admin and James Bond.
    let seeded = seed_users(&app).await;
    assert_eq!(seeded["users"][0]["role"], "admin");
    assert_eq!(seeded["users"][1]["role"], "staff");

    // 2. Start login as Admin — a challenge, not a token.
    let admin_challenge = start_login(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;

    // 3. Retrieve the code from the development email log.
    let admin_code = latest_verification_code(&app, ADMIN_EMAIL).await;

    // 4. Verify and receive the Admin JWT.
    let admin_token = verify_two_factor(&app, admin_challenge, &admin_code).await;

    // 5. Create exactly 5 tasks as Admin.
    let mut task_ids = Vec::new();
    for (title, priority) in TASKS {
        task_ids.push(create_task(&app, &admin_token, title, priority).await);
    }
    assert_eq!(ctx.task_count().await, 5);

    // 6. Assign exactly 3 of them to James Bond — one high, one medium, one low,
    //    mirroring the expected response in the assignment.
    assign(
        &app,
        &admin_token,
        &[task_ids[0], task_ids[1], task_ids[2]],
        STAFF_EMAIL,
    )
    .await;

    // 7 & 8. Log James Bond in through the same two-step flow.
    let staff_challenge = start_login(&app, STAFF_EMAIL, STAFF_PASSWORD).await;
    let staff_code = latest_verification_code(&app, STAFF_EMAIL).await;
    assert_ne!(staff_code, admin_code, "each login gets its own code");
    let staff_token = verify_two_factor(&app, staff_challenge, &staff_code).await;

    // 9. Creating a task as James Bond must be 403.
    let (status, body) = call_json(
        &app,
        test::TestRequest::post()
            .uri("/tasks")
            .insert_header(("Authorization", format!("Bearer {staff_token}")))
            .set_json(json!({ "title": "Not allowed", "priority": "high" }))
            .to_request(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // 10. view-my-tasks returns exactly 3 tasks, from the database.
    let (status, first) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(status, StatusCode::OK, "{first}");

    // The response shape is part of the contract: exactly these four keys, no
    // more and no fewer. Compared as a sorted set because `serde_json::Value`
    // stores object keys in a map and does not preserve the wire order.
    assert_eq!(
        sorted_keys(&first),
        ["cache", "summary", "tasks", "user"],
        "unexpected top-level shape: {first}"
    );

    assert_eq!(first["user"]["email"], STAFF_EMAIL);
    assert_eq!(first["user"]["role"], "staff");
    assert_eq!(first["summary"]["total_assigned_tasks"], 3);
    assert_eq!(first["cache"]["hit"], false);

    let tasks = first["tasks"].as_array().expect("tasks array");
    assert_eq!(tasks.len(), 3);
    for task in tasks {
        // Exactly the five fields the assignment specifies: no description and no
        // timestamps, and `assigned_to` is an email rather than `assigned_to_id`.
        assert_eq!(
            sorted_keys(task),
            ["assigned_to", "id", "priority", "status", "title"],
            "unexpected task shape: {task}"
        );
        assert_eq!(task["assigned_to"], STAFF_EMAIL);
        assert_eq!(task["status"], "todo");
        assert!(Uuid::parse_str(task["id"].as_str().expect("id")).is_ok());
    }
    // Priorities come back in creation order: high, medium, low.
    assert_eq!(tasks[0]["priority"], "high");
    assert_eq!(tasks[1]["priority"], "medium");
    assert_eq!(tasks[2]["priority"], "low");

    // 11. The same call again is served from cache.
    let (status, second) = view_my_tasks(&app, &staff_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["cache"]["hit"], true);
    assert_eq!(first["tasks"], second["tasks"]);

    // Printed so the exact payload can be pasted into the README.
    println!(
        "final GET /tasks/view-my-tasks response:\n{}",
        serde_json::to_string_pretty(&second).expect("serialise")
    );

    ctx.cleanup().await;
}

/// Object keys of a JSON value, sorted, for order-independent shape assertions.
fn sorted_keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .unwrap_or_else(|| panic!("expected a JSON object, got {value}"))
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// `POST /tasks/assign`, asserting success.
async fn assign<S, B>(app: &S, admin_token: &str, task_ids: &[Uuid], assignee_email: &str)
where
    S: Service<Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    let (status, body) = call_json(
        app,
        test::TestRequest::post()
            .uri("/tasks/assign")
            .insert_header(("Authorization", format!("Bearer {admin_token}")))
            .set_json(json!({ "task_ids": task_ids, "assignee_email": assignee_email }))
            .to_request(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "assignment failed: {body}");
}

#[actix_web::test]
async fn database_url_rewriting_preserves_credentials_and_options() {
    assert_eq!(
        common::with_database("postgres://u:p@localhost:5432/alkyne", "other"),
        "postgres://u:p@localhost:5432/other"
    );
    assert_eq!(
        common::with_database(
            "postgres://u:p@localhost:5432/alkyne?sslmode=require",
            "other"
        ),
        "postgres://u:p@localhost:5432/other?sslmode=require"
    );
    // No path component at all.
    assert_eq!(
        common::with_database("postgres://localhost", "other"),
        "postgres://localhost/other"
    );
}
