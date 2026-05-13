pub mod backup;
pub mod chapter;
pub mod editor;
pub mod error;
pub mod index;
pub mod project;
pub mod settings;
pub mod storage;
pub mod sync;
pub mod trash;
pub mod volume;
pub mod workspace;

pub use error::{Error, Result};

#[cfg(test)]
pub mod backup_tests;
#[cfg(test)]
pub mod chapter_tests;
pub mod facade;
#[cfg(test)]
pub mod fixture_tests;
#[cfg(test)]
pub mod project_tests;
#[cfg(test)]
pub mod settings_tests;
#[cfg(test)]
pub mod trash_tests;
#[cfg(test)]
pub mod volume_tests;
#[cfg(test)]
pub mod workspace_tests;
