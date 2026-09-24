//! Use cases. Handlers parse and serialise; services decide.
//!
//! Everything a reviewer would call "the business rules" lives here: password and
//! two-factor verification, the admin-only write paths, and the cache
//! read-through/invalidation policy.

pub mod auth;
pub mod seed;
pub mod tasks;
