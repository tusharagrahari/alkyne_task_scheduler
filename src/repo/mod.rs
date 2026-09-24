//! Data access. Every SQL statement in the application lives under this module,
//! which keeps the service layer free of query strings and makes the set of
//! database operations easy to audit.
//!
//! Queries use the runtime-checked `sqlx::query_as` family rather than the
//! `query!` macros. The macros verify SQL against a live database *at compile
//! time*, which would make `cargo build` depend on a reachable PostgreSQL (or on
//! a committed `.sqlx` cache that must be regenerated on every schema change).
//! Keeping the build hermetic is worth more here than compile-time SQL checking,
//! and the integration tests exercise every statement against a real database.
//!
//! Enum parameters are written with an explicit cast (`$1::user_role`) so
//! PostgreSQL resolves the type from the statement rather than from inference.

pub mod challenges;
pub mod email_logs;
pub mod tasks;
pub mod users;
