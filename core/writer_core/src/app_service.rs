use crate::api::{
    ChapterContentDto, ChapterMetaDto, ChapterSaveReceiptDto, LocalSettingsDto, ProjectDto,
    ProjectStatsDto, RecentEditDto, SyncConfigDto, SyncDiagnosticsResultDto, SyncPlanDto,
    SyncResultDto, SyncSecretsDto, SyncStateDto, SyncableSettingsDto, VolumeDto, WriterCoreApi,
    WriterError,
};

use std::sync::Mutex;

/// 旧版编辑会话 — 仅用于正文章节的 legacy 路径。
///
/// generation 在 load_text **之前**递增，使过期的 composition 操作被内核拒绝。
/// 这保证 reset 期间任何异步到达的 composition update/finish/cancel
/// 因 generation 不匹配而被 StaleRevision 拒绝，不会写入已重置的正文。
///
/// 新代码应使用 TextEditSessionRegistry（支持多目标会话）。
struct EditorSession {
    kernel: crate::editor::EditorKernel,
    chapter_id: Option<String>,
    /// 会话 generation——每次 load_text 前递增，用于拒绝过期 composition 操作。
    generation: u64,
}

/// Thin UniFFI adapter. Stable Core API behavior lives in `api::WriterCoreApi`.
///
/// ## 线程安全
///
/// `editor_session` 和 `session_registry` 各自用 `Mutex` 保护，保证线程安全。
/// `Mutex` 只在单次 FFI 调用期间持有，不跨调用持有，避免死锁。
/// **不得在持有 `editor_session` 锁的同时获取 `session_registry` 锁**（锁序：先 session 后 registry）。
///
/// ## 双会话路径
///
/// `editor_session` 是旧版正文章节专用路径（单 EditorKernel，单 generation），
/// `session_registry` 是新版多目标会话路径（项目名/章节名/星图标题/正文等，各自独立 EditorKernel 和 generation）。
/// 两者独立维护，不共享 EditorKernel 实例。同一时刻同一章节只能通过一条路径访问。
pub struct WriterAppService {
    api: WriterCoreApi,
    /// 旧版正文章节会话——单 EditorKernel，generation 在 load_text 前递增。
    editor_session: Mutex<EditorSession>,
    /// 新版多目标会话注册表——每个目标独立 EditorKernel 和 generation。
    session_registry: Mutex<crate::editor::TextEditSessionRegistry>,
}

