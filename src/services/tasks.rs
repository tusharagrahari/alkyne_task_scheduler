//! Task creation, assignment, update, and the cached per-user task view.
//!
//! # Cache policy
//!
//! Exactly one function inserts into the cache — [`view_my_tasks`], on a miss.
//! [`create_task`], [`assign_tasks`] and [`update_task`] only *invalidate*, and
//! they invalidate both the new assignee and the previous one, so neither user
//! can be served a stale list. See [`crate::cache::TaskCache`] for why
//! write-through is deliberately avoided.

use uuid::Uuid;

use crate::domain::task::{AssignedTask, Task, TaskPriority, TaskStatus};
use crate::domain::user::{Role, User, normalize_email};
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::repo::tasks::{NewTask, TaskPatch};
use crate::security::AuthenticatedUser;
use crate::state::AppState;

/// Upper bound on a single assignment request, so one call cannot lock an
/// unbounded number of rows.
const MAX_TASKS_PER_ASSIGNMENT: usize = 50;
const MAX_TITLE_LEN: usize = 200;
const MAX_DESCRIPTION_LEN: usize = 2_000;

/// A task list plus whether it came from the cache.
#[derive(Debug, Clone)]
pub struct CachedTaskView {
    pub tasks: Vec<AssignedTask>,
    pub cache_hit: bool,
}

/// Result of an assignment.
#[derive(Debug, Clone)]
pub struct AssignmentOutcome {
    pub assignee: User,
    pub assigned_task_ids: Vec<Uuid>,
}

/// Creates a task owned by `admin`.
///
/// If an assignee is supplied at creation time, that user's cached list is
/// invalidated; otherwise there is nothing to invalidate, because an unassigned
/// task appears in nobody's view.
pub async fn create_task(
    state: &AppState,
    admin: &AuthenticatedUser,
    input: CreateTaskInput,
) -> AppResult<Task> {
    let title = validate_title(&input.title)?;
    let description = validate_description(input.description)?;

    let assignee = match input.assignee_email.as_deref() {
        Some(email) => Some(resolve_user_by_email(state, email).await?),
        None => None,
    };

    let task = repo::tasks::insert(
        &state.db,
        NewTask {
            title,
            description,
            status: input.status.unwrap_or_default(),
            priority: input.priority.unwrap_or_default(),
            created_by_id: admin.id,
            assigned_to_id: assignee.as_ref().map(|user| user.id),
        },
    )
    .await?;

    if let Some(assignee) = &assignee {
        state.cache.invalidate(assignee.id);
    }

    tracing::info!(task_id = %task.id, created_by = %admin.email, "created task");
    Ok(task)
}

/// Assigns every listed task to one user, atomically.
///
/// The whole request is one transaction: if any id does not exist, nothing is
/// assigned. A partially applied assignment would be worse than a rejected one —
/// the caller would have no way to tell which half succeeded.
pub async fn assign_tasks(
    state: &AppState,
    admin: &AuthenticatedUser,
    task_ids: &[Uuid],
    assignee_email: &str,
) -> AppResult<AssignmentOutcome> {
    let task_ids = validate_task_ids(task_ids)?;
    let assignee = resolve_user_by_email(state, assignee_email).await?;

    let mut tx = state.db.begin().await?;

    let existing = repo::tasks::lock_assignments(&mut tx, &task_ids).await?;
    if existing.len() != task_ids.len() {
        let found: Vec<Uuid> = existing.iter().map(|row| row.id).collect();
        let missing: Vec<String> = task_ids
            .iter()
            .filter(|id| !found.contains(id))
            .map(Uuid::to_string)
            .collect();

        // Dropping `tx` rolls back; no rows were modified in any case.
        return Err(AppError::validation(format!(
            "unknown task id(s): {}",
            missing.join(", ")
        )));
    }

    // Captured before the update: whoever held these tasks previously must also
    // have their cached list dropped.
    let previous_assignees: Vec<Uuid> = existing
        .iter()
        .filter_map(|row| row.assigned_to_id)
        .collect();

    let assigned_task_ids = repo::tasks::assign_many(&mut tx, &task_ids, assignee.id).await?;
    tx.commit().await?;

    // Invalidate only after the commit succeeds: dropping cache entries for a
    // transaction that then rolled back would cause needless database reloads.
    state.cache.invalidate(assignee.id);
    state.cache.invalidate_users(&previous_assignees);

    tracing::info!(
        assignee = %assignee.email,
        assigned = assigned_task_ids.len(),
        by = %admin.email,
        "assigned tasks"
    );

    Ok(AssignmentOutcome {
        assignee,
        assigned_task_ids,
    })
}

/// Applies a partial update to one task and invalidates every affected user.
pub async fn update_task(
    state: &AppState,
    admin: &AuthenticatedUser,
    task_id: Uuid,
    input: UpdateTaskInput,
) -> AppResult<Task> {
    let assignee = match input.assignee_email.as_deref() {
        Some(email) => Some(resolve_user_by_email(state, email).await?),
        None => None,
    };

    let patch = TaskPatch {
        title: input.title.as_deref().map(validate_title).transpose()?,
        description: validate_description(input.description)?,
        status: input.status,
        priority: input.priority,
        assigned_to_id: assignee.as_ref().map(|user| user.id),
    };

    if patch.is_empty() {
        return Err(AppError::validation(
            "provide at least one of title, description, status, priority, assignee_email",
        ));
    }

    let before = repo::tasks::find_by_id(&state.db, task_id)
        .await?
        .ok_or(AppError::not_found("task"))?;

    let updated = repo::tasks::update(&state.db, task_id, patch)
        .await?
        .ok_or(AppError::not_found("task"))?;

    // A status or priority change alters the cached payload for the current
    // assignee; a reassignment alters it for both the old and the new one.
    let affected: Vec<Uuid> = [before.assigned_to_id, updated.assigned_to_id]
        .into_iter()
        .flatten()
        .collect();
    state.cache.invalidate_users(&affected);

    tracing::info!(task_id = %updated.id, by = %admin.email, "updated task");
    Ok(updated)
}

