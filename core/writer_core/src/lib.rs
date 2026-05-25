pub(crate) mod delete_guard;
pub mod action_registry;
// Always export these for UniFFI
pub mod ai_service;
pub use crate::ai_service::{AiActionResponse, AiAction, AiActionType};
pub mod app_config;
pub mod graph_service;
pub mod proofreading_service;
pub mod settings_registry;
pub mod sync_service;

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
pub mod mind_map;
pub mod writing_stats;
pub mod starmap;

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
#[cfg(test)]
pub mod writing_stats_tests;

pub fn perform_dummy_action() -> String {
    "hello from uniffi".to_string()
}

uniffi::include_scaffolding!("api");