impl WriterAppService {
    pub fn new(workspace_path: String) -> Self {
        Self {
            api: WriterCoreApi::new(workspace_path),
            editor_session: Mutex::new(EditorSession {
                kernel: crate::editor::EditorKernel::new(),
                chapter_id: None,
                generation: 0,
            }),
            session_registry: Mutex::new(crate::editor::TextEditSessionRegistry::new()),
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

    pub fn flush_recent_edits(&self) -> Result<bool, WriterError> {
        self.api.flush_recent_edits()
    }

    pub fn create_project(&self, title: String) -> Result<ProjectDto, WriterError> {
        self.api.create_project(&title)
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

    pub fn delete_project(&self, project_id: String) -> Result<bool, WriterError> {
        self.api.delete_project(&project_id)
    }

    pub fn reorder_projects(&self, ordered_project_ids: Vec<String>) -> Result<bool, WriterError> {
        self.api.reorder_projects(&ordered_project_ids)
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

    pub fn rename_volume(
        &self,
        project_id: String,
        volume_id: String,
        new_title: String,
    ) -> Result<bool, WriterError> {
        self.api.rename_volume(&project_id, &volume_id, &new_title)
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

    pub fn create_chapter_in_project(
        &self,
        project_id: String,
        title: String,
    ) -> Result<ChapterMetaDto, WriterError> {
        self.api.create_chapter_in_project(&project_id, &title)
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

    pub fn load_local_settings(&self) -> Result<LocalSettingsDto, WriterError> {
        self.api.load_local_settings()
    }

    pub fn save_local_settings(&self, settings: LocalSettingsDto) -> Result<bool, WriterError> {
        self.api.save_local_settings(settings)
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

    pub fn get_sync_capability(&self) -> Result<crate::api::SyncCapabilityDto, WriterError> {
        self.api.get_sync_capability()
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

    pub fn perform_sync(&self, config: SyncConfigDto, force_sync: bool) -> Result<SyncResultDto, WriterError> {
        self.api.perform_sync(config, force_sync)
    }

    pub fn resolve_conflict_keep_local(&self, path: String) -> Result<bool, WriterError> {
        self.api.resolve_conflict_keep_local(&path)
    }

    pub fn resolve_conflict_take_remote(&self, path: String) -> Result<bool, WriterError> {
        self.api.resolve_conflict_take_remote(&path)
    }

    pub fn resolve_conflict_mark_merged(&self, path: String) -> Result<bool, WriterError> {
        self.api.resolve_conflict_mark_merged(&path)
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

    #[allow(clippy::too_many_arguments)]
    pub fn process_writing_event(
        &self,
        device_id: String,
        platform: String,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        old_text: String,
        new_text: String,
        duration_seconds: u32,
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
            duration_seconds,
            &session_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
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
        duration_seconds: i32,
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
            duration_seconds,
            &session_id,
        )
    }

    pub fn flush_writing_stats(&self) -> Result<bool, WriterError> {
        self.api.flush_writing_stats()
    }

    pub fn ensure_device_info(
        &self,
        platform: String,
        device_class: String,
    ) -> Result<bool, WriterError> {
        self.api.ensure_device_info(&platform, &device_class)
    }

    pub fn load_device_info(&self) -> Result<crate::api::types::DeviceInfoDto, WriterError> {
        self.api.load_device_info()
    }

    pub fn save_palette_record(
        &self,
        record: crate::api::types::ThemePaletteRecordDto,
    ) -> Result<bool, WriterError> {
        self.api.save_palette_record(record)
    }

    pub fn load_palette_record(
        &self,
        device_id: String,
        fingerprint: String,
    ) -> Result<crate::api::types::ThemePaletteRecordDto, WriterError> {
        self.api.load_palette_record(&device_id, &fingerprint)
    }

    pub fn list_palette_records(&self) -> Result<Vec<crate::api::types::ThemePaletteRecordDto>, WriterError> {
        self.api.list_palette_records()
    }

    pub fn delete_palette_record(
        &self,
        device_id: String,
        fingerprint: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_palette_record(&device_id, &fingerprint)
    }

    pub fn migrate_legacy_theme_palette(&self) -> Result<bool, WriterError> {
        self.api.migrate_legacy_theme_palette()
    }

    pub fn compute_palette_fingerprint(
        &self,
        light_scheme: crate::api::types::ThemeColorSchemeDto,
        dark_scheme: crate::api::types::ThemeColorSchemeDto,
    ) -> String {
        self.api.compute_palette_fingerprint(light_scheme, dark_scheme)
    }

    pub fn list_builtin_themes(&self) -> Vec<crate::api::types::BuiltinThemeDto> {
        self.api.list_builtin_themes()
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

    pub fn update_starmap_node(
        &self,
        starmap_id: String,
        node_id: String,
        patch: crate::api::types::StarMapNodePatchInputDto,
    ) -> Result<crate::api::types::StarMapNodeDto, WriterError> {
        self.api.update_starmap_node(&starmap_id, &node_id, patch.into())
    }

    pub fn delete_starmap_node(
        &self,
        starmap_id: String,
        node_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_starmap_node(&starmap_id, &node_id)
    }

    pub fn add_starmap_edge(
        &self,
        starmap_id: String,
        edge: crate::api::types::StarMapEdgeDto,
    ) -> Result<crate::api::types::StarMapEdgeDto, WriterError> {
        self.api.add_starmap_edge(&starmap_id, edge)
    }

    pub fn update_starmap_edge(
        &self,
        starmap_id: String,
        edge_id: String,
        patch: crate::api::types::StarMapEdgePatchInputDto,
    ) -> Result<crate::api::types::StarMapEdgeDto, WriterError> {
        self.api.update_starmap_edge(&starmap_id, &edge_id, patch.into())
    }

    pub fn delete_starmap_edge(
        &self,
        starmap_id: String,
        edge_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_starmap_edge(&starmap_id, &edge_id)
    }

    pub fn save_starmap_graph(
        &self,
        starmap_id: String,
        graph: crate::api::types::StarMapGraphDto,
    ) -> Result<bool, WriterError> {
        self.api.save_starmap_graph(&starmap_id, &graph)
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

    pub fn get_starmap_motion_policy(
        &self,
    ) -> Result<crate::api::types::StarMapMotionPolicyDto, WriterError> {
        self.api.get_starmap_motion_policy()
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

    // ── Layout Policy ──

    pub fn resolve_layout(
        &self,
        metrics: crate::api::WindowMetricsDto,
    ) -> crate::api::LayoutPlanDto {
        let core_metrics: crate::layout_policy::WindowMetrics = metrics.into();
        let plan = crate::layout_policy::resolve_layout(&core_metrics);
        plan.into()
    }

    // ── Screen Policy ──

    pub fn resolve_screen_policy(
        &self,
        screen_role: crate::api::ScreenRoleDto,
        shell_mode: crate::api::ShellModeDto,
    ) -> crate::api::ScreenPolicyDto {
        let core_role: crate::screen_policy::ScreenRole = screen_role.into();
        let core_mode: crate::layout_policy::ShellMode = shell_mode.into();
        let action_slots = crate::screen_policy::resolve_screen_policy(core_role, core_mode);
        crate::api::ScreenPolicyDto {
            screen_role: core_role.into(),
            action_slots: action_slots.into_iter().map(Into::into).collect(),
        }
    }

    /// 获取旧版编辑会话的可变引用。
    ///
    /// Mutex 中毒恢复策略：如果前一个线程 panic 时持有了锁，
    /// `into_inner()` 取出内部数据继续使用，而非让所有后续 FFI 调用全部失败。
    /// 这在 UniFFI 场景下是合理的——平台端持有 WriterAppService 的全局引用，
    /// 一次编辑操作的 panic 不应使整个应用不可用。
    fn with_session<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut EditorSession) -> R,
    {
        let mut session = self.editor_session.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut session)
    }

    /// 插入文本 — 旧版编辑会话路径。
    ///
    /// 前置条件：byte_offset 必须是合法 UTF-8 char boundary 且 <= 正文长度，
    /// expected_revision 必须与当前 kernel revision 匹配。
    /// 不满足时内核返回 InvalidOffset/StaleRevision，平台端需刷新后重试。
    /// 所有 offset 均为 UTF-8 byte offset，平台端需在调用前完成 UTF-16→UTF-8 转换。
    pub fn editor_kernel_insert(
        &self,
        byte_offset: u32,
        text: String,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Insert {
                byte_offset: byte_offset as usize,
                text,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    /// 删除文本 — 旧版编辑会话路径。
    ///
    /// 前置条件：byte_start/byte_end_exclusive 必须是合法 char boundary，
    /// byte_start < byte_end_exclusive，expected_revision 匹配当前 revision。
    /// `[byte_start, byte_end_exclusive)` 为半开区间（UTF-8 byte offset）。
    pub fn editor_kernel_delete(
        &self,
        byte_start: u32,
        byte_end_exclusive: u32,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Delete {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                deleted_text: String::new(),
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    /// 替换文本 — 旧版编辑会话路径。
    ///
    /// 用 replacement_text 替换 `[byte_start, byte_end_exclusive)` 半开区间的文本。
    /// 前置条件同 delete：offset 合法 char boundary、range 有效、revision 匹配。
    pub fn editor_kernel_replace(
        &self,
        byte_start: u32,
        byte_end_exclusive: u32,
        replacement_text: String,
        original_text: String,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Replace {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                replacement_text,
                original_text,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_set_selection(
        &self,
        anchor_byte_offset: u32,
        head_byte_offset: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::SetSelection {
                anchor_byte_offset: anchor_byte_offset as usize,
                head_byte_offset: head_byte_offset as usize,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_undo(&self, expected_revision: u64) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Undo { expected_revision });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_redo(&self, expected_revision: u64) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Redo { expected_revision });
            result.into_result().into()
        })
    }

    /// 加载文本到旧版编辑会话——generation 在 load_text 前递增，
    /// 使过期的 composition 操作被内核拒绝。
    /// cursor_byte_offset 为 UTF-8 byte offset，由平台端从 UTF-16 转换。
    pub fn editor_kernel_load_text(
        &self,
        text: String,
        cursor_byte_offset: u32,
    ) -> crate::api::EditorEditResultDto {
        self.with_session(|s| {
            s.generation = s.generation.saturating_add(1);
            let result = s.kernel.load_text(text, cursor_byte_offset as usize);
            result.into_result().into()
        })
    }

    pub fn editor_kernel_composition_update_visual_intent(
        &self,
        composition_replace_start: u32,
        composition_replace_end_exclusive: u32,
        old_preedit_text: String,
        new_preedit_text: String,
    ) -> crate::api::EditorVisualIntentDto {
        self.with_session(|s| {
            let replace_range = if composition_replace_start < composition_replace_end_exclusive {
                Some((composition_replace_start as usize, composition_replace_end_exclusive as usize))
            } else {
                None
            };
            let intent = s.kernel.composition_update_visual_intent(
                replace_range,
                &old_preedit_text,
                &new_preedit_text,
            );
            intent.into()
        })
    }

    pub fn editor_kernel_commit_text(
        &self,
        byte_start: u32,
        byte_end_exclusive: u32,
        replacement_text: String,
        resulting_selection_anchor: u32,
        resulting_selection_head: u32,
        composition_session_id: u64,
        composition_base_revision: u64,
        composition_generation: u64,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::CommitText {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                replacement_text,
                resulting_selection_anchor: resulting_selection_anchor as usize,
                resulting_selection_head: resulting_selection_head as usize,
                composition_session_id,
                composition_base_revision,
                composition_generation,
                cause: core_cause,
                expected_revision,
            });
            result.into()
        })
    }

    pub fn editor_kernel_delete_surrounding(
        &self,
        before_byte_start: u32,
        before_byte_end_exclusive: u32,
        after_byte_start: u32,
        after_byte_end_exclusive: u32,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::DeleteSurrounding {
                before_byte_start: before_byte_start as usize,
                before_byte_end_exclusive: before_byte_end_exclusive as usize,
                after_byte_start: after_byte_start as usize,
                after_byte_end_exclusive: after_byte_end_exclusive as usize,
                cause: core_cause,
                expected_revision,
            });
            result.into()
        })
    }

    /// 开始 IME composition 会话。
    ///
    /// replace_start/replace_end_exclusive 为 committed 正文坐标（UTF-8 byte offset，半开区间）。
    /// 成功时返回的 DTO 包含 composition_session（session_id, base_revision, generation），
    /// 平台端必须在后续 UpdateComposition/FinishComposition/CancelComposition 中携带此信息。
    /// generation 用于过期检测——session reset 后旧 generation 的操作被内核拒绝。
    pub fn editor_kernel_begin_composition(
        &self,
        replace_start: u32,
        replace_end_exclusive: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::BeginComposition {
                replace_start: replace_start as usize,
                replace_end_exclusive: replace_end_exclusive as usize,
                expected_revision,
            });
            let mut dto: crate::api::EditorEditResultDto = result.into();
            if dto.outcome == crate::api::EditorEditOutcomeDto::Applied
                || dto.outcome == crate::api::EditorEditOutcomeDto::AppliedWithAdjustedSelection
            {
                if let Some((session_id, base_revision, generation)) = s.kernel.composition_session_info() {
                    dto.composition_session = Some(crate::api::types::CompositionSessionDto {
                        session_id,
                        base_revision,
                        generation,
                    });
                }
            }
            dto
        })
    }

    /// 更新 IME 预输入文本。
    ///
    /// composition_generation 必须与会话当前 generation 匹配，否则返回 StaleRevision。
    /// new_preedit_cursor_offset 为 preedit 内部 UTF-8 byte offset（非 committed 正文坐标）。
    pub fn editor_kernel_update_composition(
        &self,
        composition_session_id: u64,
        composition_generation: u64,
        new_preedit_text: String,
        new_preedit_cursor_offset: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::UpdateComposition {
                composition_session_id,
                composition_generation,
                new_preedit_text,
                new_preedit_cursor_offset: new_preedit_cursor_offset as usize,
                expected_revision,
            });
            result.into()
        })
    }

