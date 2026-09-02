//! Git metadata finalize module — re-export from `git/` for backward compatibility.
//!
//! New code should depend on `crate::sync::git::*` directly.

pub use super::git::model::*;
pub use super::git::finalize::*;
pub use super::git::rollback::*;
pub use super::git::locks::*;
