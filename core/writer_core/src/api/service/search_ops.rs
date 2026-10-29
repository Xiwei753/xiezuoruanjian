use super::*;

fn parse_scope(scope: &str) -> crate::search::SearchScope {
    match scope {
        "chapterBody" => crate::search::SearchScope::ChapterBody,
        "chapterTitle" => crate::search::SearchScope::ChapterTitle,
        "chapterNote" => crate::search::SearchScope::ChapterNote,
        "projectTitle" => crate::search::SearchScope::ProjectTitle,
        "volumeTitle" => crate::search::SearchScope::VolumeTitle,
        "starmapTitle" => crate::search::SearchScope::StarmapTitle,
        "starmapNode" => crate::search::SearchScope::StarmapNode,
        "starmapEdgeLabel" => crate::search::SearchScope::StarmapEdgeLabel,
        "starmapLink" => crate::search::SearchScope::StarmapLink,
        "starmapEmbed" => crate::search::SearchScope::StarmapEmbed,
        "setting" => crate::search::SearchScope::Setting,
        "annotationTag" => crate::search::SearchScope::AnnotationTag,
        "annotationEntry" => crate::search::SearchScope::AnnotationEntry,
        "annotationLink" => crate::search::SearchScope::AnnotationLink,
        _ => crate::search::SearchScope::All,
    }
}

impl WriterCoreApi {
    /// 旧接口：只返回当前页条目数组，供尚未迁移的调用方使用。
    pub fn global_search_json(
        &self,
        query: &str,
        scope: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> ApiResult<String> {
        let results = self.search_service_search(query, parse_scope(scope), limit as usize, cursor);
        Self::json_string(&results)
    }

    /// 带来源上下文与翻页游标的完整搜索页。
    pub fn global_search_page_json(
        &self,
        query: &str,
        scope: &str,
        limit: u32,
        cursor: Option<&str>,
        context: Option<&crate::search::SearchContext>,
    ) -> ApiResult<String> {
        let context = context.cloned().unwrap_or_default();
        let page =
            self.search_service_page(query, parse_scope(scope), limit as usize, cursor, &context);
        Self::json_string(&page)
    }

    pub fn rebuild_search_index_json(&self, project_id: Option<&str>) -> ApiResult<String> {
        let status = self
            .search_service_rebuild(project_id)
            .map_err(WriterError::from)?;
        Self::json_string(&status)
    }

    pub fn get_search_index_status_json(&self) -> ApiResult<String> {
        let status = self.search_service_status();
        Self::json_string(&status)
    }

    pub fn enqueue_search_index_update_public(
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
        let target = target.cloned();
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action,
            object_id: object_id.to_string(),
            scope: parse_scope(scope),
            title: title.to_string(),
            body: body.to_string(),
            target,
        });
        Ok(true)
    }

    /// 平台端常驻搜索 worker 直接提交结构化索引更新。
    /// 索引是缓存，调用方只提交变化对象，不触发磁盘扫描。
    pub fn apply_search_index_update(&self, update: crate::search::SearchIndexUpdate) {
        self.enqueue_search_index_update(update);
    }
}
