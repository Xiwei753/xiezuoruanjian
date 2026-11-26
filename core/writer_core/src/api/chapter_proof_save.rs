//! 正文保存的显式证明批次入口（Issue #1062 1.2）。
//!
//! 平台把随快照冻结的原始编辑事实（`ChapterProofProjection` JSON 数组）
//! 直接交给 Core；Core 逐条 replay 校验，再把正文、版本、meta、annotation
//! 与证明一起写进同一个 durable journal。这里不查询全局 pending map，也不
//! 用正文摘要猜批次 —— 批次是请求的一部分。

use super::chapter_api::get_chapter_title;
use super::service::{ApiResult, WriterCoreApi};
use super::types::*;
use crate::api::error::WriterError;

impl WriterCoreApi {
    /// 保存正文并携带平台冻结的原始证明批次（Issue #1062 1.2）。
    ///
    /// 与 `_with_batch` 的区别：不再按 event ID 去全局 pending map 取证明，
    /// 也不按正文摘要猜批次；`proof_batch_json` 是平台随快照冻结的原始事实
    /// （`ChapterProofProjection` JSON 数组），Core 逐条 replay 校验后与正文、
    /// 版本、meta、annotation 一起进入同一 durable journal。
    #[allow(clippy::too_many_arguments)]
    pub fn save_chapter_content_versioned_with_proof_batch(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
        proof_snapshot_revision: u64,
        proof_batch_json: &str,
        allow_empty_overwrite: bool,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.save_chapter_content_with_proof_batch_internal(
            project_id,
            volume_id,
            chapter_id,
            content,
            allow_empty_overwrite,
            false,
            expected_version,
            base_content.as_deref(),
            proof_snapshot_revision,
            proof_batch_json,
        )
        .map(|(dto, _)| dto)
    }

    /// 清空正文并携带平台冻结的原始证明批次；清空使用同一批次协议。
    pub fn clear_chapter_content_versioned_with_proof_batch(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
        proof_snapshot_revision: u64,
        proof_batch_json: &str,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.save_chapter_content_with_proof_batch_internal(
            project_id,
            volume_id,
            chapter_id,
            "",
            true,
            true,
            expected_version,
            base_content.as_deref(),
            proof_snapshot_revision,
            proof_batch_json,
        )
        .map(|(dto, _)| dto)
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn save_chapter_content_with_proof_batch_internal(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        allow_empty_overwrite: bool,
        clear_content: bool,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<&str>,
        proof_snapshot_revision: u64,
        proof_batch_json: &str,
    ) -> ApiResult<(ChapterSaveReceiptDto, crate::chapter::ChapterSaveReceipt)> {
        let proof_events: Vec<crate::chapter::ChapterProofProjection> =
            if proof_batch_json.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(proof_batch_json).map_err(WriterError::from)?
            };
        let workspace_lock = crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)
            .map_err(WriterError::from)?;
        let pending_annotations =
            self.prepare_annotation_edit_save(project_id, chapter_id, content)?;
        let additional_files = pending_annotations
            .as_ref()
            .map(|pending| pending.prepared.files.clone())
            .unwrap_or_default();
        let expected_version = expected_version.map(ChapterVersionDto::into_core);
        let (receipt, change_set) = self
            .core_write()
            .save_chapter_verified_with_proof_batch(
                project_id,
                volume_id,
                chapter_id,
                content,
                allow_empty_overwrite,
                &additional_files,
                expected_version,
                base_content,
                proof_snapshot_revision,
                &proof_events,
                Some(&workspace_lock),
            )
            .map_err(WriterError::from)?;
        self.finalize_chapter_content_save(
            project_id,
            volume_id,
            chapter_id,
            content,
            clear_content,
            pending_annotations,
            &change_set,
            receipt,
        )
    }

    /// 正文落盘后的统一收尾：搜索索引、标注会话、workspace history。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finalize_chapter_content_save(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        clear_content: bool,
        pending_annotations: Option<crate::api::service::PreparedAnnotationEditSave>,
        change_set: &crate::storage::workspace_git::WorkspaceChangeSet,
        receipt: crate::chapter::ChapterSaveReceipt,
    ) -> ApiResult<(ChapterSaveReceiptDto, crate::chapter::ChapterSaveReceipt)> {
        if clear_content {
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Delete,
                object_id: format!("chapter_body:{}:{}:{}", project_id, volume_id, chapter_id),
                scope: crate::search::SearchScope::All,
                title: String::new(),
                body: String::new(),
                target: None,
            });
        } else {
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
        }
        if let Some(pending) = pending_annotations {
            if pending.baseline_id.is_some() {
                self.finish_annotation_edit_save(&pending)?;
            }
            self.refresh_annotation_search_index(project_id, "save_chapter_content");
        }
        // 空 change set 表示正文、元数据和附加文件都没有实质变化：
        // 不写新的 workspace history / 对象 mutation，也不刷新版本事实。
        if !change_set.is_empty() {
            let _ = self.record_workspace_change_set_history(
                change_set,
                if clear_content {
                    "clear_chapter_content"
                } else {
                    "save_chapter_content"
                },
            );
        }
        Ok((receipt.clone().into(), receipt))
    }
}
