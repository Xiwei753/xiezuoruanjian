use crate::api::{
    ChapterContentDto, ChapterMetaDto, ChapterSaveReceiptDto, LocalSettingsDto, ProjectDto,
    ProjectStatsDto, RecentEditDto, SyncConfigDto, SyncDiagnosticsResultDto, SyncPlanDto,
    SyncResultDto, SyncSecretsDto, SyncStateDto, SyncableSettingsDto, VolumeDto, WriterCoreApi,
    WriterError,
};

/// Thin UniFFI adapter. Stable Core API behavior lives in `api::WriterCoreApi`.
pub struct WriterAppService {
    api: WriterCoreApi,
}

impl WriterAppService {
    pub fn new(workspace_path: String) -> Self {
        Self {
            api: WriterCoreApi::new(workspace_path),
        }
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectDto>, WriterError> {
        self.api.list_projects()
    }

    pub fn create_workspace_if_needed(&self) -> Result<bool, WriterError> {
        self.api.create_workspace_if_needed()
    }

    pub fn validate_workspace(&self) -> Result<bool, WriterError> {
        self.api.validate_workspace()
    }

    pub fn get_recent_edits(&self) -> Result<Vec<RecentEditDto>, WriterError> {
        self.api.get_recent_edits()
    }

    pub fn record_recent_edit(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<bool, WriterError> {
        self.api
            .record_recent_edit(&project_id, &volume_id, &chapter_id)
    }

    pub fn create_project(&self, title: String) -> Result<ProjectDto, WriterError> {
        self.api.create_project(&title)
    }

    pub fn create_project_envelope_json(&self, title: String) -> String {
        self.api.create_project_envelope_json(&title)
    }

    pub fn get_project_stats(&self, project_id: String) -> Result<ProjectStatsDto, WriterError> {
        self.api.get_project_stats(&project_id)
    }

    pub fn rename_project(
        &self,
        project_id: String,
        new_title: String,
    ) -> Result<bool, WriterError> {
        self.api.rename_project(&project_id, &new_title)
    }

    pub fn rename_project_envelope_json(&self, project_id: String, new_title: String) -> String {
        self.api
            .rename_project_envelope_json(&project_id, &new_title)
    }

    pub fn delete_project(&self, project_id: String) -> Result<bool, WriterError> {
        self.api.delete_project(&project_id)
    }

    pub fn reorder_projects(&self, ordered_project_ids: Vec<String>) -> Result<bool, WriterError> {
        self.api.reorder_projects(&ordered_project_ids)
    }

    pub fn reorder_projects_envelope_json(&self, ordered_project_ids: Vec<String>) -> String {
        self.api
            .reorder_projects_envelope_json(&ordered_project_ids)
    }

    pub fn list_volumes(&self, project_id: String) -> Result<Vec<VolumeDto>, WriterError> {
        self.api.list_volumes(&project_id)
    }

    pub fn create_volume(
        &self,
        project_id: String,
        title: String,
    ) -> Result<VolumeDto, WriterError> {
        self.api.create_volume(&project_id, &title)
    }

    pub fn create_volume_envelope_json(&self, project_id: String, title: String) -> String {
        self.api.create_volume_envelope_json(&project_id, &title)
    }

    pub fn rename_volume(
        &self,
        project_id: String,
        volume_id: String,
        new_title: String,
    ) -> Result<bool, WriterError> {
        self.api.rename_volume(&project_id, &volume_id, &new_title)
    }

    pub fn rename_volume_envelope_json(
        &self,
        project_id: String,
        volume_id: String,
        new_title: String,
    ) -> String {
        self.api
            .rename_volume_envelope_json(&project_id, &volume_id, &new_title)
    }

    pub fn delete_volume(
        &self,
        project_id: String,
        volume_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_volume(&project_id, &volume_id)
    }

    pub fn reorder_volumes(
        &self,
        project_id: String,
        ordered_volume_ids: Vec<String>,
    ) -> Result<bool, WriterError> {
        self.api.reorder_volumes(&project_id, &ordered_volume_ids)
    }

    pub fn reorder_volumes_envelope_json(
        &self,
        project_id: String,
        ordered_volume_ids: Vec<String>,
    ) -> String {
        self.api
            .reorder_volumes_envelope_json(&project_id, &ordered_volume_ids)
    }

    pub fn list_chapters(
        &self,
        project_id: String,
        volume_id: String,
    ) -> Result<Vec<ChapterMetaDto>, WriterError> {
        self.api.list_chapters(&project_id, &volume_id)
    }

    pub fn create_chapter(
        &self,
        project_id: String,
        volume_id: String,
        title: String,
    ) -> Result<ChapterMetaDto, WriterError> {
        self.api.create_chapter(&project_id, &volume_id, &title)
    }

    pub fn create_chapter_envelope_json(
        &self,
        project_id: String,
        volume_id: String,
        title: String,
    ) -> String {
        self.api
            .create_chapter_envelope_json(&project_id, &volume_id, &title)
    }

    pub fn rename_chapter(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        new_title: String,
    ) -> Result<bool, WriterError> {
        self.api
            .rename_chapter(&project_id, &volume_id, &chapter_id, &new_title)
    }

    pub fn rename_chapter_envelope_json(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        new_title: String,
    ) -> String {
        self.api
            .rename_chapter_envelope_json(&project_id, &volume_id, &chapter_id, &new_title)
    }

    pub fn delete_chapter(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<bool, WriterError> {
        self.api
            .delete_chapter(&project_id, &volume_id, &chapter_id)
    }

    pub fn reorder_chapters(
        &self,
        project_id: String,
        volume_id: String,
        ordered_chapter_ids: Vec<String>,
    ) -> Result<bool, WriterError> {
        self.api
            .reorder_chapters(&project_id, &volume_id, &ordered_chapter_ids)
    }

    pub fn reorder_chapters_envelope_json(
        &self,
        project_id: String,
        volume_id: String,
        ordered_chapter_ids: Vec<String>,
    ) -> String {
        self.api
            .reorder_chapters_envelope_json(&project_id, &volume_id, &ordered_chapter_ids)
    }

    pub fn open_chapter(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<ChapterContentDto, WriterError> {
        self.api.open_chapter(&project_id, &volume_id, &chapter_id)
    }

    pub fn save_chapter_content(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api
            .save_chapter_content(&project_id, &volume_id, &chapter_id, &content)
    }

    pub fn save_chapter_content_with_options(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
        allow_empty_overwrite: bool,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api.save_chapter_content_with_options(
            &project_id,
            &volume_id,
            &chapter_id,
            &content,
            allow_empty_overwrite,
        )
    }

    pub fn clear_chapter_content(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api
            .clear_chapter_content(&project_id, &volume_id, &chapter_id)
    }

    pub fn clear_chapter_content_envelope_json(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> String {
        self.api
            .clear_chapter_content_envelope_json(&project_id, &volume_id, &chapter_id)
    }

    pub fn update_chapter_note(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        note: String,
    ) -> Result<bool, WriterError> {
        self.api
            .update_chapter_note(&project_id, &volume_id, &chapter_id, &note)
    }

    pub fn update_chapter_note_envelope_json(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        note: String,
    ) -> String {
        self.api
            .update_chapter_note_envelope_json(&project_id, &volume_id, &chapter_id, &note)
    }

    pub fn load_local_settings(&self) -> Result<LocalSettingsDto, WriterError> {
        self.api.load_local_settings()
    }

    pub fn save_local_settings(&self, settings: LocalSettingsDto) -> Result<bool, WriterError> {
        self.api.save_local_settings(settings)
    }

    pub fn save_local_settings_envelope_json(&self, settings: LocalSettingsDto) -> String {
        self.api.save_local_settings_envelope_json(settings)
    }

    pub fn load_syncable_settings(&self) -> Result<SyncableSettingsDto, WriterError> {
        self.api.load_syncable_settings()
    }

    pub fn save_syncable_settings(
        &self,
        settings: SyncableSettingsDto,
    ) -> Result<bool, WriterError> {
        self.api.save_syncable_settings(settings)
    }

    pub fn save_syncable_settings_envelope_json(&self, settings: SyncableSettingsDto) -> String {
        self.api.save_syncable_settings_envelope_json(settings)
    }

    pub fn load_sync_config(&self) -> Result<SyncConfigDto, WriterError> {
        self.api.load_sync_config()
    }

    pub fn save_sync_config(&self, config: SyncConfigDto) -> Result<bool, WriterError> {
        self.api.save_sync_config(config)
    }

    pub fn load_sync_secrets(&self) -> Result<SyncSecretsDto, WriterError> {
        self.api.load_sync_secrets()
    }

    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> Result<bool, WriterError> {
        self.api.save_sync_secrets(secrets)
    }

    pub fn load_sync_state(&self) -> Result<SyncStateDto, WriterError> {
        self.api.load_sync_state()
    }

    pub fn perform_sync_diagnostics(
        &self,
        config: SyncConfigDto,
    ) -> Result<SyncDiagnosticsResultDto, WriterError> {
        self.api.perform_sync_diagnostics(config)
    }

    pub fn perform_sync_dry_run(&self, config: SyncConfigDto) -> Result<SyncPlanDto, WriterError> {
        self.api.perform_sync_dry_run(config)
    }

    pub fn perform_sync(&self, config: SyncConfigDto) -> Result<SyncResultDto, WriterError> {
        self.api.perform_sync(config)
    }

    // --- Sync envelope_json methods ---

    pub fn perform_sync_envelope_json(&self, config: SyncConfigDto) -> String {
        self.api.perform_sync_envelope_json(config)
    }

    pub fn perform_sync_dry_run_envelope_json(&self, config: SyncConfigDto) -> String {
        self.api.perform_sync_dry_run_envelope_json(config)
    }

    pub fn perform_sync_diagnostics_envelope_json(&self, config: SyncConfigDto) -> String {
        self.api.perform_sync_diagnostics_envelope_json(config)
    }

    pub fn save_sync_config_envelope_json(&self, config: SyncConfigDto) -> String {
        self.api.save_sync_config_envelope_json(config)
    }

    pub fn save_sync_secrets_envelope_json(&self, secrets: SyncSecretsDto) -> String {
        self.api.save_sync_secrets_envelope_json(secrets)
    }

    // --- Chapter/Project/Volume envelope_json methods ---

    pub fn save_chapter_content_envelope_json(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
    ) -> String {
        self.api.save_chapter_content_envelope_json(
            &project_id,
            &volume_id,
            &chapter_id,
            &content,
        )
    }

    pub fn delete_project_envelope_json(&self, project_id: String) -> String {
        self.api.delete_project_envelope_json(&project_id)
    }

    pub fn delete_volume_envelope_json(&self, project_id: String, volume_id: String) -> String {
        self.api.delete_volume_envelope_json(&project_id, &volume_id)
    }

    pub fn delete_chapter_envelope_json(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> String {
        self.api
            .delete_chapter_envelope_json(&project_id, &volume_id, &chapter_id)
    }

    pub fn get_writing_stats_summary(
        &self,
        start_date: String,
        end_date: String,
    ) -> Result<crate::api::types::WritingStatsSummaryDto, WriterError> {
        self.api.get_writing_stats_summary(&start_date, &end_date)
    }

    pub fn get_writing_stats_by_project(
        &self,
        start_date: String,
        end_date: String,
    ) -> Result<crate::api::types::ProjectStatsSummaryDto, WriterError> {
        self.api
            .get_writing_stats_by_project(&start_date, &end_date)
    }

    pub fn get_writing_stats_by_chapter(
        &self,
        start_date: String,
        end_date: String,
    ) -> Result<crate::api::types::ChapterStatsSummaryDto, WriterError> {
        self.api
            .get_writing_stats_by_chapter(&start_date, &end_date)
    }

    pub fn get_writing_stats_by_device(
        &self,
        start_date: String,
        end_date: String,
    ) -> Result<crate::api::types::DeviceStatsSummaryDto, WriterError> {
        self.api.get_writing_stats_by_device(&start_date, &end_date)
    }

    pub fn get_writing_speed_curve(
        &self,
        start_date: String,
        end_date: String,
        bucket_minutes: u32,
    ) -> Result<crate::api::types::SpeedCurveSummaryDto, WriterError> {
        self.api
            .get_writing_speed_curve(&start_date, &end_date, bucket_minutes)
    }

    pub fn calculate_word_count(&self, text: String) -> u32 {
        self.api.calculate_word_count(&text)
    }

    pub fn process_writing_event(
        &self,
        device_id: String,
        platform: String,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        old_text: String,
        new_text: String,
        session_id: String,
    ) -> Result<bool, WriterError> {
        self.api.process_writing_event(
            &device_id,
            &platform,
            &project_id,
            &volume_id,
            &chapter_id,
            &old_text,
            &new_text,
            &session_id,
        )
    }

    pub fn record_writing_event(
        &self,
        device_id: String,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        source: String,
        inserted_chars: i32,
        deleted_chars: i32,
        pasted_chars: i32,
        ai_inserted_chars: i32,
        session_id: String,
    ) -> Result<bool, WriterError> {
        self.api.record_writing_event(
            &device_id,
            &project_id,
            &volume_id,
            &chapter_id,
            &source,
            inserted_chars,
            deleted_chars,
            pasted_chars,
            ai_inserted_chars,
            &session_id,
        )
    }

    pub fn flush_writing_stats(&self) -> Result<bool, WriterError> {
        self.api.flush_writing_stats()
    }

    // MindMap
    pub fn get_mindmap_snapshot(
        &self,
        project_id: String,
    ) -> Result<crate::api::types::MindMapSnapshotDto, WriterError> {
        self.api.get_mind_map_snapshot(&project_id)
    }

    pub fn save_mindmap_graph(
        &self,
        project_id: String,
        graph: crate::api::types::MindMapGraphDto,
    ) -> Result<bool, WriterError> {
        self.api.save_mindmap_graph(&project_id, graph)
    }

    // StarMap
    pub fn list_starmaps(&self) -> Result<Vec<crate::api::types::StarMapMetaDto>, WriterError> {
        self.api.list_starmaps()
    }

    pub fn create_starmap(
        &self,
        title: String,
        desc: String,
    ) -> Result<crate::api::types::StarMapMetaDto, WriterError> {
        self.api.create_starmap(&title, &desc, None)
    }

    pub fn get_starmap_graph(
        &self,
        starmap_id: String,
    ) -> Result<crate::api::types::StarMapGraphDto, WriterError> {
        self.api.get_starmap_graph(&starmap_id)
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: String,
        node: crate::api::types::StarMapNodeDto,
        x: f32,
        y: f32,
    ) -> Result<crate::api::types::StarMapNodeDto, WriterError> {
        self.api.add_starmap_node(&starmap_id, node, x, y)
    }

    pub fn save_starmap_layout(
        &self,
        starmap_id: String,
        layout: crate::api::types::StarMapLayoutDto,
    ) -> Result<bool, WriterError> {
        self.api.save_starmap_layout(&starmap_id, &layout)
    }

    pub fn get_starmap_viewport(
        &self,
        starmap_id: String,
    ) -> Result<crate::api::types::StarMapViewportDto, WriterError> {
        self.api.get_starmap_viewport(&starmap_id)
    }

    pub fn save_starmap_viewport(
        &self,
        starmap_id: String,
        viewport: crate::api::types::StarMapViewportDto,
    ) -> Result<bool, WriterError> {
        self.api.save_starmap_viewport(&starmap_id, viewport)
    }

    pub fn compute_starmap_edge_renders(
        &self,
        graph: crate::api::types::StarMapGraphDto,
        layout: crate::api::types::StarMapLayoutDto,
    ) -> Result<Vec<crate::api::types::StarMapEdgeRenderDto>, WriterError> {
        self.api.compute_starmap_edge_renders(graph, layout)
    }

    pub fn hit_test_starmap_node(
        &self,
        layout: crate::api::types::StarMapLayoutDto,
        x: f32,
        y: f32,
    ) -> Result<Option<String>, WriterError> {
        self.api.hit_test_starmap_node(layout, x, y)
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: String,
        embed: crate::api::types::StarMapEmbedDto,
    ) -> Result<crate::api::types::StarMapEmbedDto, WriterError> {
        self.api.add_starmap_embed(&starmap_id, embed)
    }

    pub fn update_starmap_embed(
        &self,
        starmap_id: String,
        instance_id: String,
        patch: crate::api::types::StarMapEmbedPatchInputDto,
    ) -> Result<crate::api::types::StarMapEmbedDto, WriterError> {
        self.api
            .update_starmap_embed(&starmap_id, &instance_id, patch.into())
    }

    pub fn delete_starmap_embed(
        &self,
        starmap_id: String,
        instance_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_starmap_embed(&starmap_id, &instance_id)
    }

    pub fn add_starmap_link(
        &self,
        starmap_id: String,
        link: crate::api::types::StarMapLinkDto,
    ) -> Result<crate::api::types::StarMapLinkDto, WriterError> {
        self.api.add_starmap_link(&starmap_id, link)
    }

    pub fn update_starmap_link(
        &self,
        starmap_id: String,
        link_id: String,
        patch: crate::api::types::StarMapLinkPatchInputDto,
    ) -> Result<crate::api::types::StarMapLinkDto, WriterError> {
        self.api
            .update_starmap_link(&starmap_id, &link_id, patch.into())
    }

    pub fn delete_starmap_link(
        &self,
        starmap_id: String,
        link_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_starmap_link(&starmap_id, &link_id)
    }

    pub fn find_starmap_references(
        &self,
        target_starmap_id: String,
    ) -> Result<Vec<crate::api::types::StarMapReferenceDto>, WriterError> {
        self.api.find_starmap_references(&target_starmap_id)
    }

    // Actions
    pub fn list_registered_actions(
        &self,
    ) -> Result<Vec<crate::api::types::ActionDescriptorDto>, WriterError> {
        self.api.list_registered_actions()
    }

    pub fn execute_action(
        &self,
        action_id: String,
        args_json: String,
        context_json: String,
    ) -> Result<crate::api::types::ActionResultDto, WriterError> {
        self.api
            .execute_action_ext(&action_id, &args_json, &context_json)
    }

    pub fn ai_available(&self) -> bool {
        self.api.ai_available()
    }
}
