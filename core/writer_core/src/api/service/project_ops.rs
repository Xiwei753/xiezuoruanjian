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
        let project: ProjectDto = self
            .core_write()
            .create_project(title)
            .map(Into::into)
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
        // #645 评论 5504296097 问题3：写事务完成后记录本地历史。
        self.record_workspace_history(&[], "create_project");
        Ok(project)
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
        self.core_write().rename_project(project_id, new_title)?;
        let entry = crate::search::extractor::extract_project_title_entry(project_id, new_title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        self.record_workspace_history(&[], "rename_project");
        Ok(true)
    }

    pub fn delete_project(&self, project_id: &str) -> ApiResult<bool> {
        let bound_starmaps = self
            .core_write()
            .list_starmaps_bound_to_project(project_id)
            .unwrap_or_default();
        self.core_write().delete_project(project_id)?;
        for prefix in &[
            format!("project:{}", project_id),
            format!("volume:{}:", project_id),
            format!("chapter_title:{}:", project_id),
            format!("chapter_body:{}:", project_id),
            format!("chapter_note:{}:", project_id),
        ] {
            self.remove_search_index_by_prefix(prefix);
        }
        for sm in &bound_starmaps {
            let _ = self.unbind_starmap_from_project(&sm.starmap_id);
        }
        self.record_workspace_history(&[], "delete_project");
        Ok(true)
    }

    pub fn reorder_projects(&self, ordered_project_ids: &[String]) -> ApiResult<bool> {
        self.core_write()
            .reorder_projects(ordered_project_ids)
            .map(|_| true)
            .map_err(crate::api::error::WriterError::from)?;
        self.record_workspace_history(&[], "reorder_projects");
        Ok(true)
    }

    pub fn list_volumes(&self, project_id: &str) -> ApiResult<Vec<VolumeDto>> {
        self.core_read()
            .list_volumes(project_id)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn create_volume(&self, project_id: &str, title: &str) -> ApiResult<VolumeDto> {
        let volume: VolumeDto = self
            .core_write()
            .create_volume(project_id, title)
            .map(Into::into)
            .map_err(WriterError::from)?;
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
        self.record_workspace_history(&[], "create_volume");
        Ok(volume)
    }

    pub fn rename_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        new_title: &str,
    ) -> ApiResult<bool> {
        self.core_write()
            .rename_volume(project_id, volume_id, new_title)?;
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
        self.record_workspace_history(&[], "rename_volume");
        Ok(true)
    }

    pub fn delete_volume(&self, project_id: &str, volume_id: &str) -> ApiResult<bool> {
        self.core_write().delete_volume(project_id, volume_id)?;
        for prefix in &[
            format!("volume:{}:{}", project_id, volume_id),
            format!("chapter_title:{}:{}:", project_id, volume_id),
            format!("chapter_body:{}:{}:", project_id, volume_id),
            format!("chapter_note:{}:{}:", project_id, volume_id),
        ] {
            self.remove_search_index_by_prefix(prefix);
        }
        self.record_workspace_history(&[], "delete_volume");
        Ok(true)
    }

    pub fn reorder_volumes(
        &self,
        project_id: &str,
        ordered_volume_ids: &[String],
    ) -> ApiResult<bool> {
        self.core_write()
            .reorder_volumes(project_id, ordered_volume_ids)
            .map(|_| true)
            .map_err(crate::api::error::WriterError::from)?;
        self.record_workspace_history(&[], "reorder_volumes");
        Ok(true)
    }
}
