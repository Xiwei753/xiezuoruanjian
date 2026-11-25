use super::service::{ApiResult, WriterCoreApi};
use super::types::*;
use crate::api::error::WriterError;

/// 跨卷移动的本地结果。
///
/// 只要文件事务已经提交，`moved` 就为 `true`，即使 workspace history 尚未
/// 落盘（`history_pending`）：平台端必须立即把编辑器和树迁移到目标卷，
/// 残留账本由 journal 在下次启动补齐。文件事务未提交时返回 `Err`。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveChapterOutcome {
    /// 本地章节目录是否已经完成移动。
    pub moved: bool,
    /// workspace history 是否仍待补记（journal 仍在）。
    pub history_pending: bool,
    /// 持久 journal token，供诊断与后续幂等补记。
    pub journal_token: String,
}

fn get_chapter_title(
    api: &WriterCoreApi,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> String {
    api.core_write()
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
        self.core_read()
            .list_chapters(project_id, volume_id)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    /// 恢复/导入章节——使用 manifest 中的稳定 ID。
    ///
    /// 恢复场景：不记录 workspace history（manifest 是已有事实来源）。
    pub fn create_chapter_with_id(
        &self,
        project_id: &str,
        volume_id: &str,
        id: &str,
        title: &str,
        order: i32,
    ) -> ApiResult<ChapterMetaDto> {
        let chapter = self
            .core_write()
            .create_chapter_with_id(project_id, volume_id, id, title, order)
            .map_err(WriterError::from)?;
        let title_entry =
            crate::search::extractor::extract_chapter_title_entry(project_id, volume_id, id, title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: title_entry.object_id.clone(),
            scope: title_entry.scope,
            title: title_entry.title.clone(),
            body: title_entry.body.clone(),
            target: Some(title_entry.target.clone()),
        });
        Ok(chapter.into())
    }

    /// 创建章节，自动分配 UUID 和递增 order。
    pub fn create_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        title: &str,
    ) -> ApiResult<ChapterMetaDto> {
        //   用 _with_changes 版本拿变更集。
        let (chapter, change_set) = self
            .core_write()
            .create_chapter_with_changes(project_id, volume_id, title)
            .map_err(WriterError::from)?;
        let title_entry = crate::search::extractor::extract_chapter_title_entry(
            project_id,
            volume_id,
            &chapter.id,
            title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: title_entry.object_id.clone(),
            scope: title_entry.scope,
            title: title_entry.title.clone(),
            body: title_entry.body.clone(),
            target: Some(title_entry.target.clone()),
        });
        let _ = self.record_workspace_change_set_history(&change_set, "create_chapter");
        Ok(chapter.into())
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
        //   用 _with_changes 版本拿变更集。
        let (chapter, change_set) = self
            .core_write()
            .rename_chapter_with_changes(project_id, volume_id, chapter_id, new_title)?;
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
        let chapter_result = self
            .core_write()
            .open_chapter(project_id, volume_id, chapter_id);
        if let Ok(content) = chapter_result {
            if !content.content.is_empty() {
                let body_entry = crate::search::extractor::extract_chapter_body_entry(
                    project_id,
                    volume_id,
                    chapter_id,
                    new_title,
                    &content.content,
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
        let ch_note = chapter.note;
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
        let _ = self.record_workspace_change_set_history(&change_set, "rename_chapter");
        Ok(true)
    }

    /// 删除章节（磁盘文件移至 trash，非立即删除）。
    pub fn delete_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<bool> {
        //   durable 删除事务：先 plan（不碰磁盘，构造完整 PlannedWorkspaceDelete
        // 含固定 trash 路径和完整 sync_delete_facts），再 save_pending 落盘 journal，
        // 再 apply planned delete（rename + 写 tombstone），再推进 journal 阶段。
        // 只有 save_pending 成功后才允许动本地文件，保证删除事实在物理删除前已持久化。
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
        let device_id = crate::settings::load_device_info(&self.app_data_root)
            .map(|i| i.device_id)
            .unwrap_or_default();
        let (change_set, planned) = self
            .core_write()
            .plan_delete_chapter(project_id, volume_id, chapter_id, &device_id)?;
        // 先落盘 journal（phase=Pending，包含完整 facts/固定 trash path），确保崩溃后能恢复。
        let journal =
            crate::storage::journal::workspace_change::WorkspaceChangeJournal::save_pending(
                &self.app_data_root,
                &change_set,
                &device_id,
                crate::storage::journal::workspace_change::WorkspaceChangeOpType::DeleteChapter,
                Some(planned.delete_target.clone()),
                Some(planned.clone()),
            )
            .map_err(|e| {
                log::warn!(
                    "delete_chapter: save_pending journal failed: {} — aborting before local delete",
                    e
                );
                crate::api::error::WriterError::Other(format!(
                    "delete_chapter: save_pending journal failed: {e}"
                ))
            })?;
        // journal 落盘成功，执行本地删除（消费 plan 中固定的 trash 路径和 facts）。
        if let Err(e) = self
            .core_write()
            .apply_planned_delete_chapter(project_id, volume_id, chapter_id, &planned)
            .map_err(WriterError::from)
        {
            log::warn!(
                "delete_chapter: local delete failed: {} — journal retained for recovery",
                e
            );
            return Err(e);
        }
        // 本地删除已完成，推进到 LocalApplied。
        if let Err(e) = journal.mark_local_applied(&self.app_data_root) {
            log::warn!(
                "delete_chapter: mark_local_applied failed: {} — journal retained for recovery",
                e
            );
        }
        // 写 workspace history，成功后清 journal；失败时 journal 保留供下次 bootstrap 补记。
        match self.record_workspace_change_set_history(&change_set, "delete_chapter") {
            Ok(()) => {
                if let Err(e) = journal.mark_history_recorded(&self.app_data_root) {
                    log::warn!(
                        "delete_chapter: mark_history_recorded failed: {} — journal retained",
                        e
                    );
                } else if let Err(e) = journal.clear_journal(&self.app_data_root) {
                    log::warn!("delete_chapter: clear_journal failed: {}", e);
                }
            }
            Err(e) => {
                log::warn!(
                    "delete_chapter: record_workspace_change_set_history failed: {} — \
                     journal retained for recovery, history will be补 on next startup",
                    e
                );
            }
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
        //   用 _with_changes 版本拿变更集。
        let change_set = self.core_write().reorder_chapters_with_changes(
            project_id,
            volume_id,
            ordered_chapter_ids,
        )?;
        let _ = self.record_workspace_change_set_history(&change_set, "reorder_chapters");
        Ok(true)
    }

    /// 跨卷移动章节：把 `chapter_id` 从 `source_volume_id` 移到 `target_volume_id`
    /// 的 `target_index` 位置（越界钳制到末尾）。
    ///
    /// Issue #1051：卷内重排只覆盖「同一卷的全量 ID 顺序」，跨卷移动必须一次原子
    /// Core 更新完成目录迁移和两卷 order 重排，不能拿两个卷的 reorder 伪造。
    pub fn move_chapter_to_volume(
        &self,
        project_id: &str,
        source_volume_id: &str,
        target_volume_id: &str,
        chapter_id: &str,
        target_index: u32,
    ) -> ApiResult<MoveChapterOutcome> {
        // 章节层在触碰 live 文件之前已写入 Pending move journal；文件事务
        // 提交成功后才返回 (change_set, journal token)。history 未成功前
        // 不能清 journal。
        let (change_set, journal_token) = self.core_write().move_chapter_to_volume_with_changes(
            project_id,
            source_volume_id,
            target_volume_id,
            chapter_id,
            target_index as usize,
        )?;
        // 搜索索引 object_id 含 volume_id：先删旧卷三份条目，再按新卷补写。
        for prefix in ["chapter_title", "chapter_body", "chapter_note"] {
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Delete,
                object_id: format!(
                    "{}:{}:{}:{}",
                    prefix, project_id, source_volume_id, chapter_id
                ),
                scope: crate::search::SearchScope::All,
                title: String::new(),
                body: String::new(),
                target: None,
            });
        }
        if let Ok(content) = self
            .core_read()
            .open_chapter(project_id, target_volume_id, chapter_id)
        {
            let title = content.meta.title.clone();
            let title_entry = crate::search::extractor::extract_chapter_title_entry(
                project_id,
                target_volume_id,
                chapter_id,
                &title,
            );
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Upsert,
                object_id: title_entry.object_id.clone(),
                scope: title_entry.scope,
                title: title_entry.title.clone(),
                body: title_entry.body.clone(),
                target: Some(title_entry.target.clone()),
            });
            if !content.content.is_empty() {
                let body_entry = crate::search::extractor::extract_chapter_body_entry(
                    project_id,
                    target_volume_id,
                    chapter_id,
                    &title,
                    &content.content,
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
            if let Some(ref note) = content.meta.note {
                if !note.is_empty() {
                    let note_entry = crate::search::extractor::extract_chapter_note_entry(
                        project_id,
                        target_volume_id,
                        chapter_id,
                        &title,
                        note,
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
        }
        // 文件事务已提交，磁盘上的章节已经属于目标卷。从这一刻起，即使
        // journal 推进或 workspace history 失败，也不能向平台返回“移动失败”：
        // 否则编辑器会继续绑定旧卷，形成磁盘与身份分裂。返回结构化
        // `history_pending`，由 journal 在下次启动补齐账本。
        let outcome = |history_pending: bool| MoveChapterOutcome {
            moved: true,
            history_pending,
            journal_token: journal_token.clone(),
        };
        if let Err(e) = crate::storage::journal::workspace_change::mark_local_applied_by_token(
            &self.app_data_root,
            &journal_token,
        ) {
            log::warn!(
                "move_chapter_to_volume: mark_local_applied failed for journal {journal_token}: \
                 {e} — files committed, history pending, journal retained"
            );
            return Ok(outcome(true));
        }
        if let Err(e) =
            self.record_workspace_change_set_history(&change_set, "move_chapter_to_volume")
        {
            log::warn!(
                "move_chapter_to_volume: record_workspace_change_set_history failed for \
                 journal {journal_token}: {e} — files committed, journal retained for recovery"
            );
            return Ok(outcome(true));
        }
        // history 成功后才允许 ack + 清理。ack 失败时 journal 仍停留
        // LocalApplied；重启会重新 stage 同一 change set（index 无差异时
        // 返回 oid=None，不会重复提交），因此同样按 history pending 报告。
        if let Err(e) = crate::storage::journal::workspace_change::mark_history_recorded_by_token(
            &self.app_data_root,
            &journal_token,
        ) {
            log::warn!(
                "move_chapter_to_volume: mark_history_recorded failed for journal \
                 {journal_token}: {e} — journal retained"
            );
            return Ok(outcome(true));
        }
        if let Err(e) = crate::storage::journal::workspace_change::clear_journal_by_token(
            &self.app_data_root,
            &journal_token,
        ) {
            log::warn!(
                "move_chapter_to_volume: clear_journal failed for journal {journal_token}: {e}"
            );
        }
        Ok(outcome(false))
    }

    /// 打开章节，返回正文内容和元数据。
    pub fn open_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<ChapterContentDto> {
        let workspace_lock = crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)
            .map_err(WriterError::from)?;
        self.core_write()
            .open_chapter_recovering(project_id, volume_id, chapter_id, &workspace_lock)
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 只读章节当前持久化版本，供编辑器在同步后先探版本再决定是否重载。
    pub fn read_chapter_version(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<ChapterVersionDto> {
        let workspace_lock = crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)
            .map_err(WriterError::from)?;
        self.core_write()
            .read_chapter_version_recovering(project_id, volume_id, chapter_id, &workspace_lock)
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
        self.save_chapter_content_internal(
            project_id,
            volume_id,
            chapter_id,
            content,
            false,
            false,
            None,
            None,
            &[],
            None,
            &[],
        )
        .map(|(dto, _)| dto)
    }

    /// 保存章节正文并核对调用方 base 版本。
    ///
    /// `expected_version` 与磁盘当前版本不一致时返回
    /// `CHAPTER_VERSION_CONFLICT`；`base_content` 只用于冲突材料，正文比较
    /// 始终以磁盘当前内容为准。旧客户端可继续使用无版本入口。
    pub fn save_chapter_content_versioned(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.save_chapter_content_internal(
            project_id,
            volume_id,
            chapter_id,
            content,
            false,
            false,
            expected_version,
            base_content.as_deref(),
            &[],
            None,
            &[],
        )
        .map(|(dto, _)| dto)
    }

    /// 保存正文并携带编辑器冻结的快照 revision。Core 只取该 revision 之前的
    /// 证明事件批次，校验批次终点与正文一致后才提交。
    pub fn save_chapter_content_versioned_with_snapshot(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
        proof_snapshot_revision: u64,
        proof_event_ids: &[String],
        allow_empty_overwrite: bool,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.save_chapter_content_internal(
            project_id,
            volume_id,
            chapter_id,
            content,
            allow_empty_overwrite,
            false,
            expected_version,
            base_content.as_deref(),
            &[],
            Some(proof_snapshot_revision),
            proof_event_ids,
        )
        .map(|(dto, _)| dto)
    }

    /// 保存正文并允许显式清空（编辑器删除全部文字）。版本 CAS 仍必须完整。
    pub fn save_chapter_content_versioned_allow_empty(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.save_chapter_content_internal(
            project_id,
            volume_id,
            chapter_id,
            content,
            true,
            false,
            expected_version,
            base_content.as_deref(),
            &[],
            None,
            &[],
        )
        .map(|(dto, _)| dto)
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
        self.save_chapter_content_internal(
            project_id,
            volume_id,
            chapter_id,
            content,
            allow_empty_overwrite,
            false,
            None,
            None,
            &[],
            None,
            &[],
        )
        .map(|(dto, _)| dto)
    }

    /// 清空章节正文（等价于 `save_chapter_content_with_options(..., true)`）。
    pub fn clear_chapter_content(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.save_chapter_content_internal(
            project_id,
            volume_id,
            chapter_id,
            "",
            true,
            true,
            None,
            None,
            &[],
            None,
            &[],
        )
        .map(|(dto, _)| dto)
    }

    /// 清空章节正文并核对 base 版本。
    pub fn clear_chapter_content_versioned(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
    ) -> ApiResult<ChapterSaveReceiptDto> {
        self.save_chapter_content_internal(
            project_id,
            volume_id,
            chapter_id,
            "",
            true,
            true,
            expected_version,
            base_content.as_deref(),
            &[],
            None,
            &[],
        )
        .map(|(dto, _)| dto)
    }

    /// 保存正文并在同一 journal 决议中登记编辑事件投影。
    ///
    /// 返回的 receipt 带 `transaction_id` / `pending_projections`；调用方逐条
    /// 记录事件后调用 [`crate::chapter::commit::mark_chapter_commit_projection`]。
    pub fn save_chapter_with_projection_ids(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        projection_ids: Vec<String>,
    ) -> ApiResult<(ChapterSaveReceiptDto, crate::chapter::ChapterSaveReceipt)> {
        self.save_chapter_content_internal(
            project_id,
            volume_id,
            chapter_id,
            content,
            false,
            false,
            None,
            None,
            &projection_ids,
            None,
            &[],
        )
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn save_chapter_content_internal(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        allow_empty_overwrite: bool,
        clear_content: bool,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<&str>,
        projection_ids: &[String],
        proof_snapshot_revision: Option<u64>,
        proof_event_ids: &[String],
    ) -> ApiResult<(ChapterSaveReceiptDto, crate::chapter::ChapterSaveReceipt)> {
        // 锁顺序固定为 workspace → Core → store；不能在持有 Core 写锁后再申请
        // workspace，否则与同步提交线程形成 ABBA。
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
            .save_chapter_verified_with_changes_versioned(
                project_id,
                volume_id,
                chapter_id,
                content,
                allow_empty_overwrite,
                &additional_files,
                expected_version,
                base_content,
                projection_ids,
                Some(&workspace_lock),
                proof_snapshot_revision,
                proof_event_ids,
            )
            .map_err(WriterError::from)?;

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
                &change_set,
                if clear_content {
                    "clear_chapter_content"
                } else {
                    "save_chapter_content"
                },
            );
        }
        Ok((receipt.clone().into(), receipt))
    }

    /// 更新章节备注（note 字段，独立于正文）。
    pub fn update_chapter_note(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        note: &str,
    ) -> ApiResult<bool> {
        //   用 _with_changes 版本拿变更集。
        let (chapter, change_set) = self
            .core_write()
            .update_chapter_note_with_changes(project_id, volume_id, chapter_id, note)?;
        let ch_title = chapter.title;
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
        let _ = self.record_workspace_change_set_history(&change_set, "update_chapter_note");
        Ok(true)
    }
}
