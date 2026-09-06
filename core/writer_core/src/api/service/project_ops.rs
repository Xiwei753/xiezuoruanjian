use super::*;

impl WriterCoreApi {
    pub fn list_projects(&self) -> ApiResult<Vec<ProjectDto>> {
        self.core_read()
            .list_projects()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    /// #625 第二段：批量返回项目摘要（元数据 + 统计）。
    pub fn list_project_summaries(&self) -> ApiResult<Vec<ProjectSummaryDto>> {
        self.core_read()
            .list_project_summaries()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn create_project(&self, title: &str) -> ApiResult<ProjectDto> {
        // #645 评论 5504296097 问题2：用 _with_changes 版本拿变更集，
        // 调 record_workspace_change_set_history 记录本地历史。
        let (project, change_set) = self
            .core_write()
            .create_project_with_changes(title)
            .map_err(WriterError::from)?;
        let entry = crate::search::extractor::extract_project_title_entry(&project.id, title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let volumes_result = self.core_write().list_volumes(&project.id);
        if let Ok(volumes) = volumes_result {
            if let Some(default_vol) = volumes.first() {
                let vol_entry = crate::search::extractor::extract_volume_title_entry(
                    &project.id,
                    &default_vol.id,
                    &default_vol.title,
                );
                self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                    action: crate::search::SearchIndexAction::Upsert,
                    object_id: vol_entry.object_id.clone(),
                    scope: vol_entry.scope,
                    title: vol_entry.title.clone(),
                    body: vol_entry.body.clone(),
                    target: Some(vol_entry.target.clone()),
                });
            }
        }
        let _ = self.record_workspace_change_set_history(&change_set, "create_project");
        Ok(project.into())
    }

    /// #649 评论 5561286861 第 4 点：恢复/导入项目入口——使用 manifest 中的稳定 ID。
    ///
    /// 不自动创建"第一卷"（卷信息在 manifest 中已包含，由调用方逐卷恢复）。
    /// 不记录 workspace history（恢复场景下 manifest 是已有事实来源）。
    pub fn create_project_with_id(
        &self,
        id: &str,
        title: &str,
        order: i32,
    ) -> ApiResult<ProjectDto> {
        let project = self
            .core_write()
            .create_project_with_id(id, title, order)
            .map_err(WriterError::from)?;
        // 恢复场景：不记录 workspace history（manifest 是已有事实来源）
        // 不自动创建默认卷（卷信息在 manifest 中）
        Ok(project.into())
    }

    pub fn get_project_stats(&self, project_id: &str) -> ApiResult<ProjectStatsDto> {
        self.core_read()
            .get_project_stats(project_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    /// #644 评论 5467821839 第7节：一次返回作品的全部卷 + 章节 + 统计。
    ///
    /// Android `ProjectViewModel` 不再逐卷调 `list_chapters`，
    /// 而是一次拿到完整快照，减少 FFI 调用次数和中间状态不一致窗口。
    pub fn get_project_workspace_snapshot(
        &self,
        project_id: &str,
    ) -> ApiResult<ProjectWorkspaceSnapshotDto> {
        let core = self.core_read();
        let project: ProjectDto = core
            .list_projects()
            .map_err(WriterError::from)?
            .into_iter()
            .find(|p| p.id == project_id)
            .map(Into::into)
            .ok_or(WriterError::ProjectNotFound)?;

        let stats: ProjectStatsDto = core
            .get_project_stats(project_id)
            .map(Into::into)
            .map_err(WriterError::from)?;

        let volumes = core.list_volumes(project_id).map_err(WriterError::from)?;

        let mut volume_snapshots = Vec::with_capacity(volumes.len());
        for vol in volumes {
            let chapters = core
                .list_chapters(project_id, &vol.id)
                .map_err(WriterError::from)?
                .into_iter()
                .map(Into::into)
                .collect();
            volume_snapshots.push(VolumeWithChaptersDto {
                volume: vol.into(),
                chapters,
            });
        }

        Ok(ProjectWorkspaceSnapshotDto {
            project,
            stats,
            volumes: volume_snapshots,
        })
    }

    pub fn rename_project(&self, project_id: &str, new_title: &str) -> ApiResult<bool> {
        // #645 评论 5504296097 问题2：用 _with_changes 版本拿变更集。
        let (_project, change_set) = self
            .core_write()
            .rename_project_with_changes(project_id, new_title)?;
        let entry = crate::search::extractor::extract_project_title_entry(project_id, new_title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let _ = self.record_workspace_change_set_history(&change_set, "rename_project");
        Ok(true)
    }

    pub fn delete_project(&self, project_id: &str) -> ApiResult<bool> {
        // #645 评论 5504296097 缺口1/缺口2修复：
        // - core 层不再吞 list_starmaps_bound_to_project 错误，index.json 损坏时删除返回 Err。
        // - 不再二次枚举绑定 starmap——用 outcome.unbound_starmap_ids（journal 里记录的
        //   唯一事实来源）刷搜索索引。
        // - 不再在 core 内 complete/cleanup journal——记 history 成功后调
        //   ack_project_delete_history 推进到 HistoryRecorded → Completed 并清 journal。
        //   history 失败时 journal 保留在 StarMapsUnbound，下次启动 recover 补记。

        // #645 评论 5504296097 问题3：读取 device_id 传给 delete_project_with_changes，
        // 写入 journal 供 ack/recover 构造 PendingDeletedTarget（LWW tie-break）。
        let device_id = crate::settings::load_device_info(&self.app_data_root)
            .map(|i| i.device_id)
            .unwrap_or_default();

        let outcome = self
            .core_write()
            .delete_project_with_changes(project_id, &device_id)?;

        // 删除成功后才清搜索索引。搜索索引清理放在删除成功之后，避免
        // project 没删掉但搜索索引已清空的不一致状态。
        for prefix in &[
            format!("project:{}", project_id),
            format!("volume:{}:", project_id),
            format!("chapter_title:{}:", project_id),
            format!("chapter_body:{}:", project_id),
            format!("chapter_note:{}:", project_id),
        ] {
            self.remove_search_index_by_prefix(prefix);
        }

        // #645 评论 5504296097 缺口1修复：用 outcome.unbound_starmap_ids 刷搜索索引，
        // 不再二次枚举 list_starmaps_bound_to_project（避免与 core 层结果不一致）。
        for sm_id in &outcome.unbound_starmap_ids {
            self.refresh_starmap_search_index(sm_id);
        }

        // #645 评论 5504296097 缺口2修复：用 outcome.changes 记本地 history，
        // 成功后 ack 推进 journal。history 失败时 log::warn 并保留 journal，
        // 下次启动 recover 补记——不让 history 失败把删除变成失败（项目已删）。
        let layout_guard = match self.workspace_git_layout.read() {
            Ok(g) => g,
            Err(_) => {
                log::warn!(
                    "delete_project: layout lock poisoned, skipping history + ack; \
                     journal retained for recovery"
                );
                return Ok(true);
            }
        };
        match crate::storage::workspace_git::record_workspace_change_set(
            &layout_guard,
            &outcome.changes,
            "delete_project",
        ) {
            Ok(result) => {
                if result.oid.is_some() {
                    log::debug!(
                        "delete_project: history committed ({} staged)",
                        result.staged_count
                    );
                }
                // history 成功，ack 推进 journal 到 HistoryRecorded → Completed 并清 journal。
                if let Err(e) = crate::storage::journal::project_delete::ack_project_delete_history(
                    &self.app_data_root,
                    &outcome.journal_token,
                ) {
                    log::warn!(
                        "delete_project: ack_project_delete_history failed: {} — \
                         journal retained for recovery",
                        e
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "delete_project: record_workspace_change_set failed: {} — \
                     journal retained for recovery, history will be补 on next startup",
                    e
                );
            }
        }

        Ok(true)
    }

    pub fn reorder_projects(&self, ordered_project_ids: &[String]) -> ApiResult<bool> {
        // #645 评论 5504296097 问题2：用 _with_changes 版本拿变更集。
        let change_set = self
            .core_write()
            .reorder_projects_with_changes(ordered_project_ids)?;
        let _ = self.record_workspace_change_set_history(&change_set, "reorder_projects");
        Ok(true)
    }

    pub fn list_volumes(&self, project_id: &str) -> ApiResult<Vec<VolumeDto>> {
        self.core_read()
            .list_volumes(project_id)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn create_volume(&self, project_id: &str, title: &str) -> ApiResult<VolumeDto> {
        // #645 评论 5504296097 问题3：用 _with_changes 版本拿变更集，
        // 调 record_workspace_change_set_history 记录本地历史。
        let (volume, change_set) = self
            .core_write()
            .create_volume_with_changes(project_id, title)
            .map_err(WriterError::from)?;
        let volume: VolumeDto = volume.into();
        let entry =
            crate::search::extractor::extract_volume_title_entry(project_id, &volume.id, title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let _ = self.record_workspace_change_set_history(&change_set, "create_volume");
        Ok(volume)
    }

    /// #649 评论 5561286861 第 4 点：恢复/导入卷——使用 manifest 中的稳定 ID。
    ///
    /// 恢复场景：不记录 workspace history（manifest 是已有事实来源）。
    pub fn create_volume_with_id(
        &self,
        project_id: &str,
        id: &str,
        title: &str,
        order: i32,
    ) -> ApiResult<VolumeDto> {
        let volume = self
            .core_write()
            .create_volume_with_id(project_id, id, title, order)
            .map_err(WriterError::from)?;
        Ok(volume.into())
    }

    pub fn rename_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        new_title: &str,
    ) -> ApiResult<bool> {
        // #645 评论 5504296097 问题3：用 _with_changes 版本拿变更集。
        let change_set = self
            .core_write()
            .rename_volume_with_changes(project_id, volume_id, new_title)?;
        let entry =
            crate::search::extractor::extract_volume_title_entry(project_id, volume_id, new_title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let _ = self.record_workspace_change_set_history(&change_set, "rename_volume");
        Ok(true)
    }

    pub fn delete_volume(&self, project_id: &str, volume_id: &str) -> ApiResult<bool> {
        // #645 评论 5504296097 问题3：用 _with_changes 版本拿变更集。
        // change_set 由底层 delete_volume_with_changes 返回，包含
        // DeleteTree(projects/{pid}/volumes/{vid})，不再手拼路径。
        let change_set = self
            .core_write()
            .delete_volume_with_changes(project_id, volume_id)?;
        for prefix in &[
            format!("volume:{}:{}", project_id, volume_id),
            format!("chapter_title:{}:{}:", project_id, volume_id),
            format!("chapter_body:{}:{}:", project_id, volume_id),
            format!("chapter_note:{}:{}:", project_id, volume_id),
        ] {
            self.remove_search_index_by_prefix(prefix);
        }
        let _ = self.record_workspace_change_set_history(&change_set, "delete_volume");
        Ok(true)
    }

    pub fn reorder_volumes(
        &self,
        project_id: &str,
        ordered_volume_ids: &[String],
    ) -> ApiResult<bool> {
        // #645 评论 5504296097 问题3：用 _with_changes 版本拿变更集。
        let change_set = self
            .core_write()
            .reorder_volumes_with_changes(project_id, ordered_volume_ids)?;
        let _ = self.record_workspace_change_set_history(&change_set, "reorder_volumes");
        Ok(true)
    }

    /// #649 评论 5561465552 第 2 点：恢复作品树——一次跨 FFI 传入完整作品树。
    ///
    /// Core 负责：
    /// 1. 校验 project/volume/chapter ID 非空、格式合法（UUID）；
    /// 2. 校验目标 ID 不冲突（project_id 不已存在、同 project 内 volume_id 唯一、
    ///    同 volume 内 chapter_id 唯一）；
    /// 3. 逐层创建项目/卷/章节、保存正文、按 input 顺序 reorder；
    /// 4. 任一步失败时回滚已创建的项目（delete_project），返回 Err，不留半成品；
    /// 5. 全部成功后把这次恢复作为一次 workspace Git 变更记录下来；
    /// 6. 返回创建的 ProjectDto。
    pub fn restore_project_tree(&self, input: &RestoreProjectInputDto) -> ApiResult<ProjectDto> {
        // 1. 校验输入（ID 格式、唯一性、project_id 不已存在）
        self.validate_restore_input(input)?;

        // 2. 创建项目树，收集变更集
        let change_set = match self.create_restore_tree(input) {
            Ok(cs) => cs,
            Err(e) => return Err(self.rollback_restore(input, e)),
        };

        // 3. 全部成功后记录 workspace Git 变更
        let _ = self.record_workspace_change_set_history(&change_set, "restore_project_tree");

        // 4. 返回创建的 ProjectDto
        let project_dto: ProjectDto = self
            .core_read()
            .list_projects()
            .map_err(WriterError::from)?
            .into_iter()
            .find(|p| p.id == input.project_id)
            .map(Into::into)
            .ok_or(WriterError::ProjectNotFound)?;

        Ok(project_dto)
    }

    /// 校验恢复输入：ID 非空、UUID 格式、唯一性、project_id 不已存在。
    fn validate_restore_input(&self, input: &RestoreProjectInputDto) -> ApiResult<()> {
        use std::collections::HashSet;

        Self::validate_id_non_empty(&input.project_id, "project_id")?;
        Self::validate_uuid_format(&input.project_id, "project_id")?;

        // 校验 project_id 不已存在
        let existing_projects = self
            .core_read()
            .list_projects()
            .map_err(WriterError::from)?;
        if existing_projects.iter().any(|p| p.id == input.project_id) {
            return Err(WriterError::Other(format!(
                "restore_project_tree: project_id already exists: {}",
                input.project_id
            )));
        }

        // 校验 volume_id 唯一、非空、UUID 格式；chapter_id 同理
        let mut seen_volume_ids: HashSet<&str> = HashSet::new();
        for vol in &input.volumes {
            Self::validate_id_non_empty(&vol.volume_id, "volume_id")?;
            Self::validate_uuid_format(&vol.volume_id, "volume_id")?;
            if !seen_volume_ids.insert(vol.volume_id.as_str()) {
                return Err(WriterError::Other(format!(
                    "restore_project_tree: duplicate volume_id in input: {}",
                    vol.volume_id
                )));
            }

            Self::validate_volume_chapters(vol)?;
        }
        Ok(())
    }

    /// 校验单个卷内 chapter_id 唯一、非空、UUID 格式。
    fn validate_volume_chapters(vol: &RestoreVolumeInputDto) -> ApiResult<()> {
        use std::collections::HashSet;

        let mut seen_chapter_ids: HashSet<&str> = HashSet::new();
        for ch in &vol.chapters {
            Self::validate_id_non_empty(&ch.chapter_id, "chapter_id")?;
            Self::validate_uuid_format(&ch.chapter_id, "chapter_id")?;
            if !seen_chapter_ids.insert(ch.chapter_id.as_str()) {
                return Err(WriterError::Other(format!(
                    "restore_project_tree: duplicate chapter_id in volume {}: {}",
                    vol.volume_id, ch.chapter_id
                )));
            }
        }
        Ok(())
    }

    /// 校验 ID 非空。
    fn validate_id_non_empty(id: &str, name: &str) -> ApiResult<()> {
        if id.trim().is_empty() {
            return Err(WriterError::Other(format!(
                "restore_project_tree: {} must not be empty",
                name
            )));
        }
        Ok(())
    }

    /// 校验 UUID 格式。
    fn validate_uuid_format(id: &str, name: &str) -> ApiResult<()> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(WriterError::Other(format!(
                "restore_project_tree: invalid {} format: {}",
                name, id
            )));
        }
        Ok(())
    }

    /// 创建恢复项目树：项目 → 卷 → 章节 → 正文 → reorder，返回变更集。
    /// 调用方负责在失败时回滚已创建的项目。
    fn create_restore_tree(
        &self,
        input: &RestoreProjectInputDto,
    ) -> crate::error::Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        use crate::storage::workspace_git::WorkspaceChangeSet;

        // 创建项目
        self.core_write()
            .create_project_with_id(&input.project_id, &input.title, input.order)?;

        let mut change_set = WorkspaceChangeSet::new().add_upsert(
            std::path::PathBuf::from("projects")
                .join(&input.project_id)
                .join("project.json"),
        );

        // 逐卷创建
        for vol in &input.volumes {
            change_set = self.create_restore_volume(input, vol, change_set)?;
        }

        // 按 input 顺序 reorder volumes（确保 order 连续 0,1,2,...）
        if !input.volumes.is_empty() {
            let ordered_volume_ids: Vec<String> =
                input.volumes.iter().map(|v| v.volume_id.clone()).collect();
            let vol_change_set = self
                .core_write()
                .reorder_volumes_with_changes(&input.project_id, &ordered_volume_ids)?;
            change_set = change_set.merge(vol_change_set);
        }

        // 按 input 顺序 reorder 每个卷的 chapters
        for vol in &input.volumes {
            change_set = self.reorder_restore_chapters(input, vol, change_set)?;
        }

        Ok(change_set)
    }

    /// 创建单个恢复卷及其章节，返回更新后的变更集。
    fn create_restore_volume(
        &self,
        input: &RestoreProjectInputDto,
        vol: &RestoreVolumeInputDto,
        mut change_set: crate::storage::workspace_git::WorkspaceChangeSet,
    ) -> crate::error::Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        let volume = self.core_write().create_volume_with_id(
            &input.project_id,
            &vol.volume_id,
            &vol.title,
            vol.order,
        )?;

        change_set = change_set.add_upsert(
            std::path::PathBuf::from("projects")
                .join(&input.project_id)
                .join("volumes")
                .join(&volume.id)
                .join("volume.json"),
        );

        // 逐章创建 + 保存正文
        for ch in &vol.chapters {
            change_set = self.create_restore_chapter(input, vol, ch, change_set)?;
        }

        Ok(change_set)
    }

    /// 创建单个恢复章节并保存正文，返回更新后的变更集。
    fn create_restore_chapter(
        &self,
        input: &RestoreProjectInputDto,
        vol: &RestoreVolumeInputDto,
        ch: &RestoreChapterInputDto,
        mut change_set: crate::storage::workspace_git::WorkspaceChangeSet,
    ) -> crate::error::Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        let chapter = self.core_write().create_chapter_with_id(
            &input.project_id,
            &vol.volume_id,
            &ch.chapter_id,
            &ch.title,
            ch.order,
        )?;

        change_set = change_set.add_upsert(
            std::path::PathBuf::from("projects")
                .join(&input.project_id)
                .join("volumes")
                .join(&vol.volume_id)
                .join("chapters")
                .join(&chapter.id)
                .join("chapter.meta.json"),
        );

        // 保存正文：content 非空时才写入（create_chapter_with_id 已创建空 chapter.md）
        if !ch.content.is_empty() {
            let (_receipt, ch_change_set) = self
                .core_write()
                .save_chapter_verified_with_changes_with_options(
                    &input.project_id,
                    &vol.volume_id,
                    &ch.chapter_id,
                    &ch.content,
                    true,
                )?;
            change_set = change_set.merge(ch_change_set);
        }

        Ok(change_set)
    }

    /// reorder 单个卷的章节，返回更新后的变更集。
    fn reorder_restore_chapters(
        &self,
        input: &RestoreProjectInputDto,
        vol: &RestoreVolumeInputDto,
        mut change_set: crate::storage::workspace_git::WorkspaceChangeSet,
    ) -> crate::error::Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        if vol.chapters.is_empty() {
            return Ok(change_set);
        }
        let ordered_chapter_ids: Vec<String> =
            vol.chapters.iter().map(|c| c.chapter_id.clone()).collect();
        let ch_change_set = self.core_write().reorder_chapters_with_changes(
            &input.project_id,
            &vol.volume_id,
            &ordered_chapter_ids,
        )?;
        change_set = change_set.merge(ch_change_set);
        Ok(change_set)
    }

    /// 回滚恢复操作：删除已创建的项目，记录警告。
    /// 返回原始错误，让调用方返回给 FFI 调用方。
    fn rollback_restore(
        &self,
        input: &RestoreProjectInputDto,
        err: crate::error::Error,
    ) -> WriterError {
        log::warn!(
            "restore_project_tree: rolling back project {} due to error: {}",
            input.project_id,
            err
        );
        if let Err(del_err) = self.delete_project(&input.project_id) {
            log::warn!(
                "restore_project_tree: rollback delete_project failed: {} — \
                 partial state may remain",
                del_err
            );
        }
        WriterError::from(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{
        ChapterMetaDto, RestoreChapterInputDto, RestoreProjectInputDto, RestoreVolumeInputDto,
    };
    use tempfile::tempdir;
    use uuid::Uuid;

    /// 构造一个合法的 UUID 字符串。
    fn new_uuid() -> String {
        Uuid::new_v4().to_string()
    }

    /// 构造一个完整的 RestoreProjectInputDto，包含 2 卷，每卷 2 章节，正文非空。
    fn make_full_input() -> RestoreProjectInputDto {
        let project_id = new_uuid();
        let vol1_id = new_uuid();
        let vol2_id = new_uuid();
        let ch1_id = new_uuid();
        let ch2_id = new_uuid();
        let ch3_id = new_uuid();
        let ch4_id = new_uuid();

        RestoreProjectInputDto {
            project_id,
            title: "恢复测试作品".to_string(),
            order: 0,
            volumes: vec![
                RestoreVolumeInputDto {
                    volume_id: vol1_id,
                    title: "第一卷".to_string(),
                    order: 0,
                    chapters: vec![
                        RestoreChapterInputDto {
                            chapter_id: ch1_id,
                            title: "第一章".to_string(),
                            order: 0,
                            content: "第一章正文内容。".to_string(),
                        },
                        RestoreChapterInputDto {
                            chapter_id: ch2_id,
                            title: "第二章".to_string(),
                            order: 1,
                            content: "第二章正文内容。".to_string(),
                        },
                    ],
                },
                RestoreVolumeInputDto {
                    volume_id: vol2_id,
                    title: "第二卷".to_string(),
                    order: 1,
                    chapters: vec![
                        RestoreChapterInputDto {
                            chapter_id: ch3_id,
                            title: "第三章".to_string(),
                            order: 0,
                            content: "第三章正文内容。".to_string(),
                        },
                        RestoreChapterInputDto {
                            chapter_id: ch4_id,
                            title: "第四章".to_string(),
                            order: 1,
                            content: "".to_string(),
                        },
                    ],
                },
            ],
        }
    }

    /// 创建测试用 WriterCoreApi 实例。
    fn make_api() -> (tempfile::TempDir, WriterCoreApi) {
        let temp_dir = tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
        let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));
        (temp_dir, api)
    }

    #[test]
    fn restore_project_tree_success_creates_full_tree() {
        let (_dir, api) = make_api();
        let input = make_full_input();

        let expected_project_id = input.project_id.clone();
        let expected_vol_ids: Vec<String> =
            input.volumes.iter().map(|v| v.volume_id.clone()).collect();
        let expected_chapters: Vec<(String, String, String)> = input
            .volumes
            .iter()
            .flat_map(|v| {
                v.chapters
                    .iter()
                    .map(|c| (v.volume_id.clone(), c.chapter_id.clone(), c.content.clone()))
            })
            .collect();

        let result = api.restore_project_tree(&input).unwrap();
        assert_eq!(result.id, expected_project_id);
        assert_eq!(result.title, "恢复测试作品");

        // 验证卷和章节都按指定 ID 创建
        let volumes = api.list_volumes(&expected_project_id).unwrap();
        assert_eq!(volumes.len(), 2);
        let actual_vol_ids: Vec<String> = volumes.iter().map(|v| v.id.clone()).collect();
        for expected_id in &expected_vol_ids {
            assert!(
                actual_vol_ids.contains(expected_id),
                "volume {} should exist",
                expected_id
            );
        }
        // 验证卷 order 连续（0, 1）
        let mut sorted_vols = volumes.clone();
        sorted_vols.sort_by_key(|v| v.order);
        assert_eq!(sorted_vols[0].order, 0);
        assert_eq!(sorted_vols[1].order, 1);

        // 验证章节和正文
        for (vol_id, ch_id, expected_content) in &expected_chapters {
            let chapters = api.list_chapters(&expected_project_id, vol_id).unwrap();
            let chapter: &ChapterMetaDto = chapters
                .iter()
                .find(|c| &c.id == ch_id)
                .unwrap_or_else(|| panic!("chapter {} should exist", ch_id));

            let opened = api
                .open_chapter(&expected_project_id, vol_id, ch_id)
                .unwrap();
            assert_eq!(opened.meta.id, chapter.id);
            assert_eq!(opened.content, *expected_content);
        }
    }

    #[test]
    fn restore_project_tree_project_id_conflict_returns_err_and_no_partial() {
        let (_dir, api) = make_api();

        // 先创建一个项目
        let existing = api.create_project("已有作品").unwrap();

        // 尝试用相同 project_id 恢复
        let mut input = make_full_input();
        input.project_id = existing.id.clone();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(matches!(err, WriterError::Other(ref msg) if msg.contains("already exists")));

        // 验证原有项目仍然完好（没有被破坏）
        let projects = api.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, existing.id);

        // 验证原有项目的卷仍然存在（create_project 会创建默认卷）
        let volumes = api.list_volumes(&existing.id).unwrap();
        assert!(!volumes.is_empty());
    }

