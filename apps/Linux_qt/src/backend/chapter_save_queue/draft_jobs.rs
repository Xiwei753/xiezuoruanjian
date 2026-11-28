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
    /// actor 是否仍在执行本轮恢复；与 `pending` 一起区分“进行中/已结束”。
    pub running: bool,
    /// 本轮结束后磁盘上仍未决的恢复候选数（活动草稿 + 不可读原件）。
    pub remaining: usize,
    /// 仍在归档区等待下次自动重试的事务数。
    pub deferred: usize,
    /// 明确冲突/无法隔离、需要人工处理的章节数。
    pub conflicted: usize,
    /// 发现扫描是否完整（Issue #1062 评论 #7214283385 第 3 项）。
    /// 不完整时项目级恢复未决：不得把“没发现”当成“没有草稿”放行写入。
    pub scan_incomplete: bool,
    pub pending: bool,
    pub restored: usize,
    pub failed: usize,
    pub kept: usize,
    /// 已搬进独立归档区（`chapter-draft-kept/`）的保留草稿章节 id。
    ///
    /// Issue #1062 评论 7194373427 第 1 项：这些章节本次不自动恢复，编辑器
    /// 新会话会使用全新目录，不再覆盖旧草稿。
    pub kept_chapters: Vec<String>,
    /// 需要人处理/已外部解决、已永久归档的章节。
    pub archived_chapters: Vec<String>,
    /// 已归档且登记了自动重试（下次进入作品时搬回活动槽位重试）的章节。
    pub retry_chapters: Vec<String>,
    /// 归档失败、原目录仍在可写槽位的章节。QML 必须在打开这些章节之前
    /// 拒绝进入编辑器，否则新会话会覆写未能隔离的旧事务。
    pub blocked_chapters: Vec<String>,
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
    // Issue #1062 评论 #7213634487 第 A 项：UI 等待数与恢复 actor 用同一份
    // 严格发现结果；损坏的旧格式原件也计入待处理，不能被当成“没有草稿”。
    let (draft_count, discovery_failed, scan_errors) =
        match super::super::chapter_draft::discover_project_drafts(
            Path::new(&command.context.projects_root),
            &command.project_id,
        ) {
            Ok(discovery) => (
                discovery.drafts.len() + discovery.unreadable.len(),
                0,
                discovery.scan_errors.len(),
            ),
            Err(error) => {
                log::warn!("chapter draft discovery failed: {error}");
                (0, 1, 0)
            }
        };
    // Issue #1062 评论 7195690539 第 2 项：发现到的每个草稿目录都必须等
    // 恢复 actor 给出“恢复/归档”决议后才允许编辑器打开，不能因为摘要里
    // fact/text 都为空（空事务头、缺失 blob 的候选）就放行。pending-inputs
    // 同样计入。
    let count = draft_count
        // Issue #1062 评论 7195690539 第 1 项：可自动重试的事务在进入项目时
        // 必须先搬回活动槽位再重试，同样要等待恢复 actor 完成决议。
        + super::super::chapter_draft::retry_chapters(
            Path::new(&command.context.projects_root),
            &command.project_id,
        )
        .len()
        + discovery_failed
        // Issue #1062 评论 #7214283385 第 3 项：扫描不完整时“没发现”不能当成
        // “没有草稿”，必须等 actor 用一次完整发现给出项目级决议。
        + scan_errors;
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
    // Issue #1069：新建草稿的 base 以 worker 已确认回执为准；只有输入事实的
    // before 摘要确实对应回执正文时才算归属，否则保留 QML 记录的旧 base，
    // 由恢复链按真实外部冲突处理，不推测重基线。
    let chain_receipt = state.chain_receipt_for(
        &command.context,
        &command.project_id,
        &command.chapter_id,
        session_id,
    );
    // Issue #1069：链被淘汰或新会话时，只在“即将新建草稿”时读取一次 Core
    // 已提交版本；事实的 before 摘要必须与它对应，不能凭本地时间/正文猜。
    let fallback_receipt = if chain_receipt.is_none()
        && !super::super::chapter_draft::draft_state_initialized(
            &projects_root,
            &command.project_id,
            &command.chapter_id,
        )
        && !command.input.before_text_sha256.is_empty()
    {
        match state.api_for(&command.context).open_chapter(
            &command.project_id,
            &command.volume_id,
            &command.chapter_id,
        ) {
            Ok(opened)
                if opened.version.content_sha256 == command.input.before_text_sha256 =>
            {
                Some((
                    opened.version.object_generation,
                    opened.version.revision,
                    opened.version.content_sha256,
                ))
            }
            _ => None,
        }
    } else {
        None
    };
    let effective_base = match chain_receipt.as_ref() {
        Some(receipt)
            if !command.input.before_text_sha256.is_empty()
                && receipt.content_sha256 == command.input.before_text_sha256 =>
        {
            (
                receipt.object_generation.as_str(),
                receipt.revision,
                receipt.content_sha256.as_str(),
            )
        }
        _ => match fallback_receipt.as_ref() {
            Some((generation, revision, sha256)) => {
                (generation.as_str(), *revision, sha256.as_str())
            }
            None => (
                command.base_generation.as_str(),
                command.base_revision,
                command.base_sha256.as_str(),
            ),
        },
    };
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
        effective_base,
    ) {
        // Issue #1069：原始事实没有落盘就不能继续投影——一旦投影被写成
        // 事实文件，它就成了不可从 spool 恢复的唯一副本。立即停止，并把
        // 该 eventId/revision 记入内存失败集合；冻结会拒绝越过它的保存。
        crate::backend::app_backend::debug_error_static(
            "chapter_draft",
            "record_editor_proof_fact_spool_failed",
            &format!(
                "project={} chapter={} event={event_id}: {error}",
                command.project_id, command.chapter_id
            ),
        );
        super::super::chapter_draft::note_failed_input_in_memory(
            &projects_root,
            &command.project_id,
            &command.chapter_id,
            &event_id,
            revision,
        );
        return;
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
pub(super) fn handle_draft_text(
    state: &mut ChapterSaveWorkerState,
    command: ChapterDraftTextCommand,
) {
    let projects_root = Path::new(&command.context.projects_root);
    let mut force = command.force;
    if let Some(snapshot) =
        take_draft_text_snapshot(&command.context, &command.project_id, &command.chapter_id)
    {
        force |= snapshot.force;
        // Issue #1069：新草稿的 base 以 worker 已确认回执为准。QML 在回执到达前
        // 仍会带旧 committedChapter*；同一 generation 且回执确实前进时，编辑器
        // 正文就是那次保存的续写，不能把新草稿建到旧版本上。
        let chain_receipt = state.chain_receipt_for(
            &command.context,
            &command.project_id,
            &command.chapter_id,
            snapshot.session_id,
        );
        let effective_base = match chain_receipt.as_ref() {
            Some(receipt)
                if receipt.object_generation == snapshot.generation
                    && receipt.revision > snapshot.base_revision =>
            {
                (
                    receipt.object_generation.as_str(),
                    receipt.revision,
                    receipt.content_sha256.as_str(),
                )
            }
            _ => (
                snapshot.base_generation.as_str(),
                snapshot.base_revision,
                snapshot.base_sha256.as_str(),
            ),
        };
        // base 只在草稿状态未初始化时生效；之后由 Core durable receipt /
        // pending-ack 推进（见 chapter_draft::note_text）。
        super::super::chapter_draft::note_text(
            projects_root,
            &command.project_id,
            &command.chapter_id,
            &command.volume_id,
            snapshot.session_id,
            &snapshot.generation,
            snapshot.text.clone(),
            snapshot.revision,
            &snapshot.sha256,
            effective_base,
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
