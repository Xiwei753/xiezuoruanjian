// =============================================================================
// chapter_save_queue/draft_jobs.rs — 草稿事实、草稿正文与项目恢复命令
// =============================================================================
//
// Issue #1062 1.1：编辑事实、编辑器正文变化与“项目进入时恢复草稿”都走
// 同一个章节 actor（chapter_save_queue 的唯一 worker），与保存命令按入队
// 顺序串行执行，因此保存看到的事实顺序与编辑器产生顺序一致。
//
// 这里不保存业务状态：事实的最终校验由 Core 的显式批次 replay 完成，
// 本模块只负责把平台冻结的原始投影写进可恢复草稿（chapter_draft）。
// =============================================================================

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::{worker_sender, ChapterSaveContext, ChapterSaveJob, ChapterSaveWorkerState};

/// 一条待持久化的编辑事实。
pub(crate) struct ChapterFactCommand {
    pub context: ChapterSaveContext,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub input: writer_core::api::EditorChangeStatsInputDto,
}

/// 编辑器正文变化命令。
pub(crate) struct ChapterDraftTextCommand {
    pub context: ChapterSaveContext,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub text: Arc<str>,
    pub revision: u64,
    pub sha256: String,
    pub base_generation: String,
    pub base_revision: u64,
    pub base_sha256: String,
    /// true = 立即写整章正文（保存/关窗/离开作品边界）。
    pub force: bool,
}

/// 项目进入时重放草稿的请求。
pub(crate) struct ChapterRestoreCommand {
    pub context: ChapterSaveContext,
    pub project_id: String,
}

/// 草稿恢复的进程内状态，供 QML 轮询（不跨线程调用 QObject）。
#[derive(Clone, Debug, Default)]
pub(crate) struct ChapterDraftRestoreStatus {
    pub pending: bool,
    pub restored: usize,
    pub failed: usize,
    pub kept: usize,
    pub message: String,
}

static DRAFT_RESTORE_STATE: OnceLock<Mutex<HashMap<String, ChapterDraftRestoreStatus>>> =
    OnceLock::new();

fn draft_restore_state() -> &'static Mutex<HashMap<String, ChapterDraftRestoreStatus>> {
    DRAFT_RESTORE_STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 读取项目草稿恢复状态（QML 轮询）。
pub(crate) fn draft_restore_status(project_id: &str) -> ChapterDraftRestoreStatus {
    draft_restore_state()
        .lock()
        .ok()
        .and_then(|states| states.get(project_id).cloned())
        .unwrap_or_default()
}

/// UI 线程：把编辑事实交给 actor（立刻返回，不写盘、不调用 Core）。
pub(crate) fn enqueue_fact(command: ChapterFactCommand) -> Result<(), String> {
    worker_sender()
        .send(ChapterSaveJob::RecordFact(command))
        .map_err(|_| "chapter save queue worker has exited".to_string())
}

/// UI 线程：更新草稿正文并把落盘交给 actor。
pub(crate) fn enqueue_draft_text(command: ChapterDraftTextCommand) -> Result<(), String> {
    worker_sender()
        .send(ChapterSaveJob::DraftText(command))
        .map_err(|_| "chapter save queue worker has exited".to_string())
}

/// UI 线程：项目进入时请求重放草稿；返回待处理草稿数。
pub(crate) fn enqueue_restore_project(command: ChapterRestoreCommand) -> usize {
    let drafts = super::super::chapter_draft::list_project_drafts(
        Path::new(&command.context.projects_root),
        &command.project_id,
    );
    let needing: Vec<&super::super::chapter_draft::ChapterDraft> = drafts
        .iter()
        .filter(|draft| !draft.facts.is_empty() || draft.text.is_some())
        .collect();
    let count = needing.len();
    if count == 0 {
        return 0;
    }
    if let Ok(mut states) = draft_restore_state().lock() {
        states.insert(
            command.project_id.clone(),
            ChapterDraftRestoreStatus {
                pending: true,
                ..ChapterDraftRestoreStatus::default()
            },
        );
    }
    let project_id = command.project_id.clone();
    if worker_sender()
        .send(ChapterSaveJob::RestoreProject(command))
        .is_err()
    {
        if let Ok(mut states) = draft_restore_state().lock() {
            states.insert(
                project_id,
                ChapterDraftRestoreStatus {
                    pending: false,
                    message: "chapter save queue worker has exited".to_string(),
                    ..ChapterDraftRestoreStatus::default()
                },
            );
        }
    }
    count
}

