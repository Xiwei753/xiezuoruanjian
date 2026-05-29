use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::api::error::WriterError;
use crate::api::types::*;
use crate::facade::WriterCore;

pub type ApiResult<T> = Result<T, WriterError>;

/// Stable Core API service shared by platform adapters.
pub struct WriterCoreApi {
    workspace_path: PathBuf,
}

impl WriterCoreApi {
    pub fn new<P: AsRef<Path>>(workspace_path: P) -> Self {
        Self {
            workspace_path: workspace_path.as_ref().to_path_buf(),
        }
    }

    fn core(&self) -> WriterCore {
        WriterCore::new(&self.workspace_path)
    }

    fn json_string<T: Serialize>(value: &T) -> ApiResult<String> {
        serde_json::to_string(value).map_err(Into::into)
    }

    fn non_negative_counter(name: &str, value: i32) -> ApiResult<u32> {
        if value < 0 {
            return Err(WriterError::Other(format!(
                "negative writing event counter: {}={}",
                name, value
            )));
        }
        Ok(value as u32)
    }

    pub fn list_projects(&self) -> ApiResult<Vec<ProjectDto>> {
        self.core()
            .list_projects()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn create_workspace_if_needed(&self) -> ApiResult<bool> {
        self.core().create_workspace().map(|_| true).map_err(Into::into)
    }

    pub fn validate_workspace(&self) -> ApiResult<bool> {
        self.core().validate_workspace().map_err(Into::into)
    }

    pub fn get_recent_edits(&self) -> ApiResult<Vec<RecentEditDto>> {
        self.core()
            .get_recent_edits()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn record_recent_edit(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .record_recent_edit(project_id, volume_id, chapter_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn create_project(&self, title: &str) -> ApiResult<ProjectDto> {
        self.core().create_project(title).map(Into::into).map_err(Into::into)
    }

    pub fn get_project_stats(&self, project_id: &str) -> ApiResult<ProjectStatsDto> {
        self.core()
            .get_project_stats(project_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn rename_project(&self, project_id: &str, new_title: &str) -> ApiResult<bool> {
        self.core()
            .rename_project(project_id, new_title)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn delete_project(&self, project_id: &str) -> ApiResult<bool> {
        self.core().delete_project(project_id).map(|_| true).map_err(Into::into)
    }

    pub fn reorder_projects(&self, ordered_project_ids: &[String]) -> ApiResult<bool> {
        self.core()
            .reorder_projects(ordered_project_ids)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn list_volumes(&self, project_id: &str) -> ApiResult<Vec<VolumeDto>> {
        self.core()
            .list_volumes(project_id)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn create_volume(&self, project_id: &str, title: &str) -> ApiResult<VolumeDto> {
        self.core()
            .create_volume(project_id, title)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn rename_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        new_title: &str,
    ) -> ApiResult<bool> {
        self.core()
            .rename_volume(project_id, volume_id, new_title)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn delete_volume(&self, project_id: &str, volume_id: &str) -> ApiResult<bool> {
        self.core()
            .delete_volume(project_id, volume_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn reorder_volumes(&self, project_id: &str, ordered_volume_ids: &[String]) -> ApiResult<bool> {
        self.core()
            .reorder_volumes(project_id, ordered_volume_ids)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn list_chapters(
        &self,
        project_id: &str,
        volume_id: &str,
    ) -> ApiResult<Vec<ChapterMetaDto>> {
        self.core()
            .list_chapters(project_id, volume_id)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn create_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        title: &str,
    ) -> ApiResult<ChapterMetaDto> {
        self.core()
            .create_chapter(project_id, volume_id, title)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn rename_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        new_title: &str,
    ) -> ApiResult<bool> {
        self.core()
            .rename_chapter(project_id, volume_id, chapter_id, new_title)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn delete_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .delete_chapter(project_id, volume_id, chapter_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn reorder_chapters(
        &self,
        project_id: &str,
        volume_id: &str,
        ordered_chapter_ids: &[String],
    ) -> ApiResult<bool> {
        self.core()
            .reorder_chapters(project_id, volume_id, ordered_chapter_ids)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn open_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<ChapterContentDto> {
        self.core()
            .open_chapter(project_id, volume_id, chapter_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_chapter_content(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.core()
            .write_chapter_verified(project_id, volume_id, chapter_id, content)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn clear_chapter_content(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.core()
            .clear_chapter_content_verified(project_id, volume_id, chapter_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn update_chapter_note(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        note: &str,
    ) -> ApiResult<bool> {
        self.core()
            .update_chapter_note(project_id, volume_id, chapter_id, note)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn load_local_settings(&self) -> ApiResult<LocalSettingsDto> {
        self.core().load_local_settings().map(Into::into).map_err(Into::into)
    }

    pub fn save_local_settings(&self, settings: LocalSettingsDto) -> ApiResult<bool> {
        self.core()
            .save_local_settings(&settings.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn load_syncable_settings(&self) -> ApiResult<SyncableSettingsDto> {
        self.core()
            .load_syncable_settings()
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_syncable_settings(&self, settings: SyncableSettingsDto) -> ApiResult<bool> {
        self.core()
            .save_syncable_settings(&settings.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn load_sync_config(&self) -> ApiResult<SyncConfigDto> {
        self.core().load_sync_config().map(Into::into).map_err(Into::into)
    }

    pub fn save_sync_config(&self, config: SyncConfigDto) -> ApiResult<bool> {
        self.core()
            .save_sync_config(&config.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn load_sync_secrets(&self) -> ApiResult<SyncSecretsDto> {
        self.core().load_sync_secrets().map(Into::into).map_err(Into::into)
    }

    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> ApiResult<bool> {
        self.core()
            .save_sync_secrets(&secrets.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn load_sync_state(&self) -> ApiResult<SyncStateDto> {
        self.core().load_sync_state().map(Into::into).map_err(Into::into)
    }

    pub fn perform_sync_diagnostics(
        &self,
        config: SyncConfigDto,
    ) -> ApiResult<SyncDiagnosticsResultDto> {
        self.core()
            .perform_sync_diagnostics(&config.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn perform_sync_dry_run(&self, config: SyncConfigDto) -> ApiResult<SyncPlanDto> {
        self.core()
            .perform_sync_dry_run(&config.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn perform_sync(&self, config: SyncConfigDto) -> ApiResult<SyncResultDto> {
        self.core()
            .perform_sync(&config.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn get_writing_stats_summary(&self, start_date: &str, end_date: &str) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_summary(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_stats_by_project(&self, start_date: &str, end_date: &str) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_by_project(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_stats_by_chapter(&self, start_date: &str, end_date: &str) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_by_chapter(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_stats_by_device(&self, start_date: &str, end_date: &str) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_by_device(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_speed_curve(
        &self,
        start_date: &str,
        end_date: &str,
        bucket_minutes: u32,
    ) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_speed_curve(start_date, end_date, bucket_minutes)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn calculate_word_count(&self, text: &str) -> u32 {
        self.core().calculate_word_count(text)
    }

    pub fn process_writing_event(
        &self,
        device_id: &str,
        platform: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        old_text: &str,
        new_text: &str,
        session_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .process_writing_event(
                device_id, platform, project_id, volume_id, chapter_id, old_text, new_text,
                session_id,
            )
            .map(|_| true)
            .map_err(WriterError::from)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_writing_event(
        &self,
        device_id: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        source: &str,
        inserted_chars: i32,
        deleted_chars: i32,
        pasted_chars: i32,
        ai_inserted_chars: i32,
        session_id: &str,
    ) -> ApiResult<bool> {
        let inserted_chars = Self::non_negative_counter("inserted_chars", inserted_chars)?;
        let deleted_chars = Self::non_negative_counter("deleted_chars", deleted_chars)?;
        let pasted_chars = Self::non_negative_counter("pasted_chars", pasted_chars)?;
        let ai_inserted_chars =
            Self::non_negative_counter("ai_inserted_chars", ai_inserted_chars)?;

        self.core()
            .record_writing_event(
                device_id,
                "android",
                project_id,
                volume_id,
                chapter_id,
                source,
                inserted_chars,
                deleted_chars,
                pasted_chars,
                ai_inserted_chars,
                session_id,
            )
            .map(|_| true)
            .map_err(WriterError::from)
    }

    pub fn flush_writing_stats(&self) -> ApiResult<bool> {
        self.core()
            .flush_writing_stats()
            .map(|_| true)
            .map_err(WriterError::from)
    }

    pub fn get_mindmap_snapshot_json(&self, project_id: &str) -> ApiResult<String> {
        let value = self
            .core()
            .get_mind_map_snapshot(project_id)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn save_mindmap_graph_json(
        &self,
        project_id: &str,
        graph_json: &str,
    ) -> ApiResult<bool> {
        let graph: crate::mind_map::graph::MindMapGraph = serde_json::from_str(graph_json)?;
        if graph.project_id != project_id {
            return Err(WriterError::Other(format!(
                "project_id mismatch: request project_id={}, graph.project_id={}",
                project_id, graph.project_id
            )));
        }

        let core = self.core();
        crate::mind_map::storage::save_mind_map_graph(&core, &graph).map_err(WriterError::from)?;
        Ok(true)
    }

    pub fn list_starmaps(&self) -> ApiResult<String> {
        let value = self.core().list_starmaps().map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn create_starmap(&self, title: &str, desc: &str) -> ApiResult<String> {
        let value = self
            .core()
            .create_starmap(title, desc, None)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_starmap_graph(&self, starmap_id: &str) -> ApiResult<String> {
        let value = self
            .core()
            .get_starmap_graph(starmap_id)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn add_starmap_node(&self, starmap_id: &str, node_json: &str) -> ApiResult<String> {
        let value = self
            .core()
            .execute_action("starmap.node.add", starmap_id, node_json)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn save_starmap_layout(&self, starmap_id: &str, layout_json: &str) -> ApiResult<bool> {
        self.core()
            .execute_action("starmap.layout.save", starmap_id, layout_json)
            .map(|_| true)
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind_map::graph::{MindMapGraph, MindMapGraphNode, MindMapNodeKind};
    use std::fs::File;
    use tempfile::tempdir;

    fn valid_graph(project_id: &str) -> MindMapGraph {
        MindMapGraph {
            schema_version: 2,
            id: "api_graph".to_string(),
            project_id: project_id.to_string(),
            title: "API Saved Graph".to_string(),
            nodes: vec![MindMapGraphNode {
                id: "custom_node".to_string(),
                title: "API Saved Node".to_string(),
                kind: MindMapNodeKind::Character,
                payload: None,
                tags: vec!["api".to_string()],
                created_at: 1,
                updated_at: 1,
            }],
            edges: vec![],
            anchors: vec![],
            links: vec![],
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn record_writing_event_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let result = api
            .record_writing_event(
                "dev-1",
                "proj1",
                "vol1",
                "chap1",
                "human_typed",
                10,
                0,
                0,
                0,
                "session-1",
            )
            .unwrap();

        assert!(result);
    }

    #[test]
    fn process_writing_event_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let result = api
            .process_writing_event(
                "dev-1",
                "android",
                "proj1",
                "vol1",
                "chap1",
                "old",
                "old text",
                "session-1",
            )
            .unwrap();

        assert!(result);
    }

    #[test]
    fn flush_writing_stats_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        assert!(api.flush_writing_stats().unwrap());
    }

    #[test]
    fn record_writing_event_rejects_negative_counter() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let err = api
            .record_writing_event(
                "dev-1",
                "proj1",
                "vol1",
                "chap1",
                "human_typed",
                0,
                -1,
                0,
                0,
                "session-1",
            )
            .unwrap_err();

        assert!(matches!(
            err,
            WriterError::Other(message)
                if message.contains("negative writing event counter")
                    && message.contains("deleted_chars=-1")
        ));
    }

    #[test]
    fn process_writing_event_propagates_core_error() {
        let temp_dir = tempdir().unwrap();
        let workspace_file = temp_dir.path().join("not_a_directory");
        File::create(&workspace_file).unwrap();
        let api = WriterCoreApi::new(&workspace_file);

        let err = api
            .process_writing_event(
                "dev-1",
                "android",
                "proj1",
                "vol1",
                "chap1",
                "old",
                "old text",
                "session-1",
            )
            .unwrap_err();

        assert!(matches!(err, WriterError::Io(_)));
    }

    #[test]
    fn save_mindmap_graph_json_persists_graph_and_snapshot_reads_it() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test Project").unwrap();
        let graph = valid_graph(&project.id);
        let graph_json = serde_json::to_string(&graph).unwrap();

        assert_eq!(
            api.save_mindmap_graph_json(&project.id, &graph_json).unwrap(),
            true
        );

        let snapshot_json = api.get_mindmap_snapshot_json(&project.id).unwrap();
        let snapshot: crate::mind_map::MindMapSnapshot =
            serde_json::from_str(&snapshot_json).unwrap();

        assert_eq!(snapshot.project_id, project.id);
        assert_eq!(snapshot.layout_kind, "Freeform");
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.nodes[0].id, "custom_node");
        assert_eq!(snapshot.nodes[0].title, "API Saved Node");
    }

    #[test]
    fn save_mindmap_graph_json_rejects_project_id_mismatch() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test Project").unwrap();
        let graph = valid_graph("another_project");
        let graph_json = serde_json::to_string(&graph).unwrap();

        let err = api
            .save_mindmap_graph_json(&project.id, &graph_json)
            .unwrap_err();

        assert!(matches!(
            err,
            WriterError::Other(message)
                if message.contains("project_id mismatch")
                    && message.contains(&project.id)
                    && message.contains("another_project")
        ));
    }

    #[test]
    fn save_mindmap_graph_json_rejects_invalid_json() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test Project").unwrap();

        let err = api
            .save_mindmap_graph_json(&project.id, "{not-json")
            .unwrap_err();

        assert!(matches!(err, WriterError::Json(_)));
    }

    #[test]
    fn save_mindmap_graph_json_propagates_validation_error() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test Project").unwrap();
        let mut graph = valid_graph(&project.id);
        graph.schema_version = 1;
        let graph_json = serde_json::to_string(&graph).unwrap();

        let err = api
            .save_mindmap_graph_json(&project.id, &graph_json)
            .unwrap_err();

        assert!(matches!(
            err,
            WriterError::Io(message) if message.contains("UnsupportedSchemaVersion")
        ));
    }
}
