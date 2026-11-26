// =============================================================================
// chapter_save_queue/draft_jobs.rs — 草稿事实、草稿正文与项目恢复命令
// =============================================================================
//
// Issue #1062 1.1：编辑事实、编辑器正文变化与“项目进入时恢复草稿”都走
// 同一个章节 actor（chapter_save_queue 的唯一 worker），与保存命令按入队
// 顺序串行执行，因此保存看到的事实顺序与编辑器产生顺序一致。
//
// 这里不保存业务状态：事实的最终校验由 Core 的显式批次 replay 完成，
// 本模块只负责把平台冻结的原始投影写进可恢复草稿（chapter_draft），并在
// 恢复时用**草稿记录的原始 base**校验，绝不拿刚读到的磁盘版本覆盖外部更新。
// =============================================================================

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use writer_core::api::types::ChapterVersionDto;
use writer_core::api::WriterCoreApi;

use super::{
    take_draft_text_snapshot, worker_sender, ChapterSaveContext, ChapterSaveJob,
    ChapterSaveWorkerState,
};

/// 一条待持久化的编辑事实。
pub(crate) struct ChapterFactCommand {
    pub context: ChapterSaveContext,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    /// 编辑器会话（十进制字符串；解析失败按 0 处理，不阻断保存）。
    pub session_id: String,
    /// 当前对象 generation；草稿 generation 为空时冻结视为任意 generation。
    pub generation: String,
    /// 编辑器本次载入的持久化章节版本（事件 spool 的 base，供重启恢复校验）。
    pub base_generation: String,
    pub base_revision: u64,
    pub base_sha256: String,
    pub input: writer_core::api::EditorChangeStatsInputDto,
}

