use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::api::error::WriterError;
use crate::api::types::*;
use crate::api::ResultEnvelope;
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

    pub(crate) fn core(&self) -> WriterCore {
        WriterCore::new(&self.workspace_path)
    }

    fn json_string<T: Serialize>(value: &T) -> ApiResult<String> {
        serde_json::to_string(value).map_err(Into::into)
    }

    pub fn envelope_json<T: Serialize>(result: ApiResult<T>) -> String {
        ResultEnvelope::from_api_result(result).to_json_string()
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
        self.core()
            .create_workspace()
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn validate_workspace(&self) -> ApiResult<bool> {
        self.core().validate_workspace().map_err(Into::into)
    }

    pub fn get_workspace_diagnostics(
        &self,
        has_workspace: bool,
        tree_count: u64,
    ) -> ApiResult<WorkspaceDiagnosticsDto> {
        let path_obj = self.workspace_path.as_path();
        let has_path = !self.workspace_path.as_os_str().is_empty();
        let path_exists = has_path && path_obj.exists();
        let is_dir = has_path && path_obj.is_dir();
        let manifest_path = if has_path {
            path_obj.join("workspace_manifest.json")
        } else {
            PathBuf::new()
        };
        let projects_path = if has_path {
            path_obj.join("projects")
        } else {
            PathBuf::new()
        };
        let app_meta_path = if has_path {
            path_obj.join("app-meta")
        } else {
            PathBuf::new()
        };
        let manifest_exists = has_path && manifest_path.exists();
        let projects_dir_exists = has_path && projects_path.is_dir();
        let app_meta_exists = has_path && app_meta_path.exists();
        let validate_workspace = if is_dir {
            self.validate_workspace().unwrap_or(false)
        } else {
            false
        };
        let core_initialized = has_workspace && has_path;
        let last_workspace_path = crate::app_config::get_last_workspace_path().unwrap_or_default();
        let (writable, writable_error) = Self::probe_workspace_writable(path_obj, is_dir);
        let create_project_available = has_workspace
            && core_initialized
            && validate_workspace
            && path_exists
            && is_dir
            && manifest_exists
            && projects_dir_exists
            && writable;

        Ok(WorkspaceDiagnosticsDto {
            has_workspace,
            workspace_path: self.workspace_path.to_string_lossy().to_string(),
            core_initialized,
            path_exists,
            is_dir,
            manifest_path: manifest_path.to_string_lossy().to_string(),
            manifest_exists,
            projects_path: projects_path.to_string_lossy().to_string(),
            projects_dir_exists,
            app_meta_exists,
            writable,
            writable_error,
            validate_workspace,
            tree_count,
            last_workspace_path,
            create_project_available,
        })
    }

    pub fn get_workspace_diagnostics_envelope_json(
        &self,
        has_workspace: bool,
        tree_count: u64,
    ) -> String {
        Self::envelope_json(self.get_workspace_diagnostics(has_workspace, tree_count))
    }

    fn probe_workspace_writable(path: &Path, is_dir: bool) -> (bool, String) {
        if !is_dir {
            return (
                false,
                "path does not exist or is not a directory".to_string(),
            );
        }

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let test_file = path.join(format!(
            ".writer_write_test_{}_{}",
            std::process::id(),
            nonce
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&test_file)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(b"test") {
                    let _ = std::fs::remove_file(&test_file);
                    return (false, error.to_string());
                }
                drop(file);
                let remove_result = std::fs::remove_file(&test_file);
                if let Err(error) = remove_result {
                    return (false, error.to_string());
                }
                (true, String::new())
            }
            Err(error) => (false, error.to_string()),
        }
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
        self.core()
            .create_project(title)
            .map(Into::into)
            .map_err(Into::into)
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
        self.core()
            .delete_project(project_id)
            .map(|_| true)
            .map_err(Into::into)
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

    pub fn reorder_volumes(
        &self,
        project_id: &str,
        ordered_volume_ids: &[String],
    ) -> ApiResult<bool> {
        self.core()
            .reorder_volumes(project_id, ordered_volume_ids)
            .map(|_| true)
            .map_err(Into::into)
    }

    // --- Chapter methods moved to chapter_api.rs ---
    // --- Settings methods moved to settings_api.rs ---
    // --- Sync methods moved to sync_api.rs ---

    pub fn get_writing_stats_summary_json(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_summary(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_stats_by_project_json(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_by_project(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_stats_by_chapter_json(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_by_chapter(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_stats_by_device_json(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_by_device(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_speed_curve_json(
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
        self.record_writing_event_for_platform(
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
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_writing_event_for_platform(
        &self,
        device_id: &str,
        platform: &str,
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
        let ai_inserted_chars = Self::non_negative_counter("ai_inserted_chars", ai_inserted_chars)?;

        self.core()
            .record_writing_event(
                device_id,
                platform,
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

    pub fn flush_recent_edits(&self) -> ApiResult<bool> {
        self.core()
            .flush_recent_edits()
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

    pub fn save_mindmap_graph(
        &self,
        project_id: &str,
        graph: crate::api::types::MindMapGraphDto,
    ) -> ApiResult<bool> {
        let graph: crate::mind_map::graph::MindMapGraph = graph.into();
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

    pub fn list_starmaps_json(&self) -> ApiResult<String> {
        let value = self.core().list_starmaps().map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn create_starmap_json(&self, title: &str, desc: &str) -> ApiResult<String> {
        let value = self
            .core()
            .create_starmap(title, desc, None)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_starmap_graph_json(&self, starmap_id: &str) -> ApiResult<String> {
        let value = self
            .core()
            .get_starmap_graph(starmap_id)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: &str,
        embed: crate::api::types::StarMapEmbedDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        self.core()
            .add_starmap_embed(starmap_id, embed.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn update_starmap_embed(
        &self,
        starmap_id: &str,
        instance_id: &str,
        patch: crate::api::types::StarMapEmbedPatchDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        self.core()
            .update_starmap_embed(starmap_id, instance_id, patch.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn delete_starmap_embed(&self, starmap_id: &str, instance_id: &str) -> ApiResult<bool> {
        self.core()
            .delete_starmap_embed(starmap_id, instance_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn add_starmap_link(
        &self,
        starmap_id: &str,
        link: crate::api::types::StarMapLinkDto,
    ) -> ApiResult<crate::api::types::StarMapLinkDto> {
        self.core()
            .add_starmap_link(starmap_id, link.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn update_starmap_link(
        &self,
        starmap_id: &str,
        link_id: &str,
        patch: crate::api::types::StarMapLinkPatchDto,
    ) -> ApiResult<crate::api::types::StarMapLinkDto> {
        self.core()
            .update_starmap_link(starmap_id, link_id, patch.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn delete_starmap_link(&self, starmap_id: &str, link_id: &str) -> ApiResult<bool> {
        self.core()
            .delete_starmap_link(starmap_id, link_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn find_starmap_references_json(&self, target_starmap_id: &str) -> ApiResult<String> {
        let value = self
            .core()
            .find_starmap_references(target_starmap_id)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn find_starmap_references(
        &self,
        target_starmap_id: &str,
    ) -> ApiResult<Vec<crate::api::types::StarMapReferenceDto>> {
        self.core()
            .find_starmap_references(target_starmap_id)
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn get_mind_map_snapshot(
        &self,
        project_id: &str,
    ) -> ApiResult<crate::api::types::MindMapSnapshotDto> {
        self.core()
            .get_mind_map_snapshot(project_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn create_mind_map_graph(
        &self,
        project_id: &str,
        title: &str,
    ) -> ApiResult<crate::api::types::MindMapGraphDto> {
        self.core()
            .create_mind_map_graph(project_id, title)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn list_mind_map_graphs(
        &self,
        project_id: &str,
    ) -> ApiResult<crate::api::types::MindMapGraphsListDto> {
        self.core()
            .list_mind_map_graphs(project_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn set_default_mind_map_graph(&self, project_id: &str, graph_id: &str) -> ApiResult<bool> {
        self.core()
            .set_default_mind_map_graph(project_id, graph_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn create_mind_map_node(
        &self,
        project_id: &str,
        graph_id: &str,
        node: crate::api::types::MindMapGraphNodeDto,
    ) -> ApiResult<crate::api::types::MindMapGraphNodeDto> {
        self.core()
            .create_mind_map_node(project_id, graph_id, node.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn update_mind_map_node(
        &self,
        project_id: &str,
        graph_id: &str,
        node_id: &str,
        patch: crate::api::types::MindMapNodePatchDto,
    ) -> ApiResult<crate::api::types::MindMapGraphNodeDto> {
        self.core()
            .update_mind_map_node(project_id, graph_id, node_id, patch.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn delete_mind_map_node(
        &self,
        project_id: &str,
        graph_id: &str,
        node_id: &str,
        cascade: bool,
    ) -> ApiResult<bool> {
        self.core()
            .delete_mind_map_node(project_id, graph_id, node_id, cascade)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn create_mind_map_edge(
        &self,
        project_id: &str,
        graph_id: &str,
        edge: crate::api::types::MindMapGraphEdgeDto,
    ) -> ApiResult<crate::api::types::MindMapGraphEdgeDto> {
        self.core()
            .create_mind_map_edge(project_id, graph_id, edge.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn update_mind_map_edge(
        &self,
        project_id: &str,
        graph_id: &str,
        edge_id: &str,
        patch: crate::api::types::MindMapEdgePatchDto,
    ) -> ApiResult<crate::api::types::MindMapGraphEdgeDto> {
        self.core()
            .update_mind_map_edge(project_id, graph_id, edge_id, patch.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn delete_mind_map_edge(
        &self,
        project_id: &str,
        graph_id: &str,
        edge_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .delete_mind_map_edge(project_id, graph_id, edge_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn create_mind_map_anchor(
        &self,
        project_id: &str,
        graph_id: &str,
        anchor: crate::api::types::MindMapAnchorDto,
    ) -> ApiResult<crate::api::types::MindMapAnchorDto> {
        self.core()
            .create_mind_map_anchor(project_id, graph_id, anchor.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn bind_mind_map_node_to_anchor(
        &self,
        project_id: &str,
        graph_id: &str,
        node_id: &str,
        anchor_id: &str,
        link_kind: &str,
    ) -> ApiResult<crate::api::types::MindMapLinkDto> {
        self.core()
            .bind_mind_map_node_to_anchor(project_id, graph_id, node_id, anchor_id, link_kind)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_mind_map_layout(
        &self,
        project_id: &str,
        graph_id: &str,
        layout: crate::api::types::MindMapLayoutDto,
    ) -> ApiResult<bool> {
        self.core()
            .save_mind_map_layout(project_id, graph_id, layout.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn get_starmap_layout(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapLayoutDto> {
        self.core()
            .get_starmap_layout(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn get_starmap_graph(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapGraphDto> {
        self.core()
            .get_starmap_graph(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn list_starmaps(&self) -> ApiResult<Vec<crate::api::types::StarMapMetaDto>> {
        self.core()
            .list_starmaps()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn list_starmaps_for_project(
        &self,
        project_id: &str,
    ) -> ApiResult<Vec<crate::api::types::StarMapMetaDto>> {
        self.core()
            .list_starmaps_for_project(project_id)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn get_starmap(&self, starmap_id: &str) -> ApiResult<crate::api::types::StarMapMetaDto> {
        self.core()
            .get_starmap(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn create_starmap(
        &self,
        title: &str,
        desc: &str,
        template_id: Option<&str>,
    ) -> ApiResult<crate::api::types::StarMapMetaDto> {
        self.core()
            .create_starmap(title, desc, template_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: &str,
        node: crate::api::types::StarMapNodeDto,
        x: f32,
        y: f32,
    ) -> ApiResult<crate::api::types::StarMapNodeDto> {
        self.core()
            .add_starmap_node(starmap_id, node.into(), x, y)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_starmap_layout(
        &self,
        starmap_id: &str,
        layout: &crate::api::types::StarMapLayoutDto,
    ) -> ApiResult<bool> {
        self.core()
            .save_starmap_layout(starmap_id, &layout.clone().into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn get_starmap_viewport(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapViewportDto> {
        self.core()
            .get_starmap_viewport(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_starmap_viewport(
        &self,
        starmap_id: &str,
        viewport: crate::api::types::StarMapViewportDto,
    ) -> ApiResult<bool> {
        self.core()
            .save_starmap_viewport(starmap_id, &viewport.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn compute_starmap_edge_renders(
        &self,
        graph: crate::api::types::StarMapGraphDto,
        layout: crate::api::types::StarMapLayoutDto,
    ) -> ApiResult<Vec<crate::api::types::StarMapEdgeRenderDto>> {
        let node_centers: HashMap<String, (f32, f32)> = layout
            .nodes
            .iter()
            .map(|node| {
                (
                    node.node_id.clone(),
                    (node.x + node.width / 2.0, node.y + node.height / 2.0),
                )
            })
            .collect();
        let edges: Vec<crate::starmap::render::EdgeInput> = graph
            .edges
            .into_iter()
            .filter_map(|edge| {
                let from = edge.from.filter(|id| !id.is_empty())?;
                let to = edge.to.filter(|id| !id.is_empty())?;
                Some(crate::starmap::render::EdgeInput {
                    id: edge.id,
                    from,
                    to,
                })
            })
            .collect();

        Ok(crate::starmap::render::compute_edge_renders(
            &edges,
            &node_centers,
            &crate::starmap::render::EdgeRenderParams::default(),
        )
        .into_iter()
        .map(Into::into)
        .collect())
    }

    pub fn hit_test_starmap_node(
        &self,
        layout: crate::api::types::StarMapLayoutDto,
        x: f32,
        y: f32,
    ) -> ApiResult<Option<String>> {
        let layout: crate::starmap::types::StarMapLayout = layout.into();
        Ok(crate::starmap::hittest::hit_test_nodes(x, y, &layout.nodes).map(|hit| hit.id))
    }

    pub fn rename_starmap(
        &self,
        starmap_id: &str,
        new_title: &str,
    ) -> ApiResult<crate::api::types::StarMapMetaDto> {
        self.core()
            .rename_starmap(starmap_id, new_title)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn delete_starmap(&self, starmap_id: &str) -> ApiResult<bool> {
        self.core()
            .delete_starmap(starmap_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn bind_starmap_to_project(&self, starmap_id: &str, project_id: &str) -> ApiResult<bool> {
        self.core()
            .bind_starmap_to_project(starmap_id, project_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn unbind_starmap_from_project(&self, starmap_id: &str) -> ApiResult<bool> {
        self.core()
            .unbind_starmap_from_project(starmap_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn set_main_starmap_for_project(
        &self,
        starmap_id: &str,
        project_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .set_main_starmap_for_project(starmap_id, project_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn get_main_starmap_for_project(
        &self,
        project_id: &str,
    ) -> ApiResult<Option<crate::api::types::StarMapMetaDto>> {
        self.core()
            .get_main_starmap_for_project(project_id)
            .map(|opt| opt.map(Into::into))
            .map_err(Into::into)
    }

    pub fn create_child_starmap(
        &self,
        parent_id: &str,
        title: &str,
        desc: &str,
        accent_color: Option<&str>,
    ) -> ApiResult<crate::api::types::StarMapMetaDto> {
        self.core()
            .create_child_starmap(parent_id, title, desc, accent_color)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn create_child_starmap_legacy(
        &self,
        parent_id: &str,
        title: &str,
        desc: &str,
        accent_color: Option<&str>,
    ) -> ApiResult<crate::api::types::StarMapMetaDto> {
        self.create_child_starmap(parent_id, title, desc, accent_color)
    }

    pub fn update_starmap_node(
        &self,
        starmap_id: &str,
        node_id: &str,
        patch: crate::api::types::StarMapNodePatchDto,
    ) -> ApiResult<crate::api::types::StarMapNodeDto> {
        self.core()
            .update_starmap_node(starmap_id, node_id, patch.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn delete_starmap_node(&self, starmap_id: &str, node_id: &str) -> ApiResult<bool> {
        self.core()
            .delete_starmap_node(starmap_id, node_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn add_starmap_edge(
        &self,
        starmap_id: &str,
        edge: crate::api::types::StarMapEdgeDto,
    ) -> ApiResult<crate::api::types::StarMapEdgeDto> {
        self.core()
            .add_starmap_edge(starmap_id, edge.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn update_starmap_edge(
        &self,
        starmap_id: &str,
        edge_id: &str,
        patch: crate::api::types::StarMapEdgePatchDto,
    ) -> ApiResult<crate::api::types::StarMapEdgeDto> {
        self.core()
            .update_starmap_edge(starmap_id, edge_id, patch.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn delete_starmap_edge(&self, starmap_id: &str, edge_id: &str) -> ApiResult<bool> {
        self.core()
            .delete_starmap_edge(starmap_id, edge_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn save_starmap_graph(
        &self,
        starmap_id: &str,
        graph: &crate::api::types::StarMapGraphDto,
    ) -> ApiResult<bool> {
        self.core()
            .save_starmap_graph(starmap_id, &graph.clone().into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn list_registered_actions(
        &self,
    ) -> ApiResult<Vec<crate::api::types::ActionDescriptorDto>> {
        self.core()
            .list_registered_actions()
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn execute_action_ext(
        &self,
        action_id: &str,
        args_json: &str,
        context_json: &str,
    ) -> ApiResult<crate::api::types::ActionResultDto> {
        self.core()
            .execute_action(action_id, args_json, context_json)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn ai_available(&self) -> bool {
        self.core().ai_available()
    }

    pub fn list_registered_actions_json(&self) -> String {
        let result: ApiResult<Vec<crate::api::types::ActionDescriptorDto>> =
            self.list_registered_actions();
        ResultEnvelope::from_api_result(result).to_json_string()
    }

    pub fn execute_action_json(
        &self,
        action_id: &str,
        args_json: &str,
        context_json: &str,
    ) -> String {
        let result: ApiResult<crate::api::types::ActionResultDto> =
            self.execute_action_ext(action_id, args_json, context_json);
        ResultEnvelope::from_api_result(result).to_json_string()
    }

    pub fn get_writing_stats_summary(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<crate::api::types::WritingStatsSummaryDto> {
        let value = self
            .core()
            .get_writing_stats_summary(start_date, end_date)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_writing_stats_by_project(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<crate::api::types::ProjectStatsSummaryDto> {
        let value = self
            .core()
            .get_writing_stats_by_project(start_date, end_date)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_writing_stats_by_chapter(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<crate::api::types::ChapterStatsSummaryDto> {
        let value = self
            .core()
            .get_writing_stats_by_chapter(start_date, end_date)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_writing_stats_by_device(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<crate::api::types::DeviceStatsSummaryDto> {
        let value = self
            .core()
            .get_writing_stats_by_device(start_date, end_date)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_writing_speed_curve(
        &self,
        start_date: &str,
        end_date: &str,
        bucket_minutes: u32,
    ) -> ApiResult<crate::api::types::SpeedCurveSummaryDto> {
        let value = self
            .core()
            .get_writing_speed_curve(start_date, end_date, bucket_minutes)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
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
    fn flush_recent_edits_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        assert!(api.flush_recent_edits().unwrap());
    }

    #[test]
    fn workspace_diagnostics_reports_core_owned_state() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let diagnostics = api.get_workspace_diagnostics(true, 3).unwrap();

        assert!(diagnostics.has_workspace);
        assert!(diagnostics.path_exists);
        assert!(diagnostics.is_dir);
        assert!(diagnostics.manifest_exists);
        assert!(diagnostics.projects_dir_exists);
        assert!(diagnostics.writable);
        assert!(diagnostics.validate_workspace);
        assert_eq!(diagnostics.tree_count, 3);
        assert!(diagnostics.create_project_available);
    }

    #[test]
    fn workspace_diagnostics_envelope_uses_camel_case_fields() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let json = api.get_workspace_diagnostics_envelope_json(true, 0);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert_eq!(value["data"]["pathExists"], true);
        assert_eq!(value["data"]["createProjectAvailable"], true);
        assert!(value["data"].get("path_exists").is_none());
    }

    #[test]
    fn record_writing_event_for_platform_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let result = api
            .record_writing_event_for_platform(
                "dev-1",
                "linux",
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
    fn save_mindmap_graph_persists_graph_and_snapshot_reads_it() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test Project").unwrap();
        let graph = valid_graph(&project.id);

        assert_eq!(
            api.save_mindmap_graph(&project.id, graph.into()).unwrap(),
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
    fn save_mindmap_graph_rejects_project_id_mismatch() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test Project").unwrap();
        let graph = valid_graph("another_project");

        let err = api
            .save_mindmap_graph(&project.id, graph.into())
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
    fn save_mindmap_graph_propagates_validation_error() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test Project").unwrap();
        let mut graph = valid_graph(&project.id);
        graph.schema_version = 1;

        let err = api
            .save_mindmap_graph(&project.id, graph.into())
            .unwrap_err();

        assert!(matches!(
            err,
            WriterError::Io(message) if message.contains("UnsupportedSchemaVersion")
        ));
    }

    #[test]
    fn compute_starmap_edge_renders_uses_core_geometry() {
        let api = WriterCoreApi::new("");
        let graph = StarMapGraphDto {
            schema_version: 1,
            id: "graph".to_string(),
            starmap_id: "map".to_string(),
            title: "Map".to_string(),
            nodes: vec![],
            edges: vec![StarMapEdgeDto {
                id: "edge-1".to_string(),
                from: Some("a".to_string()),
                to: Some("b".to_string()),
                kind: StarMapEdgeKindDto::RelatedTo,
                label: None,
                payload: None,
                from_target: None,
                to_target: None,
                from_endpoint: None,
                to_endpoint: None,
                created_at: 0,
                updated_at: 0,
            }],
            embeds: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let layout = StarMapLayoutDto {
            kind: StarMapLayoutKindDto::Freeform,
            nodes: vec![
                StarMapLayoutNodeDto {
                    node_id: "a".to_string(),
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                    radius: 16.0,
                    collapsed: false,
                    z_index: 0,
                    scale: 1.0,
                    depth: 0.0,
                    focus_weight: 1.0,
                    orbit_group: None,
                },
                StarMapLayoutNodeDto {
                    node_id: "b".to_string(),
                    x: 200.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                    radius: 16.0,
                    collapsed: false,
                    z_index: 0,
                    scale: 1.0,
                    depth: 0.0,
                    focus_weight: 1.0,
                    orbit_group: None,
                },
            ],
        };

        let renders = api.compute_starmap_edge_renders(graph, layout).unwrap();

        assert_eq!(renders.len(), 1);
        assert_eq!(renders[0].edge_id, "edge-1");
        assert!((renders[0].start_x - 92.0).abs() < 0.1);
        assert!((renders[0].end_x - 208.0).abs() < 0.1);
    }

    #[test]
    fn hit_test_starmap_node_returns_top_z_node() {
        let api = WriterCoreApi::new("");
        let layout = StarMapLayoutDto {
            kind: StarMapLayoutKindDto::Freeform,
            nodes: vec![
                StarMapLayoutNodeDto {
                    node_id: "lower".to_string(),
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                    radius: 16.0,
                    collapsed: false,
                    z_index: 1,
                    scale: 1.0,
                    depth: 0.0,
                    focus_weight: 1.0,
                    orbit_group: None,
                },
                StarMapLayoutNodeDto {
                    node_id: "upper".to_string(),
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                    radius: 16.0,
                    collapsed: false,
                    z_index: 2,
                    scale: 1.0,
                    depth: 0.0,
                    focus_weight: 1.0,
                    orbit_group: None,
                },
            ],
        };

        let hit = api.hit_test_starmap_node(layout, 50.0, 50.0).unwrap();

        assert_eq!(hit.as_deref(), Some("upper"));
    }

    #[test]
    fn save_chapter_content_envelope_json_returns_success_with_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();

        let json = api.save_chapter_content_envelope_json(
            &project.id,
            &volume.id,
            &chapter.id,
            "Hello World",
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert!(value["data"].is_object());
        assert_eq!(value["data"]["content_len"], 11u32);
        assert!(value["changedEntities"].is_array());
        assert_eq!(value["changedEntities"][0]["entityType"], "ChapterSaved");
        assert_eq!(value["changedEntities"][0]["entityId"], chapter.id);
    }

    #[test]
    fn create_project_volume_chapter_envelope_json_returns_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let project_json = api.create_project_envelope_json("Test");
        let project_value: serde_json::Value = serde_json::from_str(&project_json).unwrap();
        let project_id = project_value["data"]["id"].as_str().unwrap().to_string();
        assert_eq!(project_value["success"], true);
        assert_eq!(
            project_value["changedEntities"][0]["entityType"],
            "ProjectCreated"
        );
        assert_eq!(project_value["changedEntities"][0]["entityId"], project_id);

        let volume_json = api.create_volume_envelope_json(&project_id, "Vol 1");
        let volume_value: serde_json::Value = serde_json::from_str(&volume_json).unwrap();
        let volume_id = volume_value["data"]["id"].as_str().unwrap().to_string();
        assert_eq!(volume_value["success"], true);
        assert_eq!(
            volume_value["changedEntities"][0]["entityType"],
            "VolumeCreated"
        );
        assert_eq!(volume_value["changedEntities"][0]["entityId"], volume_id);

        let chapter_json = api.create_chapter_envelope_json(&project_id, &volume_id, "Ch 1");
        let chapter_value: serde_json::Value = serde_json::from_str(&chapter_json).unwrap();
        let chapter_id = chapter_value["data"]["id"].as_str().unwrap().to_string();
        assert_eq!(chapter_value["success"], true);
        assert_eq!(
            chapter_value["changedEntities"][0]["entityType"],
            "ChapterCreated"
        );
        assert_eq!(chapter_value["changedEntities"][0]["entityId"], chapter_id);
    }

    #[test]
    fn project_volume_chapter_mutation_envelopes_return_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();
        let volume_ids: Vec<String> = api
            .list_volumes(&project.id)
            .unwrap()
            .into_iter()
            .map(|volume| volume.id)
            .collect();

        let cases = vec![
            (
                api.rename_project_envelope_json(&project.id, "Renamed"),
                "ProjectRenamed",
                Some(project.id.clone()),
            ),
            (
                api.reorder_projects_envelope_json(&vec![project.id.clone()]),
                "ProjectsReordered",
                None,
            ),
            (
                api.rename_volume_envelope_json(&project.id, &volume.id, "Vol 2"),
                "VolumeRenamed",
                Some(volume.id.clone()),
            ),
            (
                api.reorder_volumes_envelope_json(&project.id, &volume_ids),
                "VolumesReordered",
                Some(project.id.clone()),
            ),
            (
                api.rename_chapter_envelope_json(&project.id, &volume.id, &chapter.id, "Ch 2"),
                "ChapterRenamed",
                Some(chapter.id.clone()),
            ),
            (
                api.reorder_chapters_envelope_json(
                    &project.id,
                    &volume.id,
                    &vec![chapter.id.clone()],
                ),
                "ChaptersReordered",
                Some(volume.id.clone()),
            ),
            (
                api.update_chapter_note_envelope_json(&project.id, &volume.id, &chapter.id, "note"),
                "ChapterNoteUpdated",
                Some(chapter.id.clone()),
            ),
            (
                api.clear_chapter_content_envelope_json(&project.id, &volume.id, &chapter.id),
                "ChapterCleared",
                Some(chapter.id.clone()),
            ),
        ];

        for (json, entity_type, entity_id) in cases {
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(
                value["success"], true,
                "{entity_type} envelope failed: {json}"
            );
            assert_eq!(value["changedEntities"][0]["entityType"], entity_type);
            match entity_id {
                Some(id) => assert_eq!(value["changedEntities"][0]["entityId"], id),
                None => assert!(value["changedEntities"][0].get("entityId").is_none()),
            }
        }
    }

    #[test]
    fn delete_chapter_envelope_json_returns_success_with_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();

        let json = api.delete_chapter_envelope_json(&project.id, &volume.id, &chapter.id);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert_eq!(value["data"], true);
        assert_eq!(value["changedEntities"][0]["entityType"], "ChapterDeleted");
        assert_eq!(value["changedEntities"][0]["entityId"], chapter.id);
    }

    #[test]
    fn delete_project_envelope_json_returns_success_with_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test").unwrap();

        let json = api.delete_project_envelope_json(&project.id);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert_eq!(value["changedEntities"][0]["entityType"], "ProjectDeleted");
        assert_eq!(value["changedEntities"][0]["entityId"], project.id);
    }

    #[test]
    fn save_sync_config_envelope_json_returns_success_with_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let config = crate::api::types::SyncConfigDto {
            enabled: true,
            backend_type: "github_api".to_string(),
            remote_url: "https://github.com/test/repo.git".to_string(),
            transport: "https_token".to_string(),
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            proxy_enabled: false,
            proxy_type: "auto".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            username: "".to_string(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let json = api.save_sync_config_envelope_json(config);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert_eq!(value["data"], true);
        assert_eq!(value["changedEntities"][0]["entityType"], "SyncConfigSaved");
    }

    #[test]
    fn end_to_end_api_chapter_write_reopen_verify() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("E2E Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();

        let content = "端到端测试内容。\n第二行文本。\n第三行。";
        let receipt = api
            .save_chapter_content(&project.id, &volume.id, &chapter.id, content)
            .unwrap();

        assert_eq!(receipt.content_len, content.len() as u32);
        assert!(receipt.word_count > 0);
        assert!(!receipt.content_hash.is_empty());

        let reopened = api
            .open_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap();
        assert_eq!(reopened.content, content);
        assert_eq!(reopened.meta.hash, receipt.content_hash);
        assert_eq!(reopened.meta.word_count, receipt.word_count);
    }

    #[test]
    fn api_save_chapter_content_with_options_allow_empty_true() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("E2E Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();

        // Write initial content
        let initial_content = "Some initial content";
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, initial_content)
            .unwrap();

        // Overwrite with empty content, allow_empty_overwrite = true
        let receipt = api
            .save_chapter_content_with_options(&project.id, &volume.id, &chapter.id, "", true)
            .unwrap();

        assert_eq!(receipt.content_len, 0);
        assert_eq!(receipt.word_count, 0);

        let reopened = api
            .open_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap();
        assert_eq!(reopened.content, "");
    }

    #[test]
    fn api_save_chapter_content_with_options_allow_empty_false() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("E2E Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();

        // Write initial content
        let initial_content = "Some initial content";
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, initial_content)
            .unwrap();

        // Attempt to overwrite with empty content, allow_empty_overwrite = false
        let result = api.save_chapter_content_with_options(&project.id, &volume.id, &chapter.id, "", false);

        assert!(result.is_err());
        match result.unwrap_err() {
            crate::api::error::WriterError::EmptyOverwriteBlocked { .. } => {}
            e => panic!("Expected EmptyOverwriteBlocked, got {:?}", e),
        }

        let reopened = api
            .open_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap();
        assert_eq!(reopened.content, initial_content); // Verify it wasn't overwritten
    }

    #[test]
    fn perform_sync_envelope_json_returns_envelope_with_sync_result() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let config = crate::api::types::SyncConfigDto {
            enabled: true,
            backend_type: "github_api".to_string(),
            remote_url: "https://github.com/test/repo.git".to_string(),
            transport: "https_token".to_string(),
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            proxy_enabled: false,
            proxy_type: "auto".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            username: "".to_string(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let json = api.perform_sync_envelope_json(config);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(value["success"].is_boolean());
        assert!(value["data"].is_object());
        assert!(value["data"]["status"].is_string());
    }

    #[test]
    fn perform_sync_dry_run_envelope_json_returns_envelope_with_plan() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let config = crate::api::types::SyncConfigDto {
            enabled: true,
            backend_type: "github_api".to_string(),
            remote_url: "https://github.com/test/repo.git".to_string(),
            transport: "https_token".to_string(),
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            proxy_enabled: false,
            proxy_type: "auto".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            username: "".to_string(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let json = api.perform_sync_dry_run_envelope_json(config);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(value["success"].is_boolean());
        if value["success"] == true {
            assert!(value["data"]["files_to_upload"].is_array());
        }
    }

    #[test]
    fn perform_sync_diagnostics_envelope_json_returns_envelope_with_diagnostics() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let config = crate::api::types::SyncConfigDto {
            enabled: true,
            backend_type: "github_api".to_string(),
            remote_url: "https://github.com/test/repo.git".to_string(),
            transport: "https_token".to_string(),
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            proxy_enabled: false,
            proxy_type: "auto".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            username: "".to_string(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let json = api.perform_sync_diagnostics_envelope_json(config);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(value["success"].is_boolean());
        assert!(value["data"].is_object());
        assert!(
            value["data"]["backend_type"].is_string() || value["data"]["backendType"].is_string()
        );
    }

    #[test]
    fn delete_volume_envelope_json_returns_success_with_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();

        let json = api.delete_volume_envelope_json(&project.id, &volume.id);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert_eq!(value["changedEntities"][0]["entityType"], "VolumeDeleted");
        assert_eq!(value["changedEntities"][0]["entityId"], volume.id);
    }

    #[test]
    fn test_reorder_chapters_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let project = api.create_project("Test Project").unwrap();
        let volume = api.create_volume(&project.id, "Volume 1").unwrap();

        let chapter1 = api.create_chapter(&project.id, &volume.id, "Chapter 1").unwrap();
        let chapter2 = api.create_chapter(&project.id, &volume.id, "Chapter 2").unwrap();
        let chapter3 = api.create_chapter(&project.id, &volume.id, "Chapter 3").unwrap();

        let new_order = vec![
            chapter3.id.clone(),
            chapter1.id.clone(),
            chapter2.id.clone(),
        ];

        let result = api.reorder_chapters(&project.id, &volume.id, &new_order);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);

        // Let's verify the order actually changed
        let chapters = api.core().list_chapters(&project.id, &volume.id).unwrap();
        let actual_order: Vec<String> = chapters.into_iter().map(|c| c.id).collect();
        assert_eq!(actual_order, new_order);
    }
}
