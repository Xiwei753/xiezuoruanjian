use super::service::{ApiResult, WriterCoreApi};
use super::types::*;
use super::{ChangedEntityDto, ResultEnvelope};

impl WriterCoreApi {
    pub fn list_chapters(
        &self,
        project_id: &str,
        volume_id: &str,
    ) -> ApiResult<Vec<ChapterMetaDto>> {
        self.core()
            .list_chapters(project_id, volume_id)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn create_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        title: &str,
    ) -> ApiResult<ChapterMetaDto> {
        self.core()
            .create_chapter(project_id, volume_id, title)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn rename_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        new_title: &str,
    ) -> ApiResult<bool> {
        self.core()
            .rename_chapter(project_id, volume_id, chapter_id, new_title)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn delete_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .delete_chapter(project_id, volume_id, chapter_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn reorder_chapters(
        &self,
        project_id: &str,
        volume_id: &str,
        ordered_chapter_ids: &[String],
    ) -> ApiResult<bool> {
        self.core()
            .reorder_chapters(project_id, volume_id, ordered_chapter_ids)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn open_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<ChapterContentDto> {
        self.core()
            .open_chapter(project_id, volume_id, chapter_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_chapter_content(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.core()
            .write_chapter_verified(project_id, volume_id, chapter_id, content)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_chapter_content_with_options(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        allow_empty_overwrite: bool,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.core()
            .write_chapter_verified_with_allow_empty_overwrite(
                project_id,
                volume_id,
                chapter_id,
                content,
                allow_empty_overwrite,
            )
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn clear_chapter_content(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.core()
            .clear_chapter_content_verified(project_id, volume_id, chapter_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn update_chapter_note(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        note: &str,
    ) -> ApiResult<bool> {
        self.core()
            .update_chapter_note(project_id, volume_id, chapter_id, note)
            .map(|_| true)
            .map_err(Into::into)
    }

    // --- Chapter/Project envelope_json methods ---

    fn chapter_save_envelope(
        result: ApiResult<ChapterSaveReceiptDto>,
        chapter_id: &str,
    ) -> ResultEnvelope<ChapterSaveReceiptDto> {
        match result {
            Ok(receipt) => ResultEnvelope::success_with_changes(
                receipt,
                Vec::new(),
                vec![ChangedEntityDto {
                    entity_type: "ChapterSaved".to_string(),
                    entity_id: Some(chapter_id.to_string()),
                }],
            ),
            Err(error) => ResultEnvelope::error(error),
        }
    }

    pub fn save_chapter_content_envelope_json(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> String {
        Self::chapter_save_envelope(
            self.save_chapter_content(project_id, volume_id, chapter_id, content),
            chapter_id,
        )
        .to_json_string()
    }

    fn delete_envelope(
        result: ApiResult<bool>,
        entity_type: &str,
        entity_id: &str,
    ) -> ResultEnvelope<bool> {
        match result {
            Ok(_) => ResultEnvelope::success_with_changes(
                true,
                Vec::new(),
                vec![ChangedEntityDto {
                    entity_type: format!("{}Deleted", entity_type),
                    entity_id: Some(entity_id.to_string()),
                }],
            ),
            Err(error) => ResultEnvelope::error(error),
        }
    }

    pub fn delete_project_envelope_json(&self, project_id: &str) -> String {
        Self::delete_envelope(self.delete_project(project_id), "Project", project_id)
            .to_json_string()
    }

    pub fn delete_volume_envelope_json(&self, project_id: &str, volume_id: &str) -> String {
        Self::delete_envelope(
            self.delete_volume(project_id, volume_id),
            "Volume",
            volume_id,
        )
        .to_json_string()
    }

    pub fn delete_chapter_envelope_json(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> String {
        Self::delete_envelope(
            self.delete_chapter(project_id, volume_id, chapter_id),
            "Chapter",
            chapter_id,
        )
        .to_json_string()
    }
}