/// Returns the caller's assigned tasks, reading through the per-user cache.
///
/// This is the only cache writer in the application.
pub async fn view_my_tasks(
    state: &AppState,
    caller: &AuthenticatedUser,
) -> AppResult<CachedTaskView> {
    if let Some(tasks) = state.cache.get(caller.id) {
        tracing::debug!(user_id = %caller.id, "serving assigned tasks from cache");
        return Ok(CachedTaskView {
            tasks,
            cache_hit: true,
        });
    }

    let tasks = repo::tasks::list_assigned_to(&state.db, caller.id).await?;
    state.cache.insert(caller.id, tasks.clone());

    tracing::debug!(
        user_id = %caller.id,
        count = tasks.len(),
        "loaded assigned tasks from database"
    );
    Ok(CachedTaskView {
        tasks,
        cache_hit: false,
    })
}

/// Every task in the system. Admin-only; used to discover ids to assign.
pub async fn list_all_tasks(state: &AppState, _admin: &AuthenticatedUser) -> AppResult<Vec<Task>> {
    Ok(repo::tasks::list_all(&state.db).await?)
}

/// Inputs accepted by [`create_task`], already decoded from the request body.
#[derive(Debug, Clone)]
pub struct CreateTaskInput {
    pub title: String,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub priority: Option<TaskPriority>,
    pub assignee_email: Option<String>,
}

/// Inputs accepted by [`update_task`].
#[derive(Debug, Clone, Default)]
pub struct UpdateTaskInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub priority: Option<TaskPriority>,
    pub assignee_email: Option<String>,
}

async fn resolve_user_by_email(state: &AppState, email: &str) -> AppResult<User> {
    let email = normalize_email(email);
    repo::users::find_by_email(&state.db, &email)
        .await?
        .ok_or_else(|| AppError::validation(format!("no user exists with email `{email}`")))
}

fn validate_title(title: &str) -> AppResult<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation("title must not be empty"));
    }
    if trimmed.chars().count() > MAX_TITLE_LEN {
        return Err(AppError::validation(format!(
            "title must be at most {MAX_TITLE_LEN} characters"
        )));
    }
    Ok(trimmed.to_owned())
}

fn validate_description(description: Option<String>) -> AppResult<Option<String>> {
    match description {
        None => Ok(None),
        Some(text) if text.chars().count() > MAX_DESCRIPTION_LEN => Err(AppError::validation(
            format!("description must be at most {MAX_DESCRIPTION_LEN} characters"),
        )),
        Some(text) => Ok(Some(text)),
    }
}

/// Rejects an empty or oversized list and removes duplicate ids, which would
/// otherwise make the "unknown id" count check compare unequal lengths.
fn validate_task_ids(task_ids: &[Uuid]) -> AppResult<Vec<Uuid>> {
    if task_ids.is_empty() {
        return Err(AppError::validation("task_ids must not be empty"));
    }
    if task_ids.len() > MAX_TASKS_PER_ASSIGNMENT {
        return Err(AppError::validation(format!(
            "at most {MAX_TASKS_PER_ASSIGNMENT} tasks can be assigned in one request"
        )));
    }

    let mut unique: Vec<Uuid> = Vec::with_capacity(task_ids.len());
    for &id in task_ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    Ok(unique)
}

/// Admin-only guard used where a handler takes [`AuthenticatedUser`] but the
/// operation is still restricted. Handlers that are admin-only take
/// [`crate::security::AdminUser`] instead and never need this.
pub fn ensure_admin(user: &AuthenticatedUser) -> AppResult<()> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(AppError::InsufficientRole {
            required: Role::Admin,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_trimmed_and_required() {
        assert_eq!(validate_title("  Brief M  ").expect("valid"), "Brief M");
        assert_eq!(
            validate_title("   ").expect_err("empty").code(),
            "validation_error"
        );
        let too_long = "x".repeat(MAX_TITLE_LEN + 1);
        assert_eq!(
            validate_title(&too_long).expect_err("too long").code(),
            "validation_error"
        );
    }

    #[test]
    fn task_ids_are_deduplicated() {
        let id = Uuid::new_v4();
        let other = Uuid::new_v4();

        let unique = validate_task_ids(&[id, other, id]).expect("valid");
        assert_eq!(unique, vec![id, other]);
    }

    #[test]
    fn empty_and_oversized_task_id_lists_are_rejected() {
        assert!(validate_task_ids(&[]).is_err());

        let too_many: Vec<Uuid> = (0..=MAX_TASKS_PER_ASSIGNMENT)
            .map(|_| Uuid::new_v4())
            .collect();
        assert!(validate_task_ids(&too_many).is_err());
    }
}
