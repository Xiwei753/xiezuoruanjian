use super::*;

impl WriterCoreApi {
    pub fn list_projects(&self) -> ApiResult<Vec<ProjectDto>> {
        self.core()
            .list_projects()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn get_recent_edits(&self) -> ApiResult<Vec<RecentEditDto>> {
        self.core()
            .get_recent_edits()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn record_recent_edit(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .record_recent_edit(project_id, volume_id, chapter_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn flush_recent_edits(&self) -> ApiResult<bool> {
        self.core()
            .flush_recent_edits()
            .map(|_| true)
            .map_err(WriterError::from)
    }
}
