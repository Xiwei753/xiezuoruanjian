use crate::backup;
use crate::chapter::{self, Chapter, ChapterContent};
use crate::error::Result;
use crate::index;
use crate::project::{self, Project};
use crate::settings::{self, LocalSettings, SyncableSettings};
use crate::sync;
use crate::trash;
use crate::volume::{self, Volume};
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

    /// Create a new workspace.
    pub fn create_workspace(&self) -> Result<()> {
        workspace::create_workspace(&self.workspace_path)
    }

    /// Read the workspace manifest.
    pub fn validate_workspace(&self) -> Result<bool> {
        workspace::validate_workspace(&self.workspace_path)
    }

    /// List all projects in the workspace.
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        project::list_projects(&self.workspace_path)
    }

    /// Create a new project.
    pub fn create_project(&self, title: &str) -> Result<Project> {
        project::create_project(&self.workspace_path, title)
    }

    /// List volumes in a project.
    pub fn list_volumes(&self, project_id: &str) -> Result<Vec<Volume>> {
        volume::list_volumes(&self.workspace_path, project_id)
    }

    /// Create a new volume.
    pub fn create_volume(&self, project_id: &str, title: &str) -> Result<Volume> {
        volume::create_volume(&self.workspace_path, project_id, title)
    }

    /// List chapters in a volume.
    pub fn list_chapters(&self, project_id: &str, volume_id: &str) -> Result<Vec<Chapter>> {
        chapter::list_chapters(&self.workspace_path, project_id, volume_id)
    }

    /// Create a new chapter.
    pub fn create_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        title: &str,
    ) -> Result<Chapter> {
        chapter::create_chapter(&self.workspace_path, project_id, volume_id, title)
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

    /// Load local settings.
    pub fn load_local_settings(&self) -> Result<LocalSettings> {
        settings::load_local_settings(&self.workspace_path)
    }

    /// Save local settings.
    pub fn save_local_settings(&self, settings: &LocalSettings) -> Result<()> {
        settings::save_local_settings(&self.workspace_path, settings)
    }

    /// Load syncable settings.
    pub fn load_syncable_settings(&self) -> Result<SyncableSettings> {
        settings::load_syncable_settings(&self.workspace_path)
    }

    /// Save syncable settings.
    pub fn save_syncable_settings(&self, settings: &SyncableSettings) -> Result<()> {
        settings::save_syncable_settings(&self.workspace_path, settings)
    }

    /// Backup project
    pub fn backup_project(&self, project_id: &str) -> Result<()> {
        backup::backup_project(&self.workspace_path, project_id)
    }

    /// Move chapter to trash
    pub fn move_chapter_to_trash(&self, chapter_id: &str) -> Result<()> {
        trash::move_chapter_to_trash(&self.workspace_path, chapter_id)
    }

    /// Update index
    pub fn update_index(&self) -> Result<()> {
        index::update_index()
    }

    /// Sync workspace
    pub fn sync_workspace(&self) -> Result<()> {
        sync::sync_workspace()
    }

    // --- Settings Registry ---
    pub fn list_registered_settings(&self) -> crate::settings_registry::SettingsRegistry {
        crate::settings_registry::SettingsRegistry::default_registry()
    }

    // --- AI Service ---
    pub fn build_ai_context(
        &self,
        reference: crate::ai_service::AiContextReference,
    ) -> crate::error::Result<String> {
        let ai = crate::ai_service::AiService::new();
        ai.build_ai_context(reference)
    }

    pub fn get_ai_request_payload(
        &self,
        conversation: &crate::ai_service::AiConversation,
        tools: Option<Vec<crate::ai_service::AiToolDefinition>>,
    ) -> crate::error::Result<serde_json::Value> {
        let ai = crate::ai_service::AiService::new();
        ai.get_ai_request_payload(conversation, tools)
    }

    // --- Graph Service ---
    pub fn load_graph(
        &self,
        project_id: Option<&str>,
    ) -> crate::error::Result<crate::graph_service::GraphDocument> {
        let graph = crate::graph_service::GraphService::new(&self.workspace_path);
        graph.load_graph(project_id)
    }

    pub fn save_graph(
        &self,
        project_id: Option<&str>,
        doc: &crate::graph_service::GraphDocument,
    ) -> crate::error::Result<()> {
        let graph = crate::graph_service::GraphService::new(&self.workspace_path);
        graph.save_graph(project_id, doc)
    }

    // --- Proofreading Service ---
    pub fn proofread_text(
        &self,
        text: &str,
    ) -> crate::error::Result<Vec<crate::proofreading_service::ProofreadingSuggestion>> {
        let pr = crate::proofreading_service::ProofreadingService::new();
        pr.proofread(text)
    }

    // --- Sync Service ---
    pub fn scan_sync_files(&self) -> crate::error::Result<Vec<crate::sync_service::SyncFileEntry>> {
        crate::sync_service::SyncService::scan_workspace_for_sync(&self.workspace_path)
    }

    pub fn build_sync_plan_from_workspace(
        &self,
    ) -> crate::error::Result<crate::sync_service::SyncPlan> {
        crate::sync_service::SyncService::build_sync_plan_from_workspace(&self.workspace_path)
    }

    pub fn load_sync_state(&self) -> crate::error::Result<crate::sync_service::SyncState> {
        crate::sync_service::SyncService::load_sync_state(&self.workspace_path)
    }

    pub fn save_sync_state(
        &self,
        state: &crate::sync_service::SyncState,
    ) -> crate::error::Result<()> {
        crate::sync_service::SyncService::save_sync_state(&self.workspace_path, state)
    }

    pub fn record_sync_conflict(
        &self,
        conflict: crate::sync_service::SyncConflict,
        local_content: Option<&str>,
    ) -> crate::error::Result<()> {
        crate::sync_service::SyncService::record_sync_conflict(
            &self.workspace_path,
            conflict,
            local_content,
        )
    }

    pub fn get_sync_ignored_paths(&self) -> crate::error::Result<Vec<String>> {
        crate::sync_service::SyncService::get_sync_ignored_paths(&self.workspace_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_facade_basic_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());

        // Create workspace
        assert!(core.create_workspace().is_ok());
        assert!(core.validate_workspace().unwrap());

        // Create project
        let project = core.create_project("My Project").unwrap();
        let projects = core.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, project.id);

        // Create volume
        let volume = core.create_volume(&project.id, "Vol 1").unwrap();
        let volumes = core.list_volumes(&project.id).unwrap();
        // Since we don't know which order they will be returned in (it reads from directory),
        // we sort them by title or id or just check if the new volume is present.
        assert_eq!(volumes.len(), 2);
        assert!(volumes.iter().any(|v| v.id == volume.id));

        // Create chapter
        let chapter = core
            .create_chapter(&project.id, &volume.id, "Ch 1")
            .unwrap();
        let chapters = core.list_chapters(&project.id, &volume.id).unwrap();
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].id, chapter.id);

        // Write and read chapter
        core.write_chapter(&project.id, &volume.id, &chapter.id, "Content here")
            .unwrap();
        let content = core
            .read_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap();
        assert_eq!(content.content, "Content here");
    }

    #[test]
    fn test_facade_settings_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let mut local_settings = core.load_local_settings().unwrap();
        local_settings.window_width = 1000.0;
        local_settings.theme_mode = Some("dark".to_string());
        local_settings.auto_save_enabled = true;
        local_settings.editor_font_size = 14.0;
        core.save_local_settings(&local_settings).unwrap();

        let loaded_local = core.load_local_settings().unwrap();
        assert_eq!(loaded_local.window_width, 1000.0);
        assert_eq!(loaded_local.theme_mode.unwrap(), "dark");
        assert!(loaded_local.auto_save_enabled);
        assert_eq!(loaded_local.editor_font_size, 14.0);

        let mut syncable_settings = core.load_syncable_settings().unwrap();
        syncable_settings.font_size = 18.0;
        syncable_settings.theme_mode = "system".to_string();
        core.save_syncable_settings(&syncable_settings).unwrap();

        let loaded_syncable = core.load_syncable_settings().unwrap();
        assert_eq!(loaded_syncable.font_size, 18.0);
        assert_eq!(loaded_syncable.theme_mode, "system");
    }

    #[test]
    fn test_facade_not_implemented() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());

        assert!(core.backup_project("p1").is_err());
        assert!(core.move_chapter_to_trash("c1").is_err());
        assert!(core.update_index().is_err());
        assert!(core.sync_workspace().is_err());
    }
}
