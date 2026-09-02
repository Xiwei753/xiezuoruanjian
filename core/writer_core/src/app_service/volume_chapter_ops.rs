use crate::api::{
    ChapterContentDto, ChapterMetaDto, ChapterSaveReceiptDto, ProjectWorkspaceSnapshotDto,
    VolumeDto, WriterError,
};

impl super::WriterAppService {
    pub fn list_volumes(&self, project_id: String) -> Result<Vec<VolumeDto>, WriterError> {
        self.api.list_volumes(&project_id)
    }

    /// #644 评论 5467821839 第7节：一次返回作品的全部卷 + 章节 + 统计快照。
    pub fn get_project_workspace_snapshot(
        &self,
        project_id: String,
    ) -> Result<ProjectWorkspaceSnapshotDto, WriterError> {
        self.api.get_project_workspace_snapshot(&project_id)
    }

    pub fn create_volume(
        &self,
        project_id: String,
        title: String,
    ) -> Result<VolumeDto, WriterError> {
        self.api.create_volume(&project_id, &title)
    }

    pub fn rename_volume(
        &self,
        project_id: String,
        volume_id: String,
        new_title: String,
    ) -> Result<bool, WriterError> {
        self.api.rename_volume(&project_id, &volume_id, &new_title)
    }

    pub fn delete_volume(
        &self,
        project_id: String,
        volume_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_volume(&project_id, &volume_id)
    }

    pub fn reorder_volumes(
        &self,
        project_id: String,
        ordered_volume_ids: Vec<String>,
    ) -> Result<bool, WriterError> {
        self.api.reorder_volumes(&project_id, &ordered_volume_ids)
    }

    pub fn list_chapters(
        &self,
        project_id: String,
        volume_id: String,
    ) -> Result<Vec<ChapterMetaDto>, WriterError> {
        self.api.list_chapters(&project_id, &volume_id)
    }

    pub fn create_chapter(
        &self,
        project_id: String,
        volume_id: String,
        title: String,
    ) -> Result<ChapterMetaDto, WriterError> {
        self.api.create_chapter(&project_id, &volume_id, &title)
    }

    pub fn create_chapter_in_project(
        &self,
        project_id: String,
        title: String,
    ) -> Result<ChapterMetaDto, WriterError> {
        self.api.create_chapter_in_project(&project_id, &title)
    }

    pub fn rename_chapter(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        new_title: String,
    ) -> Result<bool, WriterError> {
        self.api
            .rename_chapter(&project_id, &volume_id, &chapter_id, &new_title)
    }

    pub fn delete_chapter(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<bool, WriterError> {
        self.api
            .delete_chapter(&project_id, &volume_id, &chapter_id)
    }

    pub fn reorder_chapters(
        &self,
        project_id: String,
        volume_id: String,
        ordered_chapter_ids: Vec<String>,
    ) -> Result<bool, WriterError> {
        self.api
            .reorder_chapters(&project_id, &volume_id, &ordered_chapter_ids)
    }

    pub fn open_chapter(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<ChapterContentDto, WriterError> {
        self.api.open_chapter(&project_id, &volume_id, &chapter_id)
    }

    pub fn save_chapter_content(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api
            .save_chapter_content(&project_id, &volume_id, &chapter_id, &content)
    }

    pub fn save_chapter_content_with_options(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
        allow_empty_overwrite: bool,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api.save_chapter_content_with_options(
            &project_id,
            &volume_id,
            &chapter_id,
            &content,
            allow_empty_overwrite,
        )
    }

    pub fn clear_chapter_content(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api
            .clear_chapter_content(&project_id, &volume_id, &chapter_id)
    }

    pub fn update_chapter_note(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        note: String,
    ) -> Result<bool, WriterError> {
        self.api
            .update_chapter_note(&project_id, &volume_id, &chapter_id, &note)
    }
}