/// 编辑器正文变化命令（Issue #1062 三.4 / 评论 7191076411 第 2 项：只带身份）。
///
/// 正文、revision/sha256 与 base 不再随命令进入无界 channel，也不由 GUI 线程
/// 直接改写草稿 registry：GUI 把它们放进 [`super::DRAFT_TEXT_MAILBOX`] 的最新
/// 快照，worker 处理到本命令时在 actor 线程按顺序写入草稿；`force` 表示保存/
/// 关窗/离开作品边界的立即写入。
pub(crate) struct ChapterDraftTextCommand {
    pub context: ChapterSaveContext,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    /// true = 立即写整章正文（保存/关窗/离开作品边界）。
    pub force: bool,
    /// 本次文档加载时的编辑器 revision（编辑器坐标，不是章节持久化 revision）。
    pub editor_base_revision: u64,
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

pub(super) fn draft_restore_state() -> &'static Mutex<HashMap<String, ChapterDraftRestoreStatus>> {
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

/// UI 线程：项目进入时请求重放草稿；返回待处理草稿数。
///
/// Issue #1062 评论 7191076411 第 1 项（平台侧配合）：即使本地没有草稿也
/// 必须入队一次 RestoreProject，让 worker 对账 Core journal 里未确认的草稿
/// ack；`pending` 标志只在确实有草稿/待处理内容时置 true，避免 QML 无谓等待。
pub(crate) fn enqueue_restore_project(command: ChapterRestoreCommand) -> usize {
    let drafts = super::super::chapter_draft::list_project_drafts(
        Path::new(&command.context.projects_root),
        &command.project_id,
    );
    // pending-inputs 也是未提交事件：只有 spool、尚无 facts/text 的草稿
    // （首次事件在投影/正文写出前崩溃）同样必须进入恢复队列。
    let count = drafts
        .iter()
        .filter(|draft| {
            draft.fact_count > 0
                || draft.text_present
                || draft.pending_input_count > 0
                || draft.pending_input_error.is_some()
                || draft.draft_error.is_some()
        })
        .count();
    if count > 0 {
        if let Ok(mut states) = draft_restore_state().lock() {
            states.insert(
                command.project_id.clone(),
                ChapterDraftRestoreStatus {
                    pending: true,
                    ..ChapterDraftRestoreStatus::default()
                },
            );
        }
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

/// 记录一条编辑事实：先持久化原始命令（accepted），再构造证明投影。
///
/// Issue #1062 复审第 3 项：原始命令必须在构造投影前落盘，并先写出 meta
/// 事务头；这样首次事件在 meta 写出前崩溃、投影构造失败或事实文件写盘失败
/// 都能在重启后按 spool 恢复，不会退化成“没有待提交证明”。
pub(super) fn handle_record_fact(state: &mut ChapterSaveWorkerState, command: ChapterFactCommand) {
    let event_id = command.input.event_id.clone();
    let revision = command.input.new_revision;
    let session_id = command.session_id.parse::<u64>().unwrap_or(0);
    let projects_root = PathBuf::from(&command.context.projects_root);
    let input_json = match serde_json::to_string(&command.input) {
        Ok(json) => json,
        Err(error) => {
            crate::backend::app_backend::debug_error_static(
                "chapter_draft",
                "record_editor_proof_fact_encode_failed",
                &format!(
                    "project={} chapter={} event={event_id}: {error}",
                    command.project_id, command.chapter_id
                ),
            );
            return;
        }
    };
    let base = (
        command.base_generation.as_str(),
        command.base_revision,
        command.base_sha256.as_str(),
    );
    if let Err(error) = super::super::chapter_draft::spool_failed_input(
        &projects_root,
        &command.project_id,
        &command.chapter_id,
        &command.volume_id,
        session_id,
        &command.generation,
        revision,
        &event_id,
        &input_json,
        base,
    ) {
        // spool 失败时仍尝试投影 + 事实文件；真正写盘失败会进入内存失败集合
        // 并阻塞冻结，不会静默通过保存。
        crate::backend::app_backend::debug_error_static(
            "chapter_draft",
            "record_editor_proof_fact_spool_failed",
            &format!(
                "project={} chapter={} event={event_id}: {error}",
                command.project_id, command.chapter_id
            ),
        );
    }
    let api = state.api_for(&command.context);
    let projection_json = match api.record_editor_proof_fact(command.input) {
        Ok(json) => json,
        Err(error) => {
            crate::backend::app_backend::debug_error_static(
                "chapter_draft",
                "record_editor_proof_fact_deferred",
                &format!(
                    "project={} chapter={} event={event_id} revision={revision}: {error}",
                    command.project_id, command.chapter_id
                ),
            );
            return;
        }
    };
    if projection_json.is_empty() {
        // 合法无证明事件（例如 Load/Format 不产生逐字证明）：清除 spool，
        // 但仍先确保 meta 事务头存在，便于恢复列表发现草稿。
        super::super::chapter_draft::clear_pending_input(
            &projects_root,
            &command.project_id,
            &command.chapter_id,
            &event_id,
        );
        return;
    }
    if let Err(error) = super::super::chapter_draft::note_fact(
        &projects_root,
        &command.project_id,
        &command.chapter_id,
        &command.volume_id,
        session_id,
        &command.generation,
        super::super::chapter_draft::DraftFact {
            event_id: event_id.clone(),
            revision,
            session_id,
            projection_json,
        },
    ) {
        crate::backend::app_backend::debug_error_static(
            "chapter_draft",
            "note_fact_failed",
            &format!(
                "project={} chapter={} event={event_id}: {error}; spool kept",
                command.project_id, command.chapter_id
            ),
        );
        return;
    }
    // 事实文件与索引都成功后才确认 accepted；崩溃留下的 spool 会按同一
    // event ID 幂等重放（Core 用 event_id 判重，note_fact 也按 ID 去重）。
    super::super::chapter_draft::clear_pending_input(
        &projects_root,
        &command.project_id,
        &command.chapter_id,
        &event_id,
    );
    if let Err(error) = super::super::chapter_draft::persist_current(
        &projects_root,
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

/// 用同一 event ID 重试所有 Core 投影构造失败的原始命令。/// 用同一 event ID 重试所有 Core 投影构造失败的原始命令。
///
/// 成功一条就写进草稿并删除 spool 文件；任何一条失败立即返回 Err，
/// 由调用方拒绝越过该 revision 的保存/恢复（Issue #1062 复审第 3 项）。
pub(super) fn retry_pending_inputs(
    api: &WriterCoreApi,
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    session_id: u64,
    generation: &str,
) -> Result<usize, String> {
    let mut retried = 0usize;
    for input in super::super::chapter_draft::pending_inputs(projects_root, project_id, chapter_id)?
    {
        let dto: writer_core::api::EditorChangeStatsInputDto =
            serde_json::from_str(&input.input_json).map_err(|error| {
                format!(
                    "pending proof input decode failed: event={} revision={}: {error}",
                    input.event_id, input.revision
                )
            })?;
        let projection = api.record_editor_proof_fact(dto).map_err(|error| {
            format!(
                "record pending proof fact event={}: {error}",
                input.event_id
            )
        })?;
        // spooled 输入自带原始会话/generation；调用方参数只在旧记录缺字段时兜底。
        let fact_session = if input.session_id == 0 {
            session_id
        } else {
            input.session_id
        };
        let fact_volume = if input.volume_id.is_empty() {
            volume_id
        } else {
            input.volume_id.as_str()
        };
        let fact_generation = if input.generation.is_empty() {
            generation
        } else {
            input.generation.as_str()
        };
        if !projection.is_empty() {
            super::super::chapter_draft::note_fact(
                projects_root,
                project_id,
                chapter_id,
                fact_volume,
                fact_session,
                fact_generation,
                super::super::chapter_draft::DraftFact {
                    event_id: input.event_id.clone(),
                    revision: input.revision,
                    session_id: fact_session,
                    projection_json: projection,
                },
            )?;
        }
        super::super::chapter_draft::clear_pending_input(
            projects_root,
            project_id,
            chapter_id,
            &input.event_id,
        );
        retried = retried.saturating_add(1);
    }
    Ok(retried)
}

/// actor 线程：把 mailbox 里该章最新的正文快照写进草稿 registry，再交给
/// `persist_current` 按证明水位与节流决定是否落盘。
///
/// Issue #1062 评论 7191076411 第 2 项：正文不再由 GUI 线程直接改 registry。
/// 命令按 actor 顺序到达，快照始终是该章最新一份；没有快照（已被更早命令
/// 取走）时只做一次节流落盘。
pub(super) fn handle_draft_text(command: ChapterDraftTextCommand) {
    let projects_root = Path::new(&command.context.projects_root);
    let mut force = command.force;
    if let Some(snapshot) =
        take_draft_text_snapshot(&command.context, &command.project_id, &command.chapter_id)
    {
        force |= snapshot.force;
        // base 只在草稿状态未初始化时生效；之后由 Core durable receipt /
        // pending-ack 推进（见 chapter_draft::note_text）。
        super::super::chapter_draft::note_text(
            projects_root,
            &command.project_id,
            &command.chapter_id,
            &command.volume_id,
            snapshot.session_id,
            &snapshot.generation,
            snapshot.text.to_string(),
            snapshot.revision,
            &snapshot.sha256,
            (
                &snapshot.base_generation,
                snapshot.base_revision,
                &snapshot.base_sha256,
            ),
            snapshot.editor_base_revision,
        );
        log::debug!(
            "chapter draft text snapshot applied: project={} chapter={} revision={} wait_ms={}",
            command.project_id,
            command.chapter_id,
            snapshot.revision,
            snapshot.updated_at.elapsed().as_millis()
        );
    }
    if let Err(error) = super::super::chapter_draft::persist_current(
        projects_root,
        &command.project_id,
        &command.chapter_id,
        force,
    ) {
        log::warn!(
            "persist chapter draft text failed: project={} chapter={}: {error}",
            command.project_id,
            command.chapter_id
        );
    }
}
