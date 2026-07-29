use super::service::{ApiResult, WriterCoreApi};
use super::types::*;
use crate::api::error::WriterError;

fn get_chapter_title(api: &WriterCoreApi, project_id: &str, volume_id: &str, chapter_id: &str) -> String {
    api.core()
        .list_chapters(project_id, volume_id)
        .ok()
        .and_then(|chapters| chapters.into_iter().find(|c| c.id == chapter_id))
        .map(|c| c.title)
        .unwrap_or_default()
}

/// 章节 API — 跨平台章节 CRUD 契约。
///
/// 所有 `project_id`/`volume_id`/`chapter_id` 均为 Core 分配的 UUID 字符串。
/// 正文内容始终为 UTF-8 纯文本，`save_chapter_content` 通过 `write_chapter_verified`
/// 执行原子写入（tmp+rename），防止损坏。
impl WriterCoreApi {
    /// 列出指定卷下的所有章节，按 `order` 字段排序。
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

    /// 创建章节，自动分配 UUID 和递增 order。
    pub fn create_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        title: &str,
    ) -> ApiResult<ChapterMetaDto> {
        let chapter: ChapterMetaDto = self.core()
            .create_chapter(project_id, volume_id, title)
            .map(Into::into)
            .map_err(WriterError::from)?;
        let title_entry = crate::search::extractor::extract_chapter_title_entry(
            project_id, volume_id, &chapter.id, title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: title_entry.object_id.clone(),
            scope: title_entry.scope,
            title: title_entry.title.clone(),
            body: title_entry.body.clone(),
            target: Some(title_entry.target.clone()),
        });
        Ok(chapter)
    }

    /// 在项目中创建章节——若项目无卷则自动创建"第一卷"。
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

    /// 重命名章节标题。
    pub fn rename_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        new_title: &str,
    ) -> ApiResult<bool> {
        self.core()
            .rename_chapter(project_id, volume_id, chapter_id, new_title)?;
        let title_entry = crate::search::extractor::extract_chapter_title_entry(
            project_id, volume_id, chapter_id, new_title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: title_entry.object_id.clone(),
            scope: title_entry.scope,
            title: title_entry.title.clone(),
            body: title_entry.body.clone(),
            target: Some(title_entry.target.clone()),
        });
        let chapter_result = self.core().open_chapter(project_id, volume_id, chapter_id);
        if let Ok(content) = chapter_result {
            if !content.content.is_empty() {
                let body_entry = crate::search::extractor::extract_chapter_body_entry(
                    project_id, volume_id, chapter_id, new_title, &content.content,
                );
                self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                    action: crate::search::SearchIndexAction::Upsert,
                    object_id: body_entry.object_id.clone(),
                    scope: body_entry.scope,
                    title: body_entry.title.clone(),
                    body: body_entry.body.clone(),
                    target: Some(body_entry.target.clone()),
                });
            }
        }
        let ch_note = self.core()
            .list_chapters(project_id, volume_id)
            .ok()
            .and_then(|chapters| chapters.into_iter().find(|c| c.id == chapter_id))
            .and_then(|c| c.note);
        if let Some(ref note) = ch_note {
            if !note.is_empty() {
                let note_entry = crate::search::extractor::extract_chapter_note_entry(
                    project_id, volume_id, chapter_id, new_title, note,
                );
                self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                    action: crate::search::SearchIndexAction::Upsert,
                    object_id: note_entry.object_id.clone(),
                    scope: note_entry.scope,
                    title: note_entry.title.clone(),
                    body: note_entry.body.clone(),
                    target: Some(note_entry.target.clone()),
                });
            }
        }
        Ok(true)
    }

    /// 删除章节（磁盘文件移至 trash，非立即删除）。
    pub fn delete_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .delete_chapter(project_id, volume_id, chapter_id)?;
        for prefix in &[
            format!("chapter_title:{}:{}:{}", project_id, volume_id, chapter_id),
            format!("chapter_body:{}:{}:{}", project_id, volume_id, chapter_id),
            format!("chapter_note:{}:{}:{}", project_id, volume_id, chapter_id),
        ] {
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Delete,
                object_id: prefix.clone(),
                scope: crate::search::SearchScope::All,
                title: String::new(),
                body: String::new(),
                target: None,
            });
        }
        Ok(true)
    }

    /// 重排章节顺序。`ordered_chapter_ids` 必须包含该卷下所有章节 ID。
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

    /// 打开章节，返回正文内容和元数据。
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

    /// 保存章节正文（原子写入）。空内容覆盖非空章节会被 `EmptyOverwriteBlocked` 拦截。
    pub fn save_chapter_content(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        let receipt: ChapterSaveReceiptDto = self.core()
            .write_chapter_verified(project_id, volume_id, chapter_id, content)
            .map(Into::into)?;
        let ch_title = get_chapter_title(self, project_id, volume_id, chapter_id);
        let entry = crate::search::extractor::extract_chapter_body_entry(
            project_id, volume_id, chapter_id, &ch_title, content,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(receipt)
    }

    /// 保存章节正文（带空覆盖控制）。`allow_empty_overwrite=true` 绕过安全拦截。
    pub fn save_chapter_content_with_options(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        allow_empty_overwrite: bool,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        let receipt: ChapterSaveReceiptDto = self.core()
            .write_chapter_verified_with_allow_empty_overwrite(
                project_id,
                volume_id,
                chapter_id,
                content,
                allow_empty_overwrite,
            )
            .map(Into::into)?;
        let ch_title = get_chapter_title(self, project_id, volume_id, chapter_id);
        let entry = crate::search::extractor::extract_chapter_body_entry(
            project_id, volume_id, chapter_id, &ch_title, content,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(receipt)
    }

    /// 清空章节正文（等价于 `save_chapter_content_with_options(..., true)`）。
    pub fn clear_chapter_content(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        let receipt: ChapterSaveReceiptDto = self.core()
            .clear_chapter_content_verified(project_id, volume_id, chapter_id)
            .map(Into::into)?;
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("chapter_body:{}:{}:{}", project_id, volume_id, chapter_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        Ok(receipt)
    }

    /// 更新章节备注（note 字段，独立于正文）。
    pub fn update_chapter_note(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        note: &str,
    ) -> ApiResult<bool> {
        self.core()
            .update_chapter_note(project_id, volume_id, chapter_id, note)?;
        let ch_title = get_chapter_title(self, project_id, volume_id, chapter_id);
        let entry = crate::search::extractor::extract_chapter_note_entry(
            project_id, volume_id, chapter_id, &ch_title, note,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(true)
    }

}
