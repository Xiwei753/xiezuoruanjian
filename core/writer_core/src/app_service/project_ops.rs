use crate::api::{ProjectDto, ProjectStatsDto, RecentEditDto, WriterError};

impl super::WriterAppService {
    pub fn list_projects(&self) -> Result<Vec<ProjectDto>, WriterError> {
        self.api.list_projects()
    }

    pub fn get_recent_edits(&self) -> Result<Vec<RecentEditDto>, WriterError> {
        self.api.get_recent_edits()
    }

    pub fn record_recent_edit(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<bool, WriterError> {
        self.api
            .record_recent_edit(&project_id, &volume_id, &chapter_id)
    }

    pub fn flush_recent_edits(&self) -> Result<bool, WriterError> {
        self.api.flush_recent_edits()
    }

    pub fn create_project(&self, title: String) -> Result<ProjectDto, WriterError> {
        self.api.create_project(&title)
    }

    pub fn get_project_stats(&self, project_id: String) -> Result<ProjectStatsDto, WriterError> {
        self.api.get_project_stats(&project_id)
    }

    pub fn rename_project(
        &self,
        project_id: String,
        new_title: String,
    ) -> Result<bool, WriterError> {
        self.api.rename_project(&project_id, &new_title)
    }

    pub fn delete_project(&self, project_id: String) -> Result<bool, WriterError> {
        self.api.delete_project(&project_id)
    }

    pub fn reorder_projects(&self, ordered_project_ids: Vec<String>) -> Result<bool, WriterError> {
        self.api.reorder_projects(&ordered_project_ids)
    }
}
