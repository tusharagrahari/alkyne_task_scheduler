use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::domain::task::{AssignedTask, Task, TaskPriority, TaskStatus};

const TASK_COLUMNS: &str = "id, title, description, status, priority, \
                            created_by_id, assigned_to_id, created_at, updated_at";

/// Values needed to create a task row.
#[derive(Debug, Clone)]
pub struct NewTask {
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub created_by_id: Uuid,
    pub assigned_to_id: Option<Uuid>,
}

/// Partial update. `None` leaves a column untouched.
///
/// Because absent fields are expressed with `COALESCE`, this cannot *clear*
/// `description` or unassign a task; both are out of scope for the assignment and
/// noted in the README.
#[derive(Debug, Clone, Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub priority: Option<TaskPriority>,
    pub assigned_to_id: Option<Uuid>,
}

impl TaskPatch {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.description.is_none()
            && self.status.is_none()
            && self.priority.is_none()
            && self.assigned_to_id.is_none()
    }
}

/// Current assignee of a task, read before an update so the caller knows which
/// user caches to invalidate.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskAssignment {
    pub id: Uuid,
    pub assigned_to_id: Option<Uuid>,
}

pub async fn insert(db: &PgPool, task: NewTask) -> Result<Task, sqlx::Error> {
    let sql = format!(
        "INSERT INTO tasks
             (id, title, description, status, priority, created_by_id, assigned_to_id)
         VALUES ($1, $2, $3, $4::task_status, $5::task_priority, $6, $7)
         RETURNING {TASK_COLUMNS}"
    );

    sqlx::query_as::<_, Task>(&sql)
        .bind(Uuid::new_v4())
        .bind(task.title)
        .bind(task.description)
        .bind(task.status)
        .bind(task.priority)
        .bind(task.created_by_id)
        .bind(task.assigned_to_id)
        .fetch_one(db)
        .await
}

pub async fn find_by_id(db: &PgPool, id: Uuid) -> Result<Option<Task>, sqlx::Error> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = $1");

    sqlx::query_as::<_, Task>(&sql)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Every task, newest last. Admin-only listing, handy for picking ids to assign.
pub async fn list_all(db: &PgPool) -> Result<Vec<Task>, sqlx::Error> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM tasks ORDER BY created_at ASC, id ASC");

    sqlx::query_as::<_, Task>(&sql).fetch_all(db).await
}

pub async fn count_created_by(db: &PgPool, user_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM tasks WHERE created_by_id = $1")
        .bind(user_id)
        .fetch_one(db)
        .await
}

/// The read model behind `GET /tasks/view-my-tasks`.
///
/// Joins `users` so the response can carry the assignee's **email** in
/// `assigned_to`, which is what the expected response specifies — the column on
/// `tasks` is a UUID. Ordering is explicit so a cached payload and a freshly
/// loaded one are byte-identical.
pub async fn list_assigned_to(
    db: &PgPool,
    user_id: Uuid,
) -> Result<Vec<AssignedTask>, sqlx::Error> {
    sqlx::query_as::<_, AssignedTask>(
        "SELECT t.id,
                t.title,
                t.status,
                t.priority,
                assignee.email AS assigned_to
           FROM tasks AS t
           JOIN users AS assignee ON assignee.id = t.assigned_to_id
          WHERE t.assigned_to_id = $1
          ORDER BY t.created_at ASC, t.id ASC",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// Locks the given tasks and reports their current assignees.
///
/// `FOR UPDATE` serialises concurrent assignments of the same task, so the
/// "previous assignee" this returns is still accurate when the caller
/// invalidates that user's cache.
pub async fn lock_assignments(
    conn: &mut PgConnection,
    task_ids: &[Uuid],
) -> Result<Vec<TaskAssignment>, sqlx::Error> {
    sqlx::query_as::<_, TaskAssignment>(
        "SELECT id, assigned_to_id
           FROM tasks
          WHERE id = ANY($1)
          FOR UPDATE",
    )
    .bind(task_ids)
    .fetch_all(&mut *conn)
    .await
}

/// Points every listed task at `assignee_id`, returning the ids actually updated.
pub async fn assign_many(
    conn: &mut PgConnection,
    task_ids: &[Uuid],
    assignee_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(
        "UPDATE tasks
            SET assigned_to_id = $2,
                updated_at     = now()
          WHERE id = ANY($1)
          RETURNING id",
    )
    .bind(task_ids)
    .bind(assignee_id)
    .fetch_all(&mut *conn)
    .await
}

/// Applies a partial update. `Ok(None)` means no task with that id exists.
pub async fn update(db: &PgPool, id: Uuid, patch: TaskPatch) -> Result<Option<Task>, sqlx::Error> {
    let sql = format!(
        "UPDATE tasks
            SET title          = COALESCE($2::text, title),
                description    = COALESCE($3::text, description),
                status         = COALESCE($4::task_status, status),
                priority       = COALESCE($5::task_priority, priority),
                assigned_to_id = COALESCE($6::uuid, assigned_to_id),
                updated_at     = now()
          WHERE id = $1
          RETURNING {TASK_COLUMNS}"
    );

    sqlx::query_as::<_, Task>(&sql)
        .bind(id)
        .bind(patch.title)
        .bind(patch.description)
        .bind(patch.status)
        .bind(patch.priority)
        .bind(patch.assigned_to_id)
        .fetch_optional(db)
        .await
}
