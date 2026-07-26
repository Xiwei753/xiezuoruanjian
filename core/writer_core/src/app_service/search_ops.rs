use crate::api::WriterError;

impl super::WriterAppService {
    pub fn global_search(
        &self,
        query: String,
        scope: String,
        limit: u32,
        cursor: Option<String>,
    ) -> Result<String, WriterError> {
        self.api.global_search_json(&query, &scope, limit, cursor.as_deref())
    }

    pub fn rebuild_search_index(
        &self,
        project_id: Option<String>,
    ) -> Result<String, WriterError> {
        self.api.rebuild_search_index_json(project_id.as_deref())
    }

    pub fn get_search_index_status(&self) -> Result<String, WriterError> {
        self.api.get_search_index_status_json()
    }

    pub fn enqueue_search_update(
        &self,
        action: String,
        object_id: String,
        scope: String,
        title: String,
        body: String,
    ) -> Result<bool, WriterError> {
        self.api.enqueue_search_index_update(&action, &object_id, &scope, &title, &body, None)
    }

    pub fn process_pending_search_updates(&self) -> Result<bool, WriterError> {
        self.api.process_pending_search_updates()
    }
}