    #[test]
    fn restore_project_tree_duplicate_volume_id_returns_err_and_rolls_back() {
        let (_dir, api) = make_api();

        let dup_id = new_uuid();
        let mut input = make_full_input();
        // 让两个卷用同一个 volume_id
        input.volumes[0].volume_id = dup_id.clone();
        input.volumes[1].volume_id = dup_id.clone();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(matches!(err, WriterError::Other(ref msg) if msg.contains("duplicate volume_id")));

        // 验证没有留下半成品：项目不应存在
        let projects = api.list_projects().unwrap();
        assert!(
            projects.is_empty(),
            "no partial project should remain after rollback"
        );
    }

    #[test]
    fn restore_project_tree_duplicate_chapter_id_returns_err_and_rolls_back() {
        let (_dir, api) = make_api();

        let dup_id = new_uuid();
        let mut input = make_full_input();
        // 让同一卷下两个章节用同一个 chapter_id
        input.volumes[0].chapters[0].chapter_id = dup_id.clone();
        input.volumes[0].chapters[1].chapter_id = dup_id.clone();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(matches!(err, WriterError::Other(ref msg) if msg.contains("duplicate chapter_id")));

        // 验证没有留下半成品
        let projects = api.list_projects().unwrap();
        assert!(
            projects.is_empty(),
            "no partial project should remain after rollback"
        );
    }