    /// 完成 IME composition——将预输入文本提交到 committed 正文。
    /// commit 后 composition session 被销毁，后续 composition 操作需重新 BeginComposition。
    pub fn editor_kernel_finish_composition(
        &self,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::FinishComposition {
                composition_session_id,
                composition_generation,
                expected_revision,
            });
            result.into()
        })
    }

    /// 取消 IME composition——丢弃预输入文本，恢复 committed 正文。
    /// cancel 后 composition session 被销毁，committed 正文不变。
    pub fn editor_kernel_cancel_composition(
        &self,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::CancelComposition {
                composition_session_id,
                composition_generation,
                expected_revision,
            });
            result.into()
        })
    }

    pub fn editor_kernel_composition_session_info(&self) -> Option<(u64, u64, u64)> {
        self.with_session(|s| {
            s.kernel.composition_session_info()
        })
    }

    pub fn editor_kernel_set_animation_enabled(
        &self,
        enabled: u8,
    ) {
        self.with_session(|s| {
            s.kernel.set_animation_enabled(enabled != 0);
        })
    }

    pub fn editor_kernel_set_animation_duration_ms(
        &self,
        duration_ms: u64,
    ) {
        self.with_session(|s| {
            s.kernel.set_animation_duration_ms(duration_ms);
        })
    }

    pub fn editor_kernel_get_text(&self) -> String {
        self.with_session(|s| {
            s.kernel.text().to_string()
        })
    }

    pub fn editor_kernel_get_revision(&self) -> u64 {
        self.with_session(|s| {
            s.kernel.revision()
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn editor_kernel_get_cursor(&self) -> u32 {
        self.with_session(|s| {
            s.kernel.cursor() as u32
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn editor_kernel_get_selection_anchor(&self) -> u32 {
        self.with_session(|s| {
            s.kernel.selection_anchor() as u32
        })
    }

    /// 获取编辑会话快照 — 包含正文、revision、光标、选区和 generation。
    ///
    /// 用途：平台端在镜像与内核不一致时（如 revision 不连续），
    /// 用此快照完整重建 CommittedTextMirror，而非依赖增量 patch。
    /// generation 用于 composition 操作的过期检测。
    #[allow(clippy::cast_possible_truncation)]
    pub fn editor_kernel_session_snapshot(&self) -> crate::api::EditorSessionSnapshotDto {
        self.with_session(|s| {
            crate::api::EditorSessionSnapshotDto {
                text: s.kernel.text().to_string(),
                revision: s.kernel.revision(),
                cursor: s.kernel.cursor() as u32,
                selection_anchor: s.kernel.selection_anchor() as u32,
                generation: s.generation,
                chapter_id: s.chapter_id.clone().unwrap_or_default(),
            }
        })
    }

    pub fn editor_kernel_replace_all(
        &self,
        search: String,
        replacement: String,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::ReplaceAll {
                search,
                replacement,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_insert_line_break(
        &self,
        byte_offset: u32,
        auto_indent_prefix: String,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::InsertLineBreak {
                byte_offset: byte_offset as usize,
                auto_indent_prefix,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    // ── #541: Text Edit Session API ──

    fn with_registry<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut crate::editor::TextEditSessionRegistry) -> R,
    {
        let mut registry = self.session_registry.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut registry)
    }

    fn with_session_in_registry<F, R>(&self, session_id: u64, f: F) -> Option<R>
    where
        F: FnOnce(&mut crate::editor::TextEditSession) -> R,
    {
        let mut registry = self.session_registry.lock().unwrap_or_else(|e| e.into_inner());
        registry.get_session_mut(crate::editor::TextEditSessionId(session_id)).map(f)
    }

    pub fn text_edit_session_create(
        &self,
        target_id: String,
        initial_text: String,
        initial_cursor_byte_offset: u32,
        is_persistent: u8,
    ) -> Option<u64> {
        let result = self.with_registry(|r| {
            r.create_session(
                target_id,
                initial_text,
                initial_cursor_byte_offset as usize,
                is_persistent != 0,
            )
        });
        match result {
            Ok(id) => Some(id.0),
            Err(_) => None,
        }
    }

    pub fn text_edit_session_close(&self, session_id: u64) -> u8 {
        self.with_registry(|r| {
            if r.close_session(crate::editor::TextEditSessionId(session_id)) {
                1u8
            } else {
                0u8
            }
        })
    }

    /// 重置编辑会话 — 加载新文本并递增 generation。
    ///
    /// 不变量：generation 在 load_text **之前**递增，使任何异步到达的
    /// composition update/finish/cancel 因 generation 不匹配而被内核拒绝，
    /// 不会写入已重置的正文。这与 EditorSession 的 generation 语义一致。
    pub fn text_edit_session_reset(
        &self,
        session_id: u64,
        text: String,
        cursor_byte_offset: u32,
    ) -> u8 {
        match self.with_session_in_registry(session_id, |s| {
            s.generation = s.generation.saturating_add(1);
            let result = s.kernel.load_text(text, cursor_byte_offset as usize);
            result.is_applied()
        }) {
            Some(true) => 1u8,
            _ => 0u8,
        }
    }

    pub fn text_edit_session_insert(
        &self,
        session_id: u64,
        byte_offset: u32,
        text: String,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Insert {
                byte_offset: byte_offset as usize,
                text,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    pub fn text_edit_session_delete(
        &self,
        session_id: u64,
        byte_start: u32,
        byte_end_exclusive: u32,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Delete {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                deleted_text: String::new(),
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    pub fn text_edit_session_replace(
        &self,
        session_id: u64,
        byte_start: u32,
        byte_end_exclusive: u32,
        replacement_text: String,
        original_text: String,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Replace {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                replacement_text,
                original_text,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    pub fn text_edit_session_set_selection(
        &self,
        session_id: u64,
        anchor_byte_offset: u32,
        head_byte_offset: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::SetSelection {
                anchor_byte_offset: anchor_byte_offset as usize,
                head_byte_offset: head_byte_offset as usize,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    pub fn text_edit_session_undo(
        &self,
        session_id: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Undo { expected_revision });
            result.into_result().into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    pub fn text_edit_session_redo(
        &self,
        session_id: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Redo { expected_revision });
            result.into_result().into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    /// Load text into a session, replacing all content. Generation is incremented before
    /// load_text so that any in-flight composition operations with the old generation are
    /// rejected — same invariant as text_edit_session_reset.
    pub fn text_edit_session_load_text(
        &self,
        session_id: u64,
        text: String,
        cursor_byte_offset: u32,
    ) -> crate::api::EditorEditResultDto {
        self.with_session_in_registry(session_id, |s| {
            s.generation = s.generation.saturating_add(1);
            let result = s.kernel.load_text(text, cursor_byte_offset as usize);
            result.into_result().into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    /// Commit text with composition session validation. composition_session_id/base_revision/
    /// generation allow the kernel to verify the commit corresponds to an active composition.
    /// If the session has been reset (generation mismatch), the commit is rejected.
    pub fn text_edit_session_commit_text(
        &self,
        session_id: u64,
        byte_start: u32,
        byte_end_exclusive: u32,
        replacement_text: String,
        resulting_selection_anchor: u32,
        resulting_selection_head: u32,
        composition_session_id: u64,
        composition_base_revision: u64,
        composition_generation: u64,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::CommitText {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                replacement_text,
                resulting_selection_anchor: resulting_selection_anchor as usize,
                resulting_selection_head: resulting_selection_head as usize,
                composition_session_id,
                composition_base_revision,
                composition_generation,
                cause: core_cause,
                expected_revision,
            });
            result.into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    pub fn text_edit_session_delete_surrounding(
        &self,
        session_id: u64,
        before_byte_start: u32,
        before_byte_end_exclusive: u32,
        after_byte_start: u32,
        after_byte_end_exclusive: u32,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::DeleteSurrounding {
                before_byte_start: before_byte_start as usize,
                before_byte_end_exclusive: before_byte_end_exclusive as usize,
                after_byte_start: after_byte_start as usize,
                after_byte_end_exclusive: after_byte_end_exclusive as usize,
                cause: core_cause,
                expected_revision,
            });
            result.into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    /// Begin a composition session. Returns composition_session (sessionId, baseRevision,
    /// generation) on success so the platform can track the composition lifecycle.
    /// The kernel validates these on subsequent updateComposition/finishComposition/cancelComposition.
    pub fn text_edit_session_begin_composition(
        &self,
        session_id: u64,
        replace_start: u32,
        replace_end_exclusive: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::BeginComposition {
                replace_start: replace_start as usize,
                replace_end_exclusive: replace_end_exclusive as usize,
                expected_revision,
            });
            let mut dto: crate::api::EditorEditResultDto = result.into();
            if dto.outcome == crate::api::EditorEditOutcomeDto::Applied
                || dto.outcome == crate::api::EditorEditOutcomeDto::AppliedWithAdjustedSelection
            {
                if let Some((cs_id, base_rev, gen)) = s.kernel.composition_session_info() {
                    dto.composition_session = Some(crate::api::types::CompositionSessionDto {
                        session_id: cs_id,
                        base_revision: base_rev,
                        generation: gen,
                    });
                }
            }
            dto
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    /// Update an active composition. composition_generation must match the kernel's current
    /// composition generation; a mismatch results in STALE_REVISION outcome.
    pub fn text_edit_session_update_composition(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        new_preedit_text: String,
        new_preedit_cursor_offset: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::UpdateComposition {
                composition_session_id,
                composition_generation,
                new_preedit_text,
                new_preedit_cursor_offset: new_preedit_cursor_offset as usize,
                expected_revision,
            });
            result.into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    pub fn text_edit_session_finish_composition(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::FinishComposition {
                composition_session_id,
                composition_generation,
                expected_revision,
            });
            result.into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    pub fn text_edit_session_cancel_composition(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::CancelComposition {
                composition_session_id,
                composition_generation,
                expected_revision,
            });
            result.into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    pub fn text_edit_session_composition_update_visual_intent(
        &self,
        session_id: u64,
        composition_replace_start: u32,
        composition_replace_end_exclusive: u32,
        old_preedit_text: String,
        new_preedit_text: String,
    ) -> crate::api::EditorVisualIntentDto {
        self.with_session_in_registry(session_id, |s| {
            let replace_range = if composition_replace_start < composition_replace_end_exclusive {
                Some((composition_replace_start as usize, composition_replace_end_exclusive as usize))
            } else {
                None
            };
            let intent = s.kernel.composition_update_visual_intent(
                replace_range,
                &old_preedit_text,
                &new_preedit_text,
            );
            intent.into()
        })
        .unwrap_or_else(|| crate::api::EditorVisualIntentDto::default_fallback())
    }

    pub fn text_edit_session_set_animation_enabled(
        &self,
        session_id: u64,
        enabled: u8,
    ) {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.set_animation_enabled(enabled != 0);
        });
    }

    pub fn text_edit_session_set_animation_duration_ms(
        &self,
        session_id: u64,
        duration_ms: u64,
    ) {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.set_animation_duration_ms(duration_ms);
        });
    }

    pub fn text_edit_session_get_text(&self, session_id: u64) -> String {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.text().to_string()
        })
        .unwrap_or_default()
    }

    pub fn text_edit_session_get_revision(&self, session_id: u64) -> u64 {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.revision()
        })
        .unwrap_or(0)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn text_edit_session_snapshot(&self, session_id: u64) -> crate::api::EditorSessionSnapshotDto {
        self.with_session_in_registry(session_id, |s| {
            crate::api::EditorSessionSnapshotDto {
                text: s.kernel.text().to_string(),
                revision: s.kernel.revision(),
                cursor: s.kernel.cursor() as u32,
                selection_anchor: s.kernel.selection_anchor() as u32,
                generation: s.generation,
                chapter_id: s.target_id.clone(),
            }
        })
        .unwrap_or_else(|| crate::api::EditorSessionSnapshotDto {
            text: String::new(),
            revision: 0,
            cursor: 0,
            selection_anchor: 0,
            generation: 0,
            chapter_id: String::new(),
        })
    }

    pub fn text_edit_session_replace_all(
        &self,
        session_id: u64,
        search: String,
        replacement: String,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::ReplaceAll {
                search,
                replacement,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }

    pub fn text_edit_session_insert_line_break(
        &self,
        session_id: u64,
        byte_offset: u32,
        auto_indent_prefix: String,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::InsertLineBreak {
                byte_offset: byte_offset as usize,
                auto_indent_prefix,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(|| crate::api::EditorEditResultDto::stale_fallback())
    }
}
