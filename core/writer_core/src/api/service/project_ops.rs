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
        self.record_workspace_change_set_history(&change_set, "create_project");
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
        self.record_workspace_change_set_history(&change_set, "rename_project");
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
        self.record_workspace_change_set_history(&change_set, "reorder_projects");
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
        self.record_workspace_change_set_history(&change_set, "create_volume");
        Ok(volume)
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
        self.record_workspace_change_set_history(&change_set, "rename_volume");
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
        self.record_workspace_change_set_history(&change_set, "delete_volume");
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
        self.record_workspace_change_set_history(&change_set, "reorder_volumes");
        Ok(true)
    }
}
