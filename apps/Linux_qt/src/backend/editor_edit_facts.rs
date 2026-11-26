use super::*;

use crate::backend::json_utils::qjson_object_from_json;
use qmetaobject::{QJsonObject, QString};

impl AppBackend {
    /// 按编辑事务上报写作统计。
    ///
    /// 平台端只透传编辑事实（cause + inserted/deleted），不自己拼 source、device_id、session_id。
    /// `cause → EventSource` 映射和设备身份管理由 Core 内部完成。
    ///
    /// Issue #843: 不再在 UI 线程同步写盘。只把命令 enqueue 到独立 worker thread，
    /// 避免每次按键的文件系统 I/O 阻塞输入热路径。
    pub(crate) fn record_editor_change_stats(
        &self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        cause: writer_core::editor::EditorTransactionCause,
        inserted_chars: u32,
        deleted_chars: u32,
        changes_json: QString,
        session_id: QString,
        base_revision: u64,
        new_revision: u64,
        old_selection_anchor: u64,
        old_selection_head: u64,
        new_selection_anchor: u64,
        new_selection_head: u64,
        baseline_text: QString,
        before_text_sha256: QString,
        after_text_sha256: QString,
        base_generation: QString,
    ) -> QString {
        // Issue #1062 1.1：编辑事实首次上报就固定稳定身份与发生时间，
        // 统计 writer 与章节 actor 共用同一个 event_id。
        let event_id = uuid::Uuid::new_v4().to_string();
        let occurred_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0);
        // 编辑事实与保存命令进同一个 workspace/chapter actor：actor 先持久化
        // 原始证明投影（可恢复草稿），保存时从自己的连续事实前缀冻结批次，
        // 不再依赖 Core 进程内 pending map 与正文摘要猜批次。
        self.enqueue_chapter_edit_fact(
            &project_id,
            &volume_id,
            &chapter_id,
            cause,
            inserted_chars,
            deleted_chars,
            &changes_json,
            &session_id,
            base_revision,
            new_revision,
            old_selection_anchor,
            old_selection_head,
            new_selection_anchor,
            new_selection_head,
            (&baseline_text, &before_text_sha256, &after_text_sha256),
            &base_generation,
            &event_id,
            occurred_at_ms,
        );
        if let Some(ref writer) = self.stats_writer {
            if let Err(e) =
                writer.enqueue(crate::backend::stats_writer::StatsWriteCommand::Editor {
                    project_id: project_id.to_string(),
                    volume_id: volume_id.to_string(),
                    chapter_id: chapter_id.to_string(),
                    cause,
                    inserted_chars,
                    deleted_chars,
                    changes_json: changes_json.to_string(),
                    session_id: session_id.to_string(),
                    base_revision,
                    new_revision,
                    old_selection_anchor,
                    old_selection_head,
                    new_selection_anchor,
                    new_selection_head,
                    baseline_text: if baseline_text.is_empty() {
                        None
                    } else {
                        Some(baseline_text.to_string())
                    },
                    before_text_sha256: before_text_sha256.to_string(),
                    after_text_sha256: after_text_sha256.to_string(),
                    event_id: event_id.clone(),
                    occurred_at_ms,
                })
            {
                // 统计写入失败不否定编辑事实：原始证明批次已经进章节 actor 的
                // 草稿，保存时按事实提交；这里只丢统计，不丢正文/证明。
            }
        }
        event_id.into()
    }

    /// 把一条编辑事实交给 workspace/chapter actor（Issue #1062 1.1）。
    ///
    /// actor 用 Core 构造原始证明投影并持久化为可恢复草稿；保存命令只从
    /// actor 自己的连续事实前缀冻结批次，不再用 QML 侧 event ID 去 Core
    /// 全局 pending map 取证明。
    #[allow(clippy::too_many_arguments)]
    fn enqueue_chapter_edit_fact(
        &self,
        project_id: &QString,
        volume_id: &QString,
        chapter_id: &QString,
        cause: writer_core::editor::EditorTransactionCause,
        inserted_chars: u32,
        deleted_chars: u32,
        changes_json: &QString,
        session_id: &QString,
        base_revision: u64,
        new_revision: u64,
        old_selection_anchor: u64,
        old_selection_head: u64,
        new_selection_anchor: u64,
        new_selection_head: u64,
        digests: (&QString, &QString, &QString),
        generation: &QString,
        event_id: &str,
        occurred_at_ms: i64,
    ) {
        let changes_json_string = changes_json.to_string();
        if changes_json_string.trim().is_empty() || changes_json_string.trim() == "[]" {
            // 老客户端只报计数、没有逐字变更：不能生成可回放证明。
            return;
        }
        let Ok(changes) =
            serde_json::from_str::<Vec<writer_core::api::EditorChangeDto>>(&changes_json_string)
        else {
            self.debug_error(
                "chapter_draft",
                "editor_changes_json_invalid",
                "cannot parse changes for chapter edit fact",
            );
            return;
        };
        if changes.is_empty() {
            return;
        }
        let project = project_id.to_string();
        let volume = volume_id.to_string();
        let chapter = chapter_id.to_string();
        if project.is_empty() || chapter.is_empty() {
            return;
        }
        let Some((data_root, projects_root, layout)) = self.annotation_task_context() else {
            return;
        };
        let (baseline, before_sha, after_sha) = digests;
        let baseline_text = {
            let text = baseline.to_string();
            (!text.is_empty()).then_some(text)
        };
        let input = writer_core::api::EditorChangeStatsInputDto {
            platform: "linux".to_string(),
            project_id: project,
            volume_id: volume,
            chapter_id: chapter,
            cause: cause.into(),
            inserted_chars,
            deleted_chars,
            event_id: event_id.to_string(),
            occurred_at_ms,
            changes,
            session_id: session_id.to_string(),
            baseline_text,
            base_revision,
            new_revision,
            old_selection_anchor: old_selection_anchor as u32,
            old_selection_head: old_selection_head as u32,
            new_selection_anchor: new_selection_anchor as u32,
            new_selection_head: new_selection_head as u32,
            sync_source_device_id: String::new(),
            parent_event_id: String::new(),
            before_text_sha256: before_sha.to_string(),
            after_text_sha256: after_sha.to_string(),
        };
        let project = input.project_id.clone();
        let volume = input.volume_id.clone();
        let chapter = input.chapter_id.clone();
        let command = crate::backend::chapter_save_queue::ChapterFactCommand {
            context: crate::backend::chapter_save_queue::ChapterSaveContext::new(
                &data_root,
                &projects_root,
                &layout,
            ),
            project_id: project,
            volume_id: volume,
            chapter_id: chapter,
            session_id: session_id.to_string(),
            generation: generation.to_string(),
            input,
        };
        if let Err(error) = crate::backend::chapter_save_queue::enqueue_fact(command) {
            self.debug_error("chapter_draft", "enqueue_edit_fact_failed", &error);
        }
    }

    /// 把编辑器正文变化交给章节 actor 更新草稿（Issue #1062 1.3）。
    ///
    /// `force=true` 用于保存、切章、离开作品和关窗等边界：立即写整章正文，
    /// 不依赖 2 秒节流。普通编辑走节流写入，避免每 300ms 重写整章。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_chapter_draft_text(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        session_id: u64,
        text: &str,
        revision: u64,
        sha256: &str,
        base_generation: &str,
        base_revision: u64,
        base_sha256: &str,
        force: bool,
    ) {
        if project_id.is_empty() || chapter_id.is_empty() {
            return;
        }
        let Some((data_root, projects_root, layout)) = self.annotation_task_context() else {
            return;
        };
        let command = crate::backend::chapter_save_queue::ChapterDraftTextCommand {
            context: crate::backend::chapter_save_queue::ChapterSaveContext::new(
                &data_root,
                &projects_root,
                &layout,
            ),
            project_id: project_id.to_string(),
            volume_id: volume_id.to_string(),
            chapter_id: chapter_id.to_string(),
            session_id,
            generation: base_generation.to_string(),
            text: std::sync::Arc::from(text),
            revision,
            sha256: sha256.to_string(),
            base_generation: base_generation.to_string(),
            base_revision,
            base_sha256: base_sha256.to_string(),
            force,
        };
        if let Err(error) = crate::backend::chapter_save_queue::enqueue_draft_text(command) {
            self.debug_error("chapter_draft", "enqueue_draft_text_failed", &error);
        }
    }

    /// 项目进入时请求重放未完成草稿；返回待处理数量（0 = 无需等待）。
    pub(crate) fn restore_project_chapter_drafts(&self, project_id: &str) -> u32 {
        if project_id.is_empty() {
            return 0;
        }
        let Some((data_root, projects_root, layout)) = self.annotation_task_context() else {
            return 0;
        };
        let command = crate::backend::chapter_save_queue::ChapterRestoreCommand {
            context: crate::backend::chapter_save_queue::ChapterSaveContext::new(
                &data_root,
                &projects_root,
                &layout,
            ),
            project_id: project_id.to_string(),
        };
        u32::try_from(crate::backend::chapter_save_queue::enqueue_restore_project(
            command,
        ))
        .unwrap_or(u32::MAX)
    }

    /// 草稿恢复状态（QML 轮询；不跨线程调 QObject）。
    pub(crate) fn chapter_draft_restore_status(&self, project_id: &str) -> QJsonObject {
        let status = crate::backend::chapter_save_queue::draft_restore_status(project_id);
        qjson_object_from_json(
            &serde_json::json!({
                "pending": status.pending,
                "restored": status.restored,
                "failed": status.failed,
                "kept": status.kept,
                "message": status.message,
            })
            .to_string(),
        )
    }

    /// 当前章节的草稿快照（只读，供恢复诊断/提示）。
    pub(crate) fn chapter_draft_snapshot(&self, project_id: &str, chapter_id: &str) -> QJsonObject {
        if project_id.is_empty() || chapter_id.is_empty() {
            return QJsonObject::default();
        }
        let Some(projects_root) = self
            .annotation_task_context()
            .map(|(_, projects_root, _)| projects_root)
        else {
            return QJsonObject::default();
        };
        match crate::backend::chapter_draft::snapshot(
            std::path::Path::new(&projects_root),
            project_id,
            chapter_id,
        ) {
            Some(snapshot) => qjson_object_from_json(
                &serde_json::json!({
                    "found": true,
                    "projectId": snapshot.project_id,
                    "volumeId": snapshot.volume_id,
                    "chapterId": snapshot.chapter_id,
                    "baseGeneration": snapshot.base_generation,
                    "baseRevision": snapshot.base_revision,
                    "baseSha256": snapshot.base_sha256,
                    "text": snapshot.text,
                    "textRevision": snapshot.text_revision,
                    "textSha256": snapshot.text_sha256,
                    "factCount": snapshot.fact_count,
                    "updatedAtMs": snapshot.updated_at_ms,
                })
                .to_string(),
            ),
            None => qjson_object_from_json("{\"found\":false}"),
        }
    }
}
