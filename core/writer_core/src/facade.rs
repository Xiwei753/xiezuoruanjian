use crate::chapter::{self, ChapterContent};
use crate::error::Result;
use crate::project::{self, Project};
use crate::workspace;
use std::path::{Path, PathBuf};

/// The main entry point for client applications (Android, Linux).
/// This struct holds the workspace root and provides high-level methods.
pub struct WriterCore {
    workspace_path: PathBuf,
}

impl WriterCore {
    /// Initialize the core with a workspace root directory.
    pub fn new<P: AsRef<Path>>(workspace_path: P) -> Self {
        Self {
            workspace_path: workspace_path.as_ref().to_path_buf(),
        }
    }

    /// Read the workspace manifest.
    pub fn validate_workspace(&self) -> Result<bool> {
        workspace::validate_workspace(&self.workspace_path)
    }

    /// List all projects in the workspace.
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        project::list_projects(&self.workspace_path)
    }

    /// Read a specific project's manifest (This requires a volume lookup, simplifying to just reading a chapter).
    pub fn read_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<ChapterContent> {
        chapter::read_chapter(&self.workspace_path, project_id, volume_id, chapter_id)
    }

    /// Write content to a chapter (atomic).
    pub fn write_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> Result<()> {
        chapter::save_chapter(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
            content,
        )
    }
}
