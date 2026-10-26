use crate::error::Result;
use crate::search::types::*;

impl super::WriterCore {
    pub fn annotation_search_object_ids_for_project(&self, project_id: &str) -> Vec<String> {
        let service = self
            .search_service
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        service.annotation_object_ids_for_project(project_id)
    }

    pub fn auto_link_matches(
        &self,
        project_id: &str,
        chapter_id: &str,
        text_revision: u64,
        text: &str,
        entries: &[crate::annotations::NamedEntry],
        manual_ranges: &[(usize, usize)],
    ) -> crate::annotations::AutoMatchSnapshot {
        let mut matcher = self
            .annotation_matcher
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        matcher.update_chapter(
            project_id,
            chapter_id,
            text_revision,
            text,
            entries,
            manual_ranges,
        )
    }

    pub fn global_search(
        &self,
        query: &str,
        scope: SearchScope,
        limit: usize,
        cursor: Option<&str>,
        context: &SearchContext,
    ) -> SearchResultPage {
        let mut service = self
            .search_service
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        service.search_page(query, scope, limit, cursor, context)
    }

    pub fn rebuild_search_index(&self, project_id: Option<&str>) -> Result<SearchIndexStatus> {
        let entries = super::super::search::rebuild::rebuild_index(
            &self.app_data_root,
            &self.projects_root,
            project_id,
        )?;
        let mut service = self
            .search_service
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(pid) = project_id {
            service.rebuild_project_from_entries(pid, entries);
        } else {
            service.rebuild_from_entries(entries);
        }
        Ok(service.status())
    }

    pub fn get_search_index_status(&self) -> SearchIndexStatus {
        let service = self
            .search_service
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        service.status()
    }

    pub fn enqueue_search_index_update(&self, update: SearchIndexUpdate) {
        let mut service = self
            .search_service
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        service.enqueue_update(update);
    }

    pub fn remove_search_index_by_prefix(&self, prefix: &str) {
        let mut service = self
            .search_service
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        service.remove_by_prefix(prefix);
    }
}