    #[test]
    fn restore_project_tree_empty_volumes_creates_project_only() {
        let (_dir, api) = make_api();

        let project_id = new_uuid();
        let input = RestoreProjectInputDto {
            project_id: project_id.clone(),
            title: "空作品".to_string(),
            order: 0,
            volumes: vec![],
        };

        let result = api.restore_project_tree(&input).unwrap();
        assert_eq!(result.id, project_id);
        assert_eq!(result.title, "空作品");

        // 验证项目存在但无卷
        let volumes = api.list_volumes(&project_id).unwrap();
        assert!(volumes.is_empty(), "no volumes should exist");
    }

    #[test]
    fn restore_project_tree_volume_with_empty_chapters() {
        let (_dir, api) = make_api();

        let project_id = new_uuid();
        let vol_id = new_uuid();
        let input = RestoreProjectInputDto {
            project_id: project_id.clone(),
            title: "空卷作品".to_string(),
            order: 0,
            volumes: vec![RestoreVolumeInputDto {
                volume_id: vol_id.clone(),
                title: "空卷".to_string(),
                order: 0,
                chapters: vec![],
            }],
        };

        let result = api.restore_project_tree(&input).unwrap();
        assert_eq!(result.id, project_id);

        // 验证卷存在但无章节
        let volumes = api.list_volumes(&project_id).unwrap();
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].id, vol_id);

        let chapters = api.list_chapters(&project_id, &vol_id).unwrap();
        assert!(chapters.is_empty(), "no chapters should exist");
    }

    #[test]
    fn restore_project_tree_empty_project_id_returns_err() {
        let (_dir, api) = make_api();

        let mut input = make_full_input();
        input.project_id = "".to_string();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(
            matches!(err, WriterError::Other(ref msg) if msg.contains("project_id must not be empty"))
        );
    }

    #[test]
    fn restore_project_tree_invalid_project_id_format_returns_err() {
        let (_dir, api) = make_api();

        let mut input = make_full_input();
        input.project_id = "not-a-uuid".to_string();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(
            matches!(err, WriterError::Other(ref msg) if msg.contains("invalid project_id format"))
        );
    }

    #[test]
    fn restore_project_tree_invalid_volume_id_format_returns_err() {
        let (_dir, api) = make_api();

        let mut input = make_full_input();
        input.volumes[0].volume_id = "bad-volume-id".to_string();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(
            matches!(err, WriterError::Other(ref msg) if msg.contains("invalid volume_id format"))
        );
    }

    #[test]
    fn restore_project_tree_invalid_chapter_id_format_returns_err() {
        let (_dir, api) = make_api();

        let mut input = make_full_input();
        input.volumes[0].chapters[0].chapter_id = "bad-chapter-id".to_string();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(
            matches!(err, WriterError::Other(ref msg) if msg.contains("invalid chapter_id format"))
        );
    }
}
