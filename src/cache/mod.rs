//! Per-user caching of the `GET /tasks/view-my-tasks` payload.

pub mod memory;

pub use memory::InMemoryTaskCache;
use uuid::Uuid;

use crate::domain::task::AssignedTask;

/// Cache of assigned-task lists, keyed by user id.
///
/// # Invariant: only the read path populates
///
/// [`TaskCache::insert`] is called from exactly one place — the cache **miss**
/// branch of `view_my_tasks`. Write paths (task creation, assignment, update)
/// must only [`TaskCache::invalidate`], never warm the cache. If assignment
/// wrote through, the first `view-my-tasks` call would report
/// `cache.hit = true`, which is precisely the behaviour the assignment's
/// validation step checks for.
///
/// The trait exists to keep that substitution point explicit: a Redis-backed
/// implementation would replace [`InMemoryTaskCache`] without touching the
/// service layer. See the caching section of the README for the limitations of
/// the in-memory implementation shipped here.
pub trait TaskCache: Send + Sync + std::fmt::Debug {
    /// Returns the cached payload if present and not expired.
    fn get(&self, user_id: Uuid) -> Option<Vec<AssignedTask>>;

    /// Stores a freshly loaded payload. Read path only — see the trait docs.
    fn insert(&self, user_id: Uuid, tasks: Vec<AssignedTask>);

    /// Drops the entry for one user. Safe to call when nothing is cached.
    fn invalidate(&self, user_id: Uuid);

    /// Drops the entries for several users, de-duplicating ids.
    fn invalidate_users(&self, user_ids: &[Uuid]) {
        let mut seen = Vec::with_capacity(user_ids.len());
        for &user_id in user_ids {
            if !seen.contains(&user_id) {
                seen.push(user_id);
                self.invalidate(user_id);
            }
        }
    }

    /// Number of live entries. Used by tests and useful for diagnostics.
    fn entry_count(&self) -> usize;
}
