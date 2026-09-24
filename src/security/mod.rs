//! Password hashing, JWT issuing/verification, and the request extractors that
//! turn a bearer token into an authenticated (optionally admin) caller.

pub mod extractors;
pub mod jwt;
pub mod password;

pub use extractors::{AdminUser, AuthenticatedUser};
pub use jwt::{Claims, IssuedToken, JwtService};
