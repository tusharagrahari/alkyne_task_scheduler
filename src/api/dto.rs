//! Wire types.
//!
//! Responses are built from explicit DTOs rather than by serialising domain
//! entities, which keeps `hashed_password` and internal columns out of response
//! bodies and makes the contract visible in one file.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::task::{AssignedTask, Task, TaskPriority, TaskStatus};
use crate::domain::user::{Role, User};

// ---------------------------------------------------------------- users / seed

#[derive(Debug, Clone, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub full_name: String,
    pub email: String,
    pub role: Role,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&User> for UserResponse {
    fn from(user: &User) -> Self {
        Self {
            id: user.id,
            full_name: user.full_name.clone(),
            email: user.email.clone(),
            role: user.role,
            created_at: user.created_at,
            updated_at: user.updated_at,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedUsersRequest {
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub admin_full_name: Option<String>,
    pub staff_email: Option<String>,
    pub staff_password: Option<String>,
    pub staff_full_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeedUsersResponse {
    pub users: Vec<UserResponse>,
    /// Restated so a reviewer never has to guess which password to log in with.
    pub credentials: Vec<SeededCredential>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeededCredential {
    pub email: String,
    pub password: String,
    pub role: Role,
}

// ------------------------------------------------------------------------ auth

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Step one of login. Carries **no** token by design: a JWT is only issued after
/// the emailed code is verified.
#[derive(Debug, Clone, Serialize)]
pub struct LoginChallengeResponse {
    pub login_challenge_id: Uuid,
    pub two_factor_required: bool,
    pub expires_at: DateTime<Utc>,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyTwoFactorRequest {
    pub login_challenge_id: Uuid,
    pub code: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccessTokenResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub expires_at: DateTime<Utc>,
    pub user: UserResponse,
}

// ----------------------------------------------------------------------- tasks

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTaskRequest {
    pub title: String,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub priority: Option<TaskPriority>,
    /// Optional shortcut: create and assign in one call.
    pub assignee_email: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub priority: Option<TaskPriority>,
    pub assignee_email: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssignTasksRequest {
    pub task_ids: Vec<Uuid>,
    pub assignee_email: String,
}

/// Full task representation, used by the admin-facing endpoints.
#[derive(Debug, Clone, Serialize)]
pub struct TaskResponse {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub created_by_id: Uuid,
    pub assigned_to_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&Task> for TaskResponse {
    fn from(task: &Task) -> Self {
        Self {
            id: task.id,
            title: task.title.clone(),
            description: task.description.clone(),
            status: task.status,
            priority: task.priority,
            created_by_id: task.created_by_id,
            assigned_to_id: task.assigned_to_id,
            created_at: task.created_at,
            updated_at: task.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskListResponse {
    pub tasks: Vec<TaskResponse>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignTasksResponse {
    pub assigned_count: usize,
    pub assignee: UserResponse,
    pub task_ids: Vec<Uuid>,
}

/// Response body for `GET /tasks/view-my-tasks`.
///
/// This shape is fixed by the assignment: `user`, `tasks`, `summary.total_assigned_tasks`
/// and `cache.hit`, with each task carrying exactly `id`, `title`, `status`,
/// `priority` and `assigned_to`. It is kept separate from [`TaskResponse`] so
/// adding a field there can never change this contract.
#[derive(Debug, Clone, Serialize)]
pub struct MyTasksResponse {
    pub user: MyTasksUser,
    pub tasks: Vec<AssignedTask>,
    pub summary: MyTasksSummary,
    pub cache: CacheMetadata,
}

#[derive(Debug, Clone, Serialize)]
pub struct MyTasksUser {
    pub email: String,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize)]
pub struct MyTasksSummary {
    pub total_assigned_tasks: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheMetadata {
    pub hit: bool,
}

// ------------------------------------------------------------------------- dev

#[derive(Debug, Clone, Serialize)]
pub struct EmailLogResponse {
    pub id: Uuid,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub verification_code: Option<String>,
    pub login_challenge_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl From<&crate::domain::email::EmailLog> for EmailLogResponse {
    fn from(log: &crate::domain::email::EmailLog) -> Self {
        Self {
            id: log.id,
            to: log.to_email.clone(),
            subject: log.subject.clone(),
            body: log.body.clone(),
            verification_code: log.verification_code.clone(),
            login_challenge_id: log.login_challenge_id,
            created_at: log.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EmailLogListResponse {
    pub emails: Vec<EmailLogResponse>,
    pub total: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmailLogQuery {
    /// Narrows the lookup to one recipient — useful once both users have
    /// requested a code.
    pub email: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub database: &'static str,
    pub cached_task_lists: usize,
}
