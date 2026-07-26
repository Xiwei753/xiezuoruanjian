use crate::error::Result;
use crate::search::types::*;

impl super::WriterCore {
    pub fn global_search(
        &self,
        query: &str,
        scope: SearchScope,
        limit: usize,
        cursor: Option<&str>,
    ) -> Vec<SearchResult> {
        let mut service = self.search_service.lock().unwrap_or_else(|e| e.into_inner());
        service.search(query, scope, limit, cursor)
    }

    pub fn rebuild_search_index(&self, project_id: Option<&str>) -> Result<SearchIndexStatus> {
        let entries = super::super::search::rebuild::rebuild_index(&self.workspace_path, project_id)?;
        let mut service = self.search_service.lock().unwrap_or_else(|e| e.into_inner());
        service.rebuild_from_entries(entries);
        Ok(service.status())
    }

    pub fn get_search_index_status(&self) -> SearchIndexStatus {
        let service = self.search_service.lock().unwrap_or_else(|e| e.into_inner());
        service.status()
    }

    pub fn enqueue_search_index_update(&self, update: SearchIndexUpdate) {
        let mut service = self.search_service.lock().unwrap_or_else(|e| e.into_inner());
        service.enqueue_update(update);
    }

    pub fn process_pending_search_updates(&self) {
        let mut service = self.search_service.lock().unwrap_or_else(|e| e.into_inner());
        service.process_updates();
    }

    pub fn remove_search_index_by_prefix(&self, prefix: &str) {
        let mut service = self.search_service.lock().unwrap_or_else(|e| e.into_inner());
        service.remove_by_prefix(prefix);
    }
}
