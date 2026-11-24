use crate::chapter::{self, Chapter, ChapterContent, ChapterSaveReceipt};
use crate::error::Result;
use crate::project::{self, Project, ProjectSummary};
use crate::storage::workspace_git::WorkspaceChangeSet;
use crate::volume::{self, Volume};

impl super::WriterCore {
    /// 一个工作区一个 Git 仓库。list 直接读 `project.json`，
    /// 不再按 project_id 构造 layout_fn / git_dir。
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        project::list_projects(&self.projects_root)
    }

    /// 批量返回项目摘要（元数据 统计）。
    /// 直接走 `project::list_project_summaries`，不再构造 layout_fn。
    pub fn list_project_summaries(&self) -> Result<Vec<ProjectSummary>> {
        project::list_project_summaries(&self.projects_root)
    }

    /// 一个工作区一个 Git 仓库。create_project 只创建作品目录、
    /// `project.json`、`volumes/`、`characters/` 和默认卷，不再初始化作品级 `.git/`。
    pub fn create_project(&self, title: &str) -> Result<Project> {
        project::create_project(&self.projects_root, title)
    }

    /// 恢复/导入项目——使用 manifest 中的稳定 ID。
    pub fn create_project_with_id(&self, id: &str, title: &str, order: i32) -> Result<Project> {
        project::create_project_with_id(&self.projects_root, id, title, order)
    }

    pub fn list_volumes(&self, project_id: &str) -> Result<Vec<Volume>> {
        let project_root = self.project_root(project_id);
        volume::list_volumes(&project_root)
    }

    pub fn create_volume(&self, project_id: &str, title: &str) -> Result<Volume> {
        let project_root = self.project_root(project_id);
        volume::create_volume(&project_root, title)
    }

    /// 恢复/导入卷——使用 manifest 中的稳定 ID。
    pub fn create_volume_with_id(
        &self,
        project_id: &str,
        id: &str,
        title: &str,
        order: i32,
    ) -> Result<Volume> {
        let project_root = self.project_root(project_id);
        volume::create_volume_with_id(&project_root, id, title, order)
    }

    pub fn list_valid_chapter_ids(
        &self,
        project_id: &str,
    ) -> Result<std::collections::HashSet<String>> {
        let project_root = self.project_root(project_id);
        chapter::list_valid_chapter_ids(&project_root)
    }

    pub fn list_chapters(&self, project_id: &str, volume_id: &str) -> Result<Vec<Chapter>> {
        let project_root = self.project_root(project_id);
        chapter::list_chapters(&project_root, volume_id)
    }

    pub fn calculate_word_count(&self, text: &str) -> u32 {
        chapter::calculate_word_count(text)
    }

    pub fn create_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        title: &str,
    ) -> Result<Chapter> {
        let project_root = self.project_root(project_id);
        chapter::create_chapter(&project_root, volume_id, title)
    }

    /// 恢复/导入章节——使用 manifest 中的稳定 ID。
    pub fn create_chapter_with_id(
        &self,
        project_id: &str,
        volume_id: &str,
        id: &str,
        title: &str,
        order: i32,
    ) -> Result<Chapter> {
        let project_root = self.project_root(project_id);
        chapter::create_chapter_with_id(&project_root, volume_id, id, title, order)
    }

    pub fn get_project_stats(&self, project_id: &str) -> Result<crate::project::ProjectStats> {
        let project_root = self.project_root(project_id);
        crate::project::get_project_stats(&project_root)
    }

    pub fn read_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<ChapterContent> {
        let project_root = self.project_root(project_id);
        chapter::read_chapter(&project_root, volume_id, chapter_id)
    }

    pub fn open_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<super::ChapterOpenResult> {
        let content = self.read_chapter(project_id, volume_id, chapter_id)?;
        Ok(super::ChapterOpenResult {
            meta: content.meta,
            content: content.content,
            version: content.version,
        })
    }

    /// 持有 workspace guard 的打开入口：先恢复未完成 journal，再读取一致版本。
    pub fn open_chapter_recovering(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        workspace_lock: &crate::storage::WorkspaceWriteLock,
    ) -> Result<super::ChapterOpenResult> {
        let project_root = self.project_root(project_id);
        let (meta, content, version) = crate::chapter::commit::read_chapter_state_recovering(
            &project_root,
            workspace_lock,
            volume_id,
            chapter_id,
        )?;
        Ok(super::ChapterOpenResult {
            meta,
            content,
            version,
        })
    }

    /// 持有 workspace guard 的版本读取。
    pub fn read_chapter_version_recovering(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        workspace_lock: &crate::storage::WorkspaceWriteLock,
    ) -> Result<crate::chapter::ChapterVersion> {
        let project_root = self.project_root(project_id);
        crate::chapter::commit::read_chapter_version_recovering(
            &project_root,
            workspace_lock,
            volume_id,
            chapter_id,
        )
    }

    /// 只读当前章节版本（不复制整章正文，但仍需读取正文计算摘要）。
    pub fn read_chapter_version(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<crate::chapter::ChapterVersion> {
        let project_root = self.project_root(project_id);
        chapter::commit::read_chapter_version(&project_root, volume_id, chapter_id)
    }

    pub fn write_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> Result<()> {
        let project_root = self.project_root(project_id);
        chapter::save_chapter(&project_root, volume_id, chapter_id, content)
    }

    pub fn clear_chapter_content(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<()> {
        let project_root = self.project_root(project_id);
        chapter::clear_chapter_content(&project_root, volume_id, chapter_id)
    }

    pub fn clear_chapter_content_verified(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<ChapterSaveReceipt> {
        let project_root = self.project_root(project_id);
        chapter::clear_chapter_content_verified(&project_root, volume_id, chapter_id)
    }

    pub fn write_chapter_verified(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> Result<ChapterSaveReceipt> {
        let project_root = self.project_root(project_id);
        chapter::save_chapter_verified(&project_root, volume_id, chapter_id, content)
    }

    ///   保存章节正文并返回变更集。
    pub fn write_chapter_verified_with_changes(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> Result<(ChapterSaveReceipt, WorkspaceChangeSet)> {
        let project_root = self.project_root(project_id);
        chapter::save_chapter_verified_with_changes(
            &project_root,
            volume_id,
            chapter_id,
            content,
            &self.app_data_root,
        )
    }

    pub fn write_chapter_verified_with_allow_empty_overwrite(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        allow_empty_overwrite: bool,
    ) -> Result<ChapterSaveReceipt> {
        let project_root = self.project_root(project_id);
        chapter::save_chapter_verified_with_allow_empty_overwrite(
            &project_root,
            volume_id,
            chapter_id,
            content,
            allow_empty_overwrite,
        )
    }

    pub fn update_chapter_note(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        note: &str,
    ) -> Result<()> {
        let project_root = self.project_root(project_id);
        chapter::update_chapter_note(&project_root, volume_id, chapter_id, note)
    }

    /// 重命名只改 `project.json`，不再构造 layout_fn。
    pub fn rename_project(&self, project_id: &str, new_title: &str) -> crate::error::Result<()> {
        crate::project::rename_project(&self.projects_root, project_id, new_title)
    }

    //   `WriterCore::delete_project`（facade 层绕过
    // workspace history 的旧入口）已删除。写操作统一走
    // `with_app_service → WriterAppService → WriterCoreApi →
    // delete_project_with_changes → record_workspace_change_set → ack`。
    // 保留 `delete_project_with_changes` 供 API 层使用。

    /// 重排只改各 `project.json` 的 `order` 字段，不再构造 layout_fn。
    pub fn reorder_projects(&self, ordered_ids: &[String]) -> crate::error::Result<()> {
        crate::project::reorder_projects(&self.projects_root, ordered_ids)
    }

    //   *_with_changes 版本，返回 WorkspaceChangeSet 供 API 层记录本地历史。

    /// 创建作品并返回变更集。
    pub fn create_project_with_changes(
        &self,
        title: &str,
    ) -> Result<(Project, WorkspaceChangeSet)> {
        crate::project::create_project_with_changes(&self.projects_root, title)
    }

    /// 重命名作品并返回变更集。
    pub fn rename_project_with_changes(
        &self,
        project_id: &str,
        new_title: &str,
    ) -> Result<(Project, WorkspaceChangeSet)> {
        crate::project::rename_project_with_changes(&self.projects_root, project_id, new_title)
    }

    /// 删除作品并返回业务结果（变更集 + 解绑 starmap ids + journal token）。
    ///
    /// 返回 `ProjectDeleteOutcome`，
    /// journal 保留在 `StarMapsUnbound`，由 API 层记 history 后调
    /// `ack_project_delete_history` 推进并清 journal。
    pub fn delete_project_with_changes(
        &self,
        project_id: &str,
        device_id: &str,
    ) -> Result<crate::project::ProjectDeleteOutcome> {
        crate::project::delete_project_with_changes(
            &self.projects_root,
            project_id,
            &self.app_data_root,
            device_id,
            crate::project::ProjectDeleteOrigin::User,
        )
    }

    /// 重排作品并返回变更集。
    pub fn reorder_projects_with_changes(
        &self,
        ordered_ids: &[String],
    ) -> Result<WorkspaceChangeSet> {
        crate::project::reorder_projects_with_changes(&self.projects_root, ordered_ids)
    }

    /// 创建章节并返回变更集。
    pub fn create_chapter_with_changes(
        &self,
        project_id: &str,
        volume_id: &str,
        title: &str,
    ) -> Result<(Chapter, WorkspaceChangeSet)> {
        let project_root = self.project_root(project_id);
        chapter::create_chapter_with_changes(&project_root, volume_id, title, &self.app_data_root)
    }

    /// 保存章节正文并返回变更集。
    pub fn save_chapter_verified_with_changes(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> Result<(ChapterSaveReceipt, WorkspaceChangeSet)> {
        let project_root = self.project_root(project_id);
        chapter::save_chapter_verified_with_changes(
            &project_root,
            volume_id,
            chapter_id,
            content,
            &self.app_data_root,
        )
    }

    /// 保存章节正文并返回变更集（带空覆盖控制）。
    pub fn save_chapter_verified_with_changes_with_options(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        allow_empty_overwrite: bool,
    ) -> Result<(ChapterSaveReceipt, WorkspaceChangeSet)> {
        let project_root = self.project_root(project_id);
        chapter::save_chapter_verified_with_changes_with_options(
            &project_root,
            volume_id,
            chapter_id,
            content,
            allow_empty_overwrite,
            &self.app_data_root,
        )
    }

    /// Save chapter content and additional project-local records in the same
    /// transaction used for chapter.md and chapter metadata.
    pub fn save_chapter_verified_with_changes_and_files_with_options(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        allow_empty_overwrite: bool,
        additional_files: &[(String, String)],
    ) -> Result<(ChapterSaveReceipt, WorkspaceChangeSet)> {
        let project_root = self.project_root(project_id);
        chapter::save_chapter_verified_with_changes_and_files_with_options(
            &project_root,
            volume_id,
            chapter_id,
            content,
            allow_empty_overwrite,
            additional_files,
            &self.app_data_root,
        )
    }

    /// 保存章节正文并返回变更集（带 base 版本校验与附加文件）。
    #[allow(clippy::too_many_arguments)]
    pub fn save_chapter_verified_with_changes_versioned(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        allow_empty_overwrite: bool,
        additional_files: &[(String, String)],
        expected_version: Option<crate::chapter::ChapterVersion>,
        base_content: Option<&str>,
        projection_ids: &[String],
        workspace_lock: Option<&crate::storage::WorkspaceWriteLock>,
    ) -> Result<(ChapterSaveReceipt, WorkspaceChangeSet)> {
        let project_root = self.project_root(project_id);
        let proof_events = self.pending_editor_proof_for(project_id, volume_id, chapter_id);
        let mut projection_ids = projection_ids.to_vec();
        for event in &proof_events {
            let event_id = event.event_id().to_string();
            if !event_id.is_empty() && !projection_ids.contains(&event_id) {
                projection_ids.push(event_id);
            }
        }
        let result = chapter::save_chapter_verified_with_changes_and_files_versioned_with_proof(
            &project_root,
            volume_id,
            chapter_id,
            content,
            allow_empty_overwrite,
            additional_files,
            expected_version,
            base_content,
            &projection_ids,
            &self.app_data_root,
            workspace_lock,
            &proof_events,
        )?;
        if !proof_events.is_empty() {
            let completed: std::collections::HashSet<String> = proof_events
                .iter()
                .map(|event| event.event_id().to_string())
                .collect();
            self.clear_pending_editor_proof(project_id, volume_id, chapter_id, &completed);
            self.apply_pending_chapter_proof(&project_root, &result.0.transaction_id);
        }
        Ok(result)
    }

    /// 清空章节正文并核对 base 版本。
    pub fn clear_chapter_content_verified_with_version(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        expected_version: Option<crate::chapter::ChapterVersion>,
        base_content: Option<&str>,
        workspace_lock: Option<&crate::storage::WorkspaceWriteLock>,
    ) -> Result<ChapterSaveReceipt> {
        let project_root = self.project_root(project_id);
        chapter::clear_chapter_content_verified_with_version(
            &project_root,
            workspace_lock,
            volume_id,
            chapter_id,
            expected_version,
            base_content,
        )
    }

    /// 重命名章节并返回变更集。
    pub fn rename_chapter_with_changes(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        new_title: &str,
    ) -> Result<(Chapter, WorkspaceChangeSet)> {
        let project_root = self.project_root(project_id);
        chapter::rename_chapter_with_changes(
            &project_root,
            volume_id,
            chapter_id,
            new_title,
            &self.app_data_root,
        )
    }

    /// 删除章节并返回变更集。
    pub fn delete_chapter_with_changes(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<WorkspaceChangeSet> {
        let project_root = self.project_root(project_id);
        chapter::delete_chapter_with_changes(
            &project_root,
            volume_id,
            chapter_id,
            &self.app_data_root,
            &self.app_data_root,
        )
    }

    /// 删除章节的"先计划"版本（不修改磁盘），供 durable 删除事务使用。
    ///
    /// 只校验目标并构造 `WorkspaceChangeSet`，不执行本地删除。
    /// 调用方在 `save_pending` 落盘 journal 后再调 [`Self::delete_chapter`] 执行本地删除。
    pub fn plan_delete_chapter_changes(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<WorkspaceChangeSet> {
        let project_root = self.project_root(project_id);
        chapter::plan_delete_chapter_changes(
            &project_root,
            volume_id,
            chapter_id,
            &self.app_data_root,
            &self.app_data_root,
        )
    }

    /// 构造完整的章节删除计划（不修改磁盘），供 durable 删除事务 `save_pending` 使用。
    ///
    /// 返回 `(WorkspaceChangeSet, PlannedWorkspaceDelete)`。
    /// `device_id` 用真实设备 ID。
    pub fn plan_delete_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        device_id: &str,
    ) -> Result<(
        WorkspaceChangeSet,
        crate::storage::journal::workspace_change::PlannedWorkspaceDelete,
    )> {
        let project_root = self.project_root(project_id);
        chapter::plan_delete_chapter(
            &project_root,
            volume_id,
            chapter_id,
            &self.app_data_root,
            &self.app_data_root,
            device_id,
        )
    }

    /// 消费 `PlannedWorkspaceDelete` 执行章节删除。
    pub fn apply_planned_delete_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        planned: &crate::storage::journal::workspace_change::PlannedWorkspaceDelete,
    ) -> Result<()> {
        let project_root = self.project_root(project_id);
        chapter::apply_planned_delete_chapter(
            &project_root,
            volume_id,
            chapter_id,
            &self.app_data_root,
            planned,
        )
    }

    /// 重排章节并返回变更集。
    pub fn reorder_chapters_with_changes(
        &self,
        project_id: &str,
        volume_id: &str,
        ordered_ids: &[String],
    ) -> Result<WorkspaceChangeSet> {
        let project_root = self.project_root(project_id);
        chapter::reorder_chapters_with_changes(
            &project_root,
            volume_id,
            ordered_ids,
            &self.app_data_root,
        )
    }

    /// 跨卷移动章节并返回变更集与 journal token。
    ///
    /// journal 已在章节层写入 `app_data_root`（workspace root）的
    /// workspace-change journal；API 层必须用返回的 token 在 history 成功后
    /// 推进 `HistoryRecorded` 并清理，不能在文件事务成功后就丢弃 token。
    pub fn move_chapter_to_volume_with_changes(
        &self,
        project_id: &str,
        source_volume_id: &str,
        target_volume_id: &str,
        chapter_id: &str,
        target_index: usize,
    ) -> Result<(WorkspaceChangeSet, String)> {
        let project_root = self.project_root(project_id);
        chapter::move_chapter_to_volume_with_changes(
            &project_root,
            source_volume_id,
            target_volume_id,
            chapter_id,
            target_index,
            &self.app_data_root,
        )
    }

    /// 更新章节备注并返回变更集。
    pub fn update_chapter_note_with_changes(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        note: &str,
    ) -> Result<(Chapter, WorkspaceChangeSet)> {
        let project_root = self.project_root(project_id);
        chapter::update_chapter_note_with_changes(
            &project_root,
            volume_id,
            chapter_id,
            note,
            &self.app_data_root,
        )
    }

    pub fn rename_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        new_title: &str,
    ) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::volume::rename_volume(&project_root, volume_id, new_title)
    }

    pub fn delete_volume(&self, project_id: &str, volume_id: &str) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::volume::delete_volume(&project_root, volume_id, &self.app_data_root)
    }

    pub fn reorder_volumes(
        &self,
        project_id: &str,
        ordered_ids: &[String],
    ) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::volume::reorder_volumes(&project_root, ordered_ids)
    }

    //   volume 的 *_with_changes 转发，
    // 返回 WorkspaceChangeSet 供 API 层记录本地历史。

    /// 创建卷并返回变更集。
    pub fn create_volume_with_changes(
        &self,
        project_id: &str,
        title: &str,
    ) -> Result<(Volume, WorkspaceChangeSet)> {
        let project_root = self.project_root(project_id);
        crate::volume::create_volume_with_changes(&project_root, title, &self.app_data_root)
    }

    /// 重命名卷并返回变更集。
    pub fn rename_volume_with_changes(
        &self,
        project_id: &str,
        volume_id: &str,
        new_title: &str,
    ) -> Result<WorkspaceChangeSet> {
        let project_root = self.project_root(project_id);
        crate::volume::rename_volume_with_changes(
            &project_root,
            volume_id,
            new_title,
            &self.app_data_root,
        )
    }

    /// 删除卷并返回变更集。
    pub fn delete_volume_with_changes(
        &self,
        project_id: &str,
        volume_id: &str,
    ) -> Result<WorkspaceChangeSet> {
        let project_root = self.project_root(project_id);
        crate::volume::delete_volume_with_changes(&project_root, volume_id, &self.app_data_root)
    }

    /// 删除卷的"先计划"版本（不修改磁盘），供 durable 删除事务使用。
    ///
    /// 只校验目标并构造 `WorkspaceChangeSet`，不执行物理删除。
    /// 调用方在 `save_pending` 落盘 journal 后再调 [`Self::delete_volume`] 执行物理删除。
    pub fn plan_delete_volume_changes(
        &self,
        project_id: &str,
        volume_id: &str,
    ) -> Result<WorkspaceChangeSet> {
        let project_root = self.project_root(project_id);
        crate::volume::plan_delete_volume_changes(&project_root, volume_id, &self.app_data_root)
    }

    /// 构造完整的卷删除计划（不修改磁盘），供 durable 删除事务 `save_pending` 使用。
    ///
    /// 返回 `(WorkspaceChangeSet, PlannedWorkspaceDelete)`。
    /// `device_id` 用真实设备 ID。
    pub fn plan_delete_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        device_id: &str,
    ) -> Result<(
        WorkspaceChangeSet,
        crate::storage::journal::workspace_change::PlannedWorkspaceDelete,
    )> {
        let project_root = self.project_root(project_id);
        crate::volume::plan_delete_volume(&project_root, volume_id, &self.app_data_root, device_id)
    }

    /// 消费 `PlannedWorkspaceDelete` 执行卷删除。
    pub fn apply_planned_delete_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        planned: &crate::storage::journal::workspace_change::PlannedWorkspaceDelete,
    ) -> Result<()> {
        let project_root = self.project_root(project_id);
        crate::volume::apply_planned_delete_volume(
            &project_root,
            volume_id,
            &self.app_data_root,
            planned,
        )
    }

    /// 重排卷并返回变更集。
    pub fn reorder_volumes_with_changes(
        &self,
        project_id: &str,
        ordered_ids: &[String],
    ) -> Result<WorkspaceChangeSet> {
        let project_root = self.project_root(project_id);
        crate::volume::reorder_volumes_with_changes(&project_root, ordered_ids, &self.app_data_root)
    }

    pub fn rename_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        new_title: &str,
    ) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::chapter::rename_chapter(&project_root, volume_id, chapter_id, new_title)
    }

    pub fn delete_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::chapter::delete_chapter(&project_root, volume_id, chapter_id, &self.app_data_root)
    }

    pub fn reorder_chapters(
        &self,
        project_id: &str,
        volume_id: &str,
        ordered_ids: &[String],
    ) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::chapter::reorder_chapters(&project_root, volume_id, ordered_ids)
    }

    /// 从子章节聚合获取 volume 的最近更新时间。
    pub fn get_volume_updated_at_aggregated(
        &self,
        project_id: &str,
        volume_id: &str,
    ) -> crate::error::Result<String> {
        let project_root = self.project_root(project_id);
        crate::project::get_volume_updated_at_aggregated(&project_root, volume_id)
    }

    /// 从子章节聚合获取 project 的最近更新时间。
    pub fn get_project_updated_at_aggregated(
        &self,
        project_id: &str,
    ) -> crate::error::Result<String> {
        let project_root = self.project_root(project_id);
        crate::project::get_project_updated_at_aggregated(&project_root)
    }
}
