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
        crate::search::api::global_search(query, scope, limit, cursor)
    }

    pub fn rebuild_search_index(&self, project_id: Option<&str>) -> Result<SearchIndexStatus> {
        crate::search::api::rebuild_search_index(&self.workspace_path, project_id)
    }

    pub fn get_search_index_status(&self) -> SearchIndexStatus {
        crate::search::api::get_search_index_status()
    }

    pub fn enqueue_search_index_update(&self, update: SearchIndexUpdate) {
        crate::search::api::enqueue_search_index_update(update);
    }

    pub fn process_pending_search_updates(&self) {
        crate::search::api::process_pending_updates();
    }
}
