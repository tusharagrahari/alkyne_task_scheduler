//! Process-local [`TaskCache`] implementation.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::TaskCache;
use crate::domain::task::AssignedTask;

/// An in-memory, TTL'd cache guarded by a `RwLock`.
///
/// Reads take the shared lock, so concurrent `view-my-tasks` requests do not
/// serialise against each other. Entries expire lazily: an expired entry is
/// removed on the next read that touches it rather than by a sweeper task, which
/// keeps the type free of background work at the cost of holding expired
/// payloads until they are next looked up.
#[derive(Debug)]
pub struct InMemoryTaskCache {
    ttl: Duration,
    entries: RwLock<HashMap<Uuid, Entry>>,
}

#[derive(Debug, Clone)]
struct Entry {
    tasks: Vec<AssignedTask>,
    expires_at: Instant,
}

impl InMemoryTaskCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: RwLock::new(HashMap::new()),
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// A poisoned lock means another thread panicked while holding it. The cache
    /// holds no invariant that a panic could have broken — it is a plain map of
    /// derived data — so recovering the guard is safe and strictly better than
    /// propagating the panic to every later request.
    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<Uuid, Entry>> {
        self.entries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<Uuid, Entry>> {
        self.entries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl TaskCache for InMemoryTaskCache {
    fn get(&self, user_id: Uuid) -> Option<Vec<AssignedTask>> {
        let now = Instant::now();

        // Fast path: shared lock only.
        {
            let entries = self.read();
            match entries.get(&user_id) {
                Some(entry) if entry.expires_at > now => return Some(entry.tasks.clone()),
                None => return None,
                Some(_) => {} // Expired — fall through and evict.
            }
        }

        // Slow path: the entry was expired when we looked. Re-check under the
        // exclusive lock, because another thread may have refreshed it meanwhile.
        let mut entries = self.write();
        match entries.get(&user_id) {
            Some(entry) if entry.expires_at > Instant::now() => Some(entry.tasks.clone()),
            Some(_) => {
                entries.remove(&user_id);
                None
            }
            None => None,
        }
    }

    fn insert(&self, user_id: Uuid, tasks: Vec<AssignedTask>) {
        let entry = Entry {
            tasks,
            expires_at: Instant::now() + self.ttl,
        };
        self.write().insert(user_id, entry);
    }

    fn invalidate(&self, user_id: Uuid) {
        if self.write().remove(&user_id).is_some() {
            tracing::debug!(%user_id, "invalidated cached task list");
        }
    }

    fn entry_count(&self) -> usize {
        self.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::task::{TaskPriority, TaskStatus};

    fn sample_task(title: &str) -> AssignedTask {
        AssignedTask {
            id: Uuid::new_v4(),
            title: title.to_owned(),
            status: TaskStatus::Todo,
            priority: TaskPriority::High,
            assigned_to: "jamesbond@example.com".into(),
        }
    }

    #[test]
    fn miss_then_hit() {
        let cache = InMemoryTaskCache::new(Duration::from_secs(60));
        let user_id = Uuid::new_v4();

        assert!(cache.get(user_id).is_none(), "empty cache must miss");

        let tasks = vec![sample_task("Prepare briefing")];
        cache.insert(user_id, tasks.clone());

        assert_eq!(cache.get(user_id), Some(tasks));
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn entries_are_isolated_per_user() {
        let cache = InMemoryTaskCache::new(Duration::from_secs(60));
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());

        cache.insert(first, vec![sample_task("Only mine")]);

        assert!(cache.get(first).is_some());
        assert!(cache.get(second).is_none(), "cache must be keyed per user");
    }

    #[test]
    fn invalidate_removes_one_user_only() {
        let cache = InMemoryTaskCache::new(Duration::from_secs(60));
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
        cache.insert(first, vec![sample_task("a")]);
        cache.insert(second, vec![sample_task("b")]);

        cache.invalidate(first);

        assert!(cache.get(first).is_none());
        assert!(cache.get(second).is_some());
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn invalidate_users_deduplicates_and_tolerates_unknown_ids() {
        let cache = InMemoryTaskCache::new(Duration::from_secs(60));
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
        cache.insert(first, vec![sample_task("a")]);

        cache.invalidate_users(&[first, first, second]);

        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn entries_expire_after_ttl() {
        let cache = InMemoryTaskCache::new(Duration::from_millis(20));
        let user_id = Uuid::new_v4();
        cache.insert(user_id, vec![sample_task("short lived")]);

        assert!(cache.get(user_id).is_some());
        std::thread::sleep(Duration::from_millis(40));

        assert!(cache.get(user_id).is_none(), "entry must expire");
        assert_eq!(cache.entry_count(), 0, "expired entry must be evicted");
    }
}
