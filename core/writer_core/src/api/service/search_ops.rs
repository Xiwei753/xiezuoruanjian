use super::*;

impl WriterCoreApi {
    pub fn global_search_json(
        &self,
        query: &str,
        scope: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> ApiResult<String> {
        let scope = match scope {
            "chapterBody" => crate::search::SearchScope::ChapterBody,
            "chapterTitle" => crate::search::SearchScope::ChapterTitle,
            "chapterNote" => crate::search::SearchScope::ChapterNote,
            "projectTitle" => crate::search::SearchScope::ProjectTitle,
            "volumeTitle" => crate::search::SearchScope::VolumeTitle,
            "starmapTitle" => crate::search::SearchScope::StarmapTitle,
            "starmapNode" => crate::search::SearchScope::StarmapNode,
            "starmapEdgeLabel" => crate::search::SearchScope::StarmapEdgeLabel,
            "starmapHyperlink" => crate::search::SearchScope::StarmapHyperlink,
            "setting" => crate::search::SearchScope::Setting,
            _ => crate::search::SearchScope::All,
        };
        let results = self.core().global_search(query, scope, limit as usize, cursor);
        Self::json_string(&results)
    }

    pub fn rebuild_search_index_json(&self, project_id: Option<&str>) -> ApiResult<String> {
        let status = self.core().rebuild_search_index(project_id).map_err(WriterError::from)?;
        Self::json_string(&status)
    }

    pub fn get_search_index_status_json(&self) -> ApiResult<String> {
        let status = self.core().get_search_index_status();
        Self::json_string(&status)
    }

    pub fn enqueue_search_index_update(
        &self,
        action: &str,
        object_id: &str,
        scope: &str,
        title: &str,
        body: &str,
        target: Option<&crate::search::SearchTarget>,
    ) -> ApiResult<bool> {
        let action = match action {
            "delete" => crate::search::SearchIndexAction::Delete,
            _ => crate::search::SearchIndexAction::Upsert,
        };
        let scope = match scope {
            "chapterBody" => crate::search::SearchScope::ChapterBody,
            "chapterTitle" => crate::search::SearchScope::ChapterTitle,
            "chapterNote" => crate::search::SearchScope::ChapterNote,
            "projectTitle" => crate::search::SearchScope::ProjectTitle,
            "volumeTitle" => crate::search::SearchScope::VolumeTitle,
            "starmapTitle" => crate::search::SearchScope::StarmapTitle,
            "starmapNode" => crate::search::SearchScope::StarmapNode,
            "starmapEdgeLabel" => crate::search::SearchScope::StarmapEdgeLabel,
            "starmapHyperlink" => crate::search::SearchScope::StarmapHyperlink,
            "setting" => crate::search::SearchScope::Setting,
            _ => crate::search::SearchScope::All,
        };
        let target = target.cloned();
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action,
            object_id: object_id.to_string(),
            scope,
            title: title.to_string(),
            body: body.to_string(),
            target,
        });
        Ok(true)
    }

    pub fn process_pending_search_updates(&self) -> ApiResult<bool> {
        self.core().process_pending_search_updates();
        Ok(true)
    }
}
