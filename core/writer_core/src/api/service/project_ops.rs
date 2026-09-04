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
        // #645 评论 5504296097 问题2：删除 best-effort unbind loop。
        // starmap 解绑已收进 `delete_project_with_changes` 的 durable delete transaction
        // （新增 `StarMapsUnbound` phase），解绑和删除原子化，避免半状态：
        // - 情况 A：解绑成功，作品删除失败 → starmap 已解绑但 project 还在
        // - 情况 B：某个解绑失败，作品删除成功 → 悬空引用
        // 现在只调用一次 `delete_project_with_changes`，成功后再统一清搜索索引、
        // 记录一次本地 history。

        // 在删除前获取绑定的 starmap ids，供删除成功后更新搜索索引。
        // 注意：不在此时解绑——解绑收进事务，避免半状态。
        let bound_starmap_ids: Vec<String> = self
            .core_write()
            .list_starmaps_bound_to_project(project_id)
            .unwrap_or_default()
            .into_iter()
            .map(|m| m.starmap_id)
            .collect();

        let change_set = self.core_write().delete_project_with_changes(project_id)?;

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

        // #645 评论 5504296097 问题2：删除成功后更新被解绑 starmap 的搜索索引。
        // core 层 `unbind_starmaps` 只改 starmap meta/index 文件，不更新搜索索引。
        // API 层负责把 starmap meta 的变化同步到搜索索引。
        for sm_id in &bound_starmap_ids {
            self.refresh_starmap_search_index(sm_id);
        }

        // history 记录是 best-effort：record_workspace_change_set_history
        // 内部已用 log::warn 吞掉 git 错误，不会把 history 失败当成删除失败。
        self.record_workspace_change_set_history(&change_set, "delete_project");
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
