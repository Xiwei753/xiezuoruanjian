use super::*;

impl WriterCoreApi {
    pub fn create_project(&self, title: &str) -> ApiResult<ProjectDto> {
        let project: ProjectDto = self.core()
            .create_project(title)
            .map(Into::into)
            .map_err(WriterError::from)?;
        let entry = crate::search::extractor::extract_project_title_entry(
            &project.id, title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        if let Ok(volumes) = self.core().list_volumes(&project.id) {
            if let Some(default_vol) = volumes.first() {
                let vol_entry = crate::search::extractor::extract_volume_title_entry(
                    &project.id, &default_vol.id, &default_vol.title,
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
        Ok(project)
    }

    pub fn get_project_stats(&self, project_id: &str) -> ApiResult<ProjectStatsDto> {
        self.core()
            .get_project_stats(project_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn rename_project(&self, project_id: &str, new_title: &str) -> ApiResult<bool> {
        self.core()
            .rename_project(project_id, new_title)?;
        let entry = crate::search::extractor::extract_project_title_entry(
            project_id, new_title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(true)
    }

    pub fn delete_project(&self, project_id: &str) -> ApiResult<bool> {
        self.core()
            .delete_project(project_id)?;
        for prefix in &[
            format!("project:{}", project_id),
            format!("volume:{}:", project_id),
            format!("chapter_title:{}:", project_id),
            format!("chapter_body:{}:", project_id),
            format!("chapter_note:{}:", project_id),
        ] {
            self.remove_search_index_by_prefix(prefix);
        }
        if let Ok(starmaps) = self.core().list_starmaps_bound_to_project(project_id) {
            for sm in &starmaps {
                for prefix in &[
                    format!("starmap:{}", sm.starmap_id),
                    format!("starmap_node:{}:", sm.starmap_id),
                    format!("starmap_edge:{}:", sm.starmap_id),
                    format!("starmap_hyperlink:{}:", sm.starmap_id),
                    format!("starmap_link:{}:", sm.starmap_id),
                    format!("starmap_embed:{}:", sm.starmap_id),
                ] {
                    self.remove_search_index_by_prefix(prefix);
                }
            }
        }
        Ok(true)
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
        let volume: VolumeDto = self.core()
            .create_volume(project_id, title)
            .map(Into::into)
            .map_err(WriterError::from)?;
        let entry = crate::search::extractor::extract_volume_title_entry(
            project_id, &volume.id, title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(volume)
    }

    pub fn rename_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        new_title: &str,
    ) -> ApiResult<bool> {
        self.core()
            .rename_volume(project_id, volume_id, new_title)?;
        let entry = crate::search::extractor::extract_volume_title_entry(
            project_id, volume_id, new_title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(true)
    }

    pub fn delete_volume(&self, project_id: &str, volume_id: &str) -> ApiResult<bool> {
        self.core()
            .delete_volume(project_id, volume_id)?;
        for prefix in &[
            format!("volume:{}:{}", project_id, volume_id),
            format!("chapter_title:{}:{}:", project_id, volume_id),
            format!("chapter_body:{}:{}:", project_id, volume_id),
            format!("chapter_note:{}:{}:", project_id, volume_id),
        ] {
            self.remove_search_index_by_prefix(prefix);
        }
        Ok(true)
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
}
