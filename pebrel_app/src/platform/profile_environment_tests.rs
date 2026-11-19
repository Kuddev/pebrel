//! Host-specific PTY fixtures remain workspace children to exercise private launch paths.

use super::*;

#[cfg(unix)]
#[path = "profile_environment_tests/unix.rs"]
mod unix;
