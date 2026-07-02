//! # Writer Core - ???????
//!
//! ? crate ??????**??????**(Single Source of Truth)?
//! ???? I/O???????????????????????
//!
//! ## ????
//!
//! - **???? UI ??**:????????????????????????
//! - **???????**:Android / Desktop ????? Facade(`facade.rs`)?? Core?
//! - **????????**:???????????,??? HTML?
//!
//! ## ????
//!
//! | ?? | ?? | ?? |
//! |------|------|------|
//! | `facade` | Core ??????,??????? | ??????????? |
//! | `api` | ????? API ?,?? DTO / Error / Service | UniFFI?Linux binding???????? |
//! | `workspace` | ????????????? | ??????? |
//! | `project` | ?? CRUD????????? | ??? `delete_guard` |
//! | `volume` | ? CRUD?????? | ??? `delete_guard` |
//! | `chapter` | ?? CRUD?????????? | ???????? |
//! | `settings` | ???? & ????? | ?? LocalSettings / SyncableSettings |
//! | `sync` | ??????????????????(??? sync) | ?????? |

//! | `starmap` | ??(????????)- ?????? | ???????? |
//! | `writing_stats` | ????(??????????) | ???/??/???? |
//! | `error` | ?????? | ?????? |
//! | `storage` | ????????(????? + fsync ???? + rename) | ?????;???????????? |
//! | `delete_guard` | ??????(?? ID?????????) | ????????????? |
//!
//! ## ??????
//!
//! ```text
//! ??? ? facade::WriterCore::create_chapter()
//!         ? chapter::create_chapter()
//!         ? storage::atomic_write_string()
//! ```

pub mod action_registry;
pub mod api;
pub(crate) mod delete_guard;
// Always export these for UniFFI
pub mod ai_service;
pub use crate::ai_service::{AiAction, AiActionResponse, AiActionType};
pub mod app_config;
pub mod settings_registry;

pub mod chapter;
pub mod editor;
pub mod error;
pub mod history;
pub mod index;

#[cfg(feature = "harmony-ffi")]
pub mod ffi;
pub mod layout_policy;
pub mod project;
pub mod screen_policy;
pub mod settings;
pub mod settings_presentation;
pub mod starmap;
pub mod storage;
pub mod sync;
pub mod trash;
pub mod volume;
pub mod workspace;
pub mod writing_stats;

pub use api::*;
pub use error::{Error, Result};

#[cfg(test)]
pub mod chapter_tests;
#[cfg(test)]
pub mod dto_contract_tests;
pub mod facade;
#[cfg(test)]
pub mod index_tests;
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

use std::path::Path;

pub fn init_workspace(path: String) -> std::result::Result<bool, WriterError> {
    let p = Path::new(&path);
    crate::workspace::create_workspace(p).map_err(WriterError::from)?;

    Ok(true)
}

pub fn open_workspace(
    path: String,
) -> std::result::Result<std::sync::Arc<WriterAppService>, WriterError> {
    let p = Path::new(&path);
    if !crate::workspace::validate_workspace(p).map_err(WriterError::from)? {
        return Err(WriterError::InvalidWorkspace);
    }
    Ok(std::sync::Arc::new(WriterAppService::new(path)))
}

pub fn repair_workspace(path: String) -> std::result::Result<bool, WriterError> {
    let p = Path::new(&path);
    crate::workspace::create_workspace(p).map_err(WriterError::from)?;
    Ok(true)
}

pub fn create_project_in_workspace(
    workspace: String,
    title: String,
) -> std::result::Result<ProjectDto, WriterError> {
    let p = Path::new(&workspace);
    let project = crate::project::create_project(p, &title).map_err(WriterError::from)?;
    Ok(project.into())
}

pub fn load_workspace_summary(
    path: String,
) -> std::result::Result<WorkspaceSummaryDto, WriterError> {
    let p = Path::new(&path);
    let is_valid = crate::workspace::validate_workspace(p).unwrap_or(false);

    let projects = if is_valid {
        crate::project::list_projects(p)
            .map(|v| v.into_iter().map(Into::into).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let recent_edits = if is_valid {
        crate::workspace::get_recent_edits(p)
            .map(|v| v.into_iter().map(Into::into).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(WorkspaceSummaryDto {
        path,
        is_valid,
        projects,
        recent_edits,
    })
}

uniffi::include_scaffolding!("api");
pub mod app_service;
pub use app_service::WriterAppService;
