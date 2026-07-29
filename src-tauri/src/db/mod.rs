//! Database client: discover MySQL/MariaDB servers on a host, reach them through an SSH
//! tunnel, and browse/query them.
//!
//! Reaching a database this way means the user never has to expose port 3306 to the
//! internet, and never has to publish a container port just to look at its data — the
//! forward is resolved on the SSH server's side of the connection. See
//! [`crate::ssh::tunnel`].

pub mod discover;
pub mod query;
