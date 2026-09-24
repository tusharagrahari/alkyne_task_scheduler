use actix_web::{HttpResponse, get, patch, post, web};
use uuid::Uuid;

use crate::api::dto::{
    AssignTasksRequest, AssignTasksResponse, CacheMetadata, CreateTaskRequest, MyTasksResponse,
    MyTasksSummary, MyTasksUser, TaskListResponse, TaskResponse, UpdateTaskRequest, UserResponse,
};
use crate::error::AppResult;
use crate::security::{AdminUser, AuthenticatedUser};
use crate::services::tasks::{self, CreateTaskInput, UpdateTaskInput};
use crate::state::AppState;

/// `POST /tasks` — create a task. **Admin only.**
///
/// The `AdminUser` extractor is the whole authorisation check: a staff token
/// never reaches this body, which is why James Bond gets `403 Forbidden` here.
#[post("")]
pub async fn create_task(
    state: web::Data<AppState>,
    admin: AdminUser,
    body: web::Json<CreateTaskRequest>,
) -> AppResult<HttpResponse> {
    let body = body.into_inner();

    let task = tasks::create_task(
        &state,
        &admin,
        CreateTaskInput {
            title: body.title,
            description: body.description,
            status: body.status,
            priority: body.priority,
            assignee_email: body.assignee_email,
        },
    )
    .await?;

    Ok(HttpResponse::Created().json(TaskResponse::from(&task)))
}

/// `POST /tasks/assign` — assign tasks to a user. **Admin only.**
#[post("/assign")]
pub async fn assign_tasks(
    state: web::Data<AppState>,
    admin: AdminUser,
    body: web::Json<AssignTasksRequest>,
) -> AppResult<HttpResponse> {
    let body = body.into_inner();

    let outcome = tasks::assign_tasks(&state, &admin, &body.task_ids, &body.assignee_email).await?;

    Ok(HttpResponse::Ok().json(AssignTasksResponse {
        assigned_count: outcome.assigned_task_ids.len(),
        assignee: UserResponse::from(&outcome.assignee),
        task_ids: outcome.assigned_task_ids,
    }))
}

/// `GET /tasks` — every task. **Admin only.** Convenience for picking ids to assign.
#[get("")]
pub async fn list_tasks(state: web::Data<AppState>, admin: AdminUser) -> AppResult<HttpResponse> {
    let all = tasks::list_all_tasks(&state, &admin).await?;
    let tasks: Vec<TaskResponse> = all.iter().map(TaskResponse::from).collect();

    Ok(HttpResponse::Ok().json(TaskListResponse {
        total: tasks.len(),
        tasks,
    }))
}

/// `PATCH /tasks/{id}` — partial update. **Admin only.**
///
/// Exists so cache invalidation on *update* (not just assignment) is observable.
#[patch("/{id}")]
pub async fn update_task(
    state: web::Data<AppState>,
    admin: AdminUser,
    path: web::Path<Uuid>,
    body: web::Json<UpdateTaskRequest>,
) -> AppResult<HttpResponse> {
    let body = body.into_inner();

    let task = tasks::update_task(
        &state,
        &admin,
        path.into_inner(),
        UpdateTaskInput {
            title: body.title,
            description: body.description,
            status: body.status,
            priority: body.priority,
            assignee_email: body.assignee_email,
        },
    )
    .await?;

    Ok(HttpResponse::Ok().json(TaskResponse::from(&task)))
}

/// `GET /tasks/view-my-tasks` — the caller's own assigned tasks.
///
/// The assignment's primary validation point. Any authenticated role may call it,
/// and it only ever returns tasks whose `assigned_to_id` is the caller: a staff
/// user cannot see anyone else's work. `cache.hit` reports whether the payload
/// came from the per-user cache — `false` on the first call, `true` on the next.
#[get("/view-my-tasks")]
pub async fn view_my_tasks(
    state: web::Data<AppState>,
    caller: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    let view = tasks::view_my_tasks(&state, &caller).await?;

    Ok(HttpResponse::Ok().json(MyTasksResponse {
        user: MyTasksUser {
            email: caller.email.clone(),
            role: caller.role,
        },
        summary: MyTasksSummary {
            total_assigned_tasks: view.tasks.len(),
        },
        tasks: view.tasks,
        cache: CacheMetadata {
            hit: view.cache_hit,
        },
    }))
}
