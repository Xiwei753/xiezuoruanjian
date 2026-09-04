use super::*;

/// #645 评论 5504296097 Blocker 2：构造 project/volume 的 workspace-relative paths。
///
/// - project: `projects/{project_id}/project.json`
/// - volume: `projects/{project_id}/volumes/{volume_id}/volume.json`
fn project_json_rel_path(project_id: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("projects")
        .join(project_id)
        .join("project.json")
}

fn volume_json_rel_path(project_id: &str, volume_id: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("volume.json")
}

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
        // #645 评论 5504296097 Blocker 2：create_project 同时创建默认卷
        //（volume.json），记录 project.json + 默认卷 volume.json。
        let default_vol_id = volumes_result
            .as_ref()
            .ok()
            .and_then(|vols| vols.first().map(|v| v.id.clone()));
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
        // Blocker 2：精确传 project.json 路径，替代全量 &[] 扫描。
        let mut paths = vec![project_json_rel_path(&project.id)];
        if let Some(vid) = default_vol_id {
            paths.push(volume_json_rel_path(&project.id, &vid));
        }
        self.record_workspace_history(&paths, "create_project");
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
        // #645 评论 5504296097 Blocker 2：rename 只改 project.json。
        self.record_workspace_history(&[project_json_rel_path(project_id)], "rename_project");
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
        // #645 评论 5504296097 Blocker 2：删除作品移除整个 project 目录，
        // 传 project.json 路径，record_workspace_history 会用 remove_path
        // 从 index 移除已删除文件。
        self.record_workspace_history(&[project_json_rel_path(project_id)], "delete_project");
        Ok(true)
    }

    pub fn reorder_projects(&self, ordered_project_ids: &[String]) -> ApiResult<bool> {
        self.core_write()
            .reorder_projects(ordered_project_ids)
            .map(|_| true)
            .map_err(crate::api::error::WriterError::from)?;
        // #645 评论 5504296097 Blocker 2：reorder 改写每个 project.json 的 order。
        let changed_paths: Vec<std::path::PathBuf> = ordered_project_ids
            .iter()
            .map(|id| project_json_rel_path(id))
            .collect();
        self.record_workspace_history(&changed_paths, "reorder_projects");
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
        // #645 评论 5504296097 Blocker 2：create_volume 写 volume.json。
        self.record_workspace_history(
            &[volume_json_rel_path(project_id, &volume.id)],
            "create_volume",
        );
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
        // #645 评论 5504296097 Blocker 2：rename_volume 只改 volume.json。
        self.record_workspace_history(
            &[volume_json_rel_path(project_id, volume_id)],
            "rename_volume",
        );
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
        // #645 评论 5504296097 Blocker 2：删除卷移除整个 volume 目录，
        // 传 volume.json 路径，record_workspace_history 用 remove_path 移除。
        self.record_workspace_history(
            &[volume_json_rel_path(project_id, volume_id)],
            "delete_volume",
        );
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
        // #645 评论 5504296097 Blocker 2：reorder 改写每个 volume.json 的 order。
        let changed_paths: Vec<std::path::PathBuf> = ordered_volume_ids
            .iter()
            .map(|vid| volume_json_rel_path(project_id, vid))
            .collect();
        self.record_workspace_history(&changed_paths, "reorder_volumes");
        Ok(true)
    }
}