/// 记录一条编辑事实：构造原始证明投影并写进可恢复草稿。
pub(super) fn handle_record_fact(state: &mut ChapterSaveWorkerState, command: ChapterFactCommand) {
    let event_id = command.input.event_id.clone();
    let revision = command.input.new_revision;
    let api = state.api_for(&command.context);
    let projection_json = match api.record_editor_proof_fact(command.input) {
        Ok(json) => json,
        Err(error) => {
            crate::backend::app_backend::debug_error_static(
                "chapter_draft",
                "record_editor_proof_fact_failed",
                &format!(
                    "project={} chapter={} event={event_id}: {error}",
                    command.project_id, command.chapter_id
                ),
            );
            return;
        }
    };
    if projection_json.is_empty() {
        return;
    }
    super::super::chapter_draft::note_fact(
        &command.project_id,
        &command.chapter_id,
        &command.volume_id,
        super::super::chapter_draft::DraftFact {
            event_id,
            revision,
            projection_json,
        },
    );
    if let Err(error) = super::super::chapter_draft::persist_current(
        Path::new(&command.context.projects_root),
        &command.project_id,
        &command.chapter_id,
        false,
    ) {
        crate::backend::app_backend::debug_error_static(
            "chapter_draft",
            "persist_edit_fact_failed",
            &format!(
                "project={} chapter={}: {error}",
                command.project_id, command.chapter_id
            ),
        );
    }
}

/// 更新草稿正文；`force` 时立即写整章。
pub(super) fn handle_draft_text(command: ChapterDraftTextCommand) {
    super::super::chapter_draft::note_text(
        &command.project_id,
        &command.chapter_id,
        &command.volume_id,
        command.text.to_string(),
        command.revision,
        &command.sha256,
        (
            &command.base_generation,
            command.base_revision,
            &command.base_sha256,
        ),
    );
    if let Err(error) = super::super::chapter_draft::persist_current(
        Path::new(&command.context.projects_root),
        &command.project_id,
        &command.chapter_id,
        command.force,
    ) {
        crate::backend::app_backend::debug_error_static(
            "chapter_draft",
            "persist_draft_text_failed",
            &format!(
                "project={} chapter={}: {error}",
                command.project_id, command.chapter_id
            ),
        );
    }
}

/// 项目进入时按同一批次协议重放未完成草稿。
///
/// 先读当前已提交正文作为基线，用原始事实 replay 出最终正文，再以
/// “显式批次 + 当前版本 CAS”提交；失败（外部删除、版本冲突、链不一致）
/// 只保留草稿，不删除任何用户数据。
pub(super) fn handle_restore_project(
    state: &mut ChapterSaveWorkerState,
    command: ChapterRestoreCommand,
) {
    let projects_root = PathBuf::from(&command.context.projects_root);
    let drafts =
        super::super::chapter_draft::list_project_drafts(&projects_root, &command.project_id);
    let mut restored = 0usize;
    let mut failed = 0usize;
    let mut kept = 0usize;
    let mut messages: Vec<String> = Vec::new();
    let api = state.api_for(&command.context);
    for draft in drafts {
        let Some(plan) = super::super::chapter_draft::restore_plan(
            &projects_root,
            &command.project_id,
            &draft.chapter_id,
        ) else {
            continue;
        };
        let opened = match api.open_chapter(&command.project_id, &plan.volume_id, &plan.chapter_id)
        {
            Ok(content) => content,
            Err(error) => {
                failed += 1;
                kept += 1;
                messages.push(format!(
                    "{}: cannot read committed chapter: {error}",
                    plan.chapter_id
                ));
                continue;
            }
        };
        let committed = opened.content;
        let (content, batch_json) = if plan.fact_count > 0 {
            match api.replay_editor_proof_batch(&plan.chapter_id, &committed, &plan.batch_json) {
                Ok(replayed) => (replayed, plan.batch_json),
                Err(error) => {
                    failed += 1;
                    kept += 1;
                    messages.push(format!("{}: replay failed: {error}", plan.chapter_id));
                    continue;
                }
            }
        } else {
            match plan.text {
                Some(text) => (text, String::new()),
                None => continue,
            }
        };
        if content == committed && batch_json.is_empty() {
            super::super::chapter_draft::remove(
                &projects_root,
                &command.project_id,
                &plan.chapter_id,
            );
            continue;
        }
        let result = api.save_chapter_content_versioned_with_proof_batch(
            &command.project_id,
            &plan.volume_id,
            &plan.chapter_id,
            &content,
            Some(opened.version),
            Some(committed),
            0,
            &batch_json,
            true,
        );
        match result {
            Ok(_receipt) => {
                super::super::chapter_draft::remove(
                    &projects_root,
                    &command.project_id,
                    &plan.chapter_id,
                );
                restored += 1;
            }
            Err(error) => {
                failed += 1;
                kept += 1;
                messages.push(format!("{}: save draft failed: {error}", plan.chapter_id));
            }
        }
    }
    if let Ok(mut states) = draft_restore_state().lock() {
        states.insert(
            command.project_id,
            ChapterDraftRestoreStatus {
                pending: false,
                restored,
                failed,
                kept,
                message: messages.join("; "),
            },
        );
    }
}
