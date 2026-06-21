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

    pub fn create_chapter_in_project(
        &self,
        project_id: &str,
        title: &str,
    ) -> ApiResult<ChapterMetaDto> {
        let volumes = self.list_volumes(project_id)?;
        let volume_id = if let Some(vol) = volumes.first() {
            vol.id.clone()
        } else {
            let new_vol = self.create_volume(project_id, "第一卷")?;
            new_vol.id
        };
        self.create_chapter(project_id, &volume_id, title)
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

    // --- Chapter/Project/Volume envelope helpers (internal) ---

    #[allow(dead_code)]
    fn changed_value_envelope<T: serde::Serialize>(
        data: T,
        entity_type: &str,
        entity_id: Option<String>,
    ) -> ResultEnvelope<T> {
        ResultEnvelope::success_with_changes(
            data,
            Vec::new(),
            vec![ChangedEntityDto {
                entity_type: entity_type.to_string(),
                entity_id,
            }],
        )
    }

    #[allow(dead_code)]
    fn changed_result_envelope<T: serde::Serialize>(
        result: ApiResult<T>,
        entity_type: &str,
        entity_id: Option<String>,
    ) -> ResultEnvelope<T> {
        match result {
            Ok(data) => Self::changed_value_envelope(data, entity_type, entity_id),
            Err(error) => ResultEnvelope::error(error),
        }
    }


    #[allow(dead_code)]
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


}
