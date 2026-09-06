use crate::api::{
    ProjectDto, ProjectStatsDto, ProjectSummaryDto, RecentEditDto, RestoreProjectInputDto,
    WriterError,
};

impl super::WriterAppService {
    pub fn list_projects(&self) -> Result<Vec<ProjectDto>, WriterError> {
        self.api.list_projects()
    }

    /// #625 第二段：批量返回项目摘要（元数据 + 统计）。
    pub fn list_project_summaries(&self) -> Result<Vec<ProjectSummaryDto>, WriterError> {
        self.api.list_project_summaries()
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

    /// #649 评论 5561286861 第 4 点：恢复/导入项目——使用 manifest 中的稳定 ID。
    pub fn create_project_with_id(
        &self,
        id: String,
        title: String,
        order: i32,
    ) -> Result<ProjectDto, WriterError> {
        self.api.create_project_with_id(&id, &title, order)
    }

    /// #649 评论 5561465552 第 2 点：恢复作品树——一次跨 FFI 传入完整作品树。
    ///
    /// Core 负责校验 ID、校验目标不冲突、原子发布、记录 workspace Git 变更。
    pub fn restore_project_tree(
        &self,
        input: RestoreProjectInputDto,
    ) -> Result<ProjectDto, WriterError> {
        self.api.restore_project_tree(&input)
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
