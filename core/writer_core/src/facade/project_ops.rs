use crate::chapter::{self, Chapter, ChapterContent, ChapterSaveReceipt};
use crate::error::Result;
use crate::project::{self, Project};
use crate::volume::{self, Volume};

impl super::WriterCore {
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        project::list_projects(&self.workspace_path)
    }

    pub fn create_project(&self, title: &str) -> Result<Project> {
        project::create_project(&self.workspace_path, title)
    }

    pub fn list_volumes(&self, project_id: &str) -> Result<Vec<Volume>> {
        volume::list_volumes(&self.workspace_path, project_id)
    }

    pub fn create_volume(&self, project_id: &str, title: &str) -> Result<Volume> {
        volume::create_volume(&self.workspace_path, project_id, title)
    }

    pub fn list_valid_chapter_ids(&self, project_id: &str) -> Result<std::collections::HashSet<String>> {
        chapter::list_valid_chapter_ids(&self.workspace_path, project_id)
    }

    pub fn list_chapters(&self, project_id: &str, volume_id: &str) -> Result<Vec<Chapter>> {
        chapter::list_chapters(&self.workspace_path, project_id, volume_id)
    }

    pub fn calculate_word_count(&self, text: &str) -> u32 {
        chapter::calculate_word_count(text)
    }

    pub fn create_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        title: &str,
    ) -> Result<Chapter> {
        chapter::create_chapter(&self.workspace_path, project_id, volume_id, title)
    }

    pub fn get_project_stats(&self, project_id: &str) -> Result<crate::project::ProjectStats> {
        crate::project::get_project_stats(&self.workspace_path, project_id)
    }

    pub fn read_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<ChapterContent> {
        chapter::read_chapter(&self.workspace_path, project_id, volume_id, chapter_id)
    }

    pub fn open_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<super::ChapterOpenResult> {
        let content = self.read_chapter(project_id, volume_id, chapter_id)?;
        Ok(super::ChapterOpenResult {
            meta: content.meta,
            content: content.content,
        })
    }

    pub fn write_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> Result<()> {
        chapter::save_chapter(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
            content,
        )
    }

    pub fn clear_chapter_content(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<()> {
        chapter::clear_chapter_content(&self.workspace_path, project_id, volume_id, chapter_id)
    }

    pub fn clear_chapter_content_verified(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<ChapterSaveReceipt> {
        chapter::clear_chapter_content_verified(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
        )
    }

    pub fn write_chapter_verified(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> Result<ChapterSaveReceipt> {
        chapter::save_chapter_verified(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
            content,
        )
    }

    pub fn write_chapter_verified_with_allow_empty_overwrite(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        allow_empty_overwrite: bool,
    ) -> Result<ChapterSaveReceipt> {
        chapter::save_chapter_verified_with_allow_empty_overwrite(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
            content,
            allow_empty_overwrite,
        )
    }

    pub fn update_chapter_note(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        note: &str,
    ) -> Result<()> {
        chapter::update_chapter_note(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
            note,
        )
    }

    pub fn rename_project(&self, project_id: &str, new_title: &str) -> crate::error::Result<()> {
        crate::project::rename_project(&self.workspace_path, project_id, new_title)
    }

    pub fn delete_project(&self, project_id: &str) -> crate::error::Result<()> {
        crate::project::delete_project(&self.workspace_path, project_id)
    }

    pub fn reorder_projects(&self, ordered_ids: &[String]) -> crate::error::Result<()> {
        crate::project::reorder_projects(&self.workspace_path, ordered_ids)
    }

    pub fn rename_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        new_title: &str,
    ) -> crate::error::Result<()> {
        crate::volume::rename_volume(&self.workspace_path, project_id, volume_id, new_title)
    }

    pub fn delete_volume(&self, project_id: &str, volume_id: &str) -> crate::error::Result<()> {
        crate::volume::delete_volume(&self.workspace_path, project_id, volume_id)
    }

    pub fn reorder_volumes(
        &self,
        project_id: &str,
        ordered_ids: &[String],
    ) -> crate::error::Result<()> {
        crate::volume::reorder_volumes(&self.workspace_path, project_id, ordered_ids)
    }

    pub fn rename_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        new_title: &str,
    ) -> crate::error::Result<()> {
        crate::chapter::rename_chapter(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
            new_title,
        )
    }

    pub fn delete_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> crate::error::Result<()> {
        crate::chapter::delete_chapter(&self.workspace_path, project_id, volume_id, chapter_id)
    }

    pub fn reorder_chapters(
        &self,
        project_id: &str,
        volume_id: &str,
        ordered_ids: &[String],
    ) -> crate::error::Result<()> {
        crate::chapter::reorder_chapters(&self.workspace_path, project_id, volume_id, ordered_ids)
    }
}