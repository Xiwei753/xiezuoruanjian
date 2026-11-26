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

use super::{worker_sender, ChapterSaveContext, ChapterSaveJob, ChapterSaveWorkerState};

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

/// 编辑器正文变化命令（Issue #1062 三.4：只带身份与唤醒）。
///
/// 正文、revision/sha256 与 base 不再随命令进入无界 channel：GUI 线程先把
/// 它们写进 chapter_draft 的内存 registry，worker 收到命令时只按身份把该
/// 草稿按 2s 节流落盘，`force` 表示保存/关窗/离开作品边界的立即写入。
pub(crate) struct ChapterDraftTextCommand {
    pub context: ChapterSaveContext,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
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
    // pending-inputs 也是未提交事件：只有 spool、尚无 facts/text 的草稿
    // （首次事件在投影/正文写出前崩溃）同样必须进入恢复队列。
    let count = drafts
        .iter()
        .filter(|draft| {
            draft.fact_count > 0
                || draft.text_present
                || draft.pending_input_count > 0
                || draft.pending_input_error.is_some()
        })
        .count();
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

/// 把内存 registry 里已更新的草稿正文按节流落盘；`force` 时立即写整章。
pub(super) fn handle_draft_text(command: ChapterDraftTextCommand) {
    if let Err(error) = super::super::chapter_draft::persist_current(
        Path::new(&command.context.projects_root),
        &command.project_id,
        &command.chapter_id,
        command.force,
    ) {
        log::warn!(
            "persist chapter draft text failed: project={} chapter={}: {error}",
            command.project_id,
            command.chapter_id
        );
    }
}

fn version_display(version: &ChapterVersionDto) -> String {
    format!(
        "{}@{} sha={}",
        version.object_generation, version.revision, version.content_sha256
    )
}

/// 恢复提交使用的 proof snapshot revision：有事实时是事件终态 revision，
/// 否则是草稿正文 revision。
///
/// Issue #1062 复审：replay 成功后的正文是事件终态，继续传 text.txt 的旧
/// revision 会让 Core 判成 `new_revision > snapshot_revision`。
fn restore_commit_revision(plan: &super::super::chapter_draft::ChapterDraftRestorePlan) -> u64 {
    if plan.fact_count > 0 {
        plan.event_end_revision
    } else {
        plan.text_revision
    }
}

/// 事件链中 revision <= 上限的投影 JSON 数组；为空返回空串。
fn batch_json_through_revision(
    plan: &super::super::chapter_draft::ChapterDraftRestorePlan,
    through: u64,
) -> Result<String, String> {
    let mut values = Vec::new();
    for fact in plan.facts.iter().filter(|fact| fact.revision <= through) {
        let value: serde_json::Value = serde_json::from_str(&fact.projection_json)
            .map_err(|error| format!("decode draft fact revision={}: {error}", fact.revision))?;
        values.push(value);
    }
    if values.is_empty() {
        return Ok(String::new());
    }
    serde_json::to_string(&values).map_err(|error| format!("encode draft prefix batch: {error}"))
}

/// 项目进入时按同一批次协议重放未完成草稿。
///
/// 四种出口：
/// - 基线一致：用草稿事实 replay（或草稿正文）作为内容提交，成功才删草稿；
/// - 基线不一致（外部同步/MCP 改过、generation 变了、混合会话）：不覆盖，
///   用草稿 base 作 expected 调一次保存让 Core 落冲突材料，然后保留草稿；
/// - base 为空不可校验：直接保留，不调用保存；
/// - 无事实且无正文：直接删除。
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
        let chapter_id = draft.chapter_id.clone();
        // 先把 Core 投影构造失败的原始命令重试掉；仍失败就保留草稿，
        // 不能带着缺失事件把正文提交成“完整”状态。
        if let Err(error) = retry_pending_inputs(
            api,
            &projects_root,
            &command.project_id,
            &chapter_id,
            &draft.volume_id,
            draft.session_id,
            &draft.generation,
        ) {
            failed += 1;
            kept += 1;
            messages.push(format!(
                "{chapter_id}: pending_fact_retry_failed kept: {error}"
            ));
            continue;
        }
        // Issue #1062 复审 P0：重试可能刚刚写出事实文件，不能用重试前的
        // draft.fact_count 判断清理；重试后重新读取磁盘对象。
        let pending_after = match super::super::chapter_draft::pending_inputs(
            &projects_root,
            &command.project_id,
            &chapter_id,
        ) {
            Ok(inputs) => inputs,
            Err(error) => {
                failed += 1;
                kept += 1;
                messages.push(format!(
                    "{chapter_id}: pending_inputs_unreadable kept: {error}"
                ));
                continue;
            }
        };
        // 读取失败必须显式保留；只有 Ok(Some(plan)) 且证明为空才允许清理，
        // Ok(None) 表示磁盘上本来就没有草稿，也不能据此删除任何东西。
        let plan = match super::super::chapter_draft::restore_plan_strict(
            &projects_root,
            &command.project_id,
            &chapter_id,
        ) {
            Ok(Some(plan)) => plan,
            Ok(None) => continue,
            Err(error) => {
                failed += 1;
                kept += 1;
                messages.push(format!("{chapter_id}: draft_unreadable kept: {error}"));
                continue;
            }
        };
        if draft_dir_is_empty(pending_after.len(), plan.fact_count, plan.text.is_some()) {
            // 只有 meta 事务头的空壳草稿：无 pending、无事实、无正文。
            super::super::chapter_draft::remove(&projects_root, &command.project_id, &chapter_id);
            continue;
        }
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
        // Issue #1062 复审第 1 项：事实落盘与正文草稿落盘是两条独立节流，
        // 退出/崩溃时 facts 可能已经包含最后几次输入，而 text.txt 还是较早
        // 的 revision。这属于正常状态，不能一律判成 replay_text_mismatch。
        let content = if plan.fact_count == 0 {
            match plan.text.clone() {
                Some(text) => text,
                None => {
                    super::super::chapter_draft::remove(
                        &projects_root,
                        &command.project_id,
                        &plan.chapter_id,
                    );
                    continue;
                }
            }
        } else {
            if plan.text_revision > plan.event_end_revision {
                // 正文比完整事件链还新：说明有编辑事件没落盘，保留草稿等
                // 下一次重试，不能用“缺少事实”的正文覆盖。
                failed += 1;
                kept += 1;
                messages.push(format!(
                    "{}: text_ahead_of_events kept (textRevision={} eventEnd={})",
                    plan.chapter_id, plan.text_revision, plan.event_end_revision
                ));
                continue;
            }
            if let Some(draft_text) = plan.text.as_ref() {
                if plan.text_revision < plan.event_end_revision {
                    // 正文落后于事件链：先核对 text.txt 确实等于事件链在
                    // text_revision 的中间态，再继续 replay 后续事件。
                    let prefix = match batch_json_through_revision(&plan, plan.text_revision) {
                        Ok(json) if !json.is_empty() => json,
                        Ok(_) => {
                            failed += 1;
                            kept += 1;
                            messages.push(format!(
                                "{}: draft_text_precedes_events kept (textRevision={} firstEvent={})",
                                plan.chapter_id,
                                plan.text_revision,
                                plan.facts.first().map_or(0, |fact| fact.revision)
                            ));
                            continue;
                        }
                        Err(error) => {
                            failed += 1;
                            kept += 1;
                            messages.push(format!(
                                "{}: draft_prefix_encode_failed kept: {error}",
                                plan.chapter_id
                            ));
                            continue;
                        }
                    };
                    match api.replay_editor_proof_batch(&plan.chapter_id, &opened.content, &prefix)
                    {
                        Ok(mid) if mid == *draft_text => {}
                        Ok(_) => {
                            failed += 1;
                            kept += 1;
                            messages.push(format!(
                                "{}: draft_text_mismatch_at_revision kept (textRevision={})",
                                plan.chapter_id, plan.text_revision
                            ));
                            continue;
                        }
                        Err(error) => {
                            failed += 1;
                            kept += 1;
                            messages.push(format!(
                                "{}: draft_prefix_replay_failed kept: {error}",
                                plan.chapter_id
                            ));
                            continue;
                        }
                    }
                }
            }
            match api.replay_editor_proof_batch(&plan.chapter_id, &opened.content, &plan.batch_json)
            {
                Ok(text) => {
                    if plan.text_revision == plan.event_end_revision {
                        if let Some(draft_text) = plan.text.as_ref() {
                            if text != *draft_text {
                                failed += 1;
                                kept += 1;
                                messages.push(format!(
                                    "{}: replay_text_mismatch failed kept (replayed_len={} draft_len={})",
                                    plan.chapter_id,
                                    text.len(),
                                    draft_text.len()
                                ));
                                continue;
                            }
                        }
                    }
                    text
                }
                Err(error) => {
                    failed += 1;
                    kept += 1;
                    messages.push(format!("{}: replay_failed kept: {error}", plan.chapter_id));
                    continue;
                }
            }
        };
        // Issue #1062 复审：全链 replay 成功后提交的正文是事件终态，proof
        // snapshot revision 必须跟着到 event_end_revision；继续用 text.txt
        // 的旧 revision 会被 Core 判成 new_revision > snapshot_revision。
        let commit_revision = restore_commit_revision(&plan);
        let draft_base = ChapterVersionDto {
            object_generation: plan.base_generation.clone(),
            revision: plan.base_revision,
            content_sha256: plan.base_sha256.clone(),
        };
        let base_known = !plan.base_generation.is_empty() || plan.base_revision > 0;
        let base_matches = base_known
            && plan.base_generation == opened.version.object_generation
            && plan.base_revision == opened.version.revision
            && plan.base_sha256 == opened.version.content_sha256;
        if base_matches && !plan.mixed_sessions {
            let result = api.save_chapter_content_versioned_with_proof_batch(
                &command.project_id,
                &plan.volume_id,
                &plan.chapter_id,
                &content,
                Some(opened.version),
                Some(opened.content),
                commit_revision,
                &plan.batch_json,
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
                    messages.push(format!(
                        "{}: save_draft_failed kept: {error}",
                        plan.chapter_id
                    ));
                }
            }
            continue;
        }
        if base_known && !base_matches {
            // 不覆盖外部更新：用草稿记录的原始 base 作 expected，让 Core
            // 写出冲突材料（预期 CHAPTER_VERSION_CONFLICT），草稿原样保留。
            let result = api.save_chapter_content_versioned_with_proof_batch(
                &command.project_id,
                &plan.volume_id,
                &plan.chapter_id,
                &content,
                Some(draft_base.clone()),
                Some(opened.content),
                commit_revision,
                &plan.batch_json,
                true,
            );
            match result {
                Ok(_receipt) => crate::backend::app_backend::debug_error_static(
                    "chapter_draft",
                    "restore_conflict_save_unexpected_ok",
                    &format!(
                        "project={} chapter={}: draft base was unexpectedly accepted",
                        command.project_id, plan.chapter_id
                    ),
                ),
                Err(error) if error.code() == "CHAPTER_VERSION_CONFLICT" => {}
                Err(error) => crate::backend::app_backend::debug_error_static(
                    "chapter_draft",
                    "restore_conflict_save_failed",
                    &format!(
                        "project={} chapter={}: {error}",
                        command.project_id, plan.chapter_id
                    ),
                ),
            }
        }
        let reason = if !base_known {
            "base_unverifiable"
        } else if plan.mixed_sessions {
            "mixed_sessions"
        } else {
            "external_change"
        };
        kept += 1;
        messages.push(format!(
            "{}: external_change_kept ({reason}) draft_base={} disk_version={} {}",
            plan.chapter_id,
            version_display(&draft_base),
            version_display(&opened.version),
            plan.session_summary()
        ));
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

/// 已成功读取的草稿清单显示“无 pending、无事实、无正文”时才允许删除。
///
/// 调用方只能传入 `restore_plan_strict` 成功返回的 plan；读取失败/未知
/// 状态必须在调用前就作为 kept 报告，不能落到这个判断里。
fn draft_dir_is_empty(pending_inputs: usize, fact_count: usize, has_text: bool) -> bool {
    pending_inputs == 0 && fact_count == 0 && !has_text
}

#[cfg(test)]
mod tests {
    use super::restore_commit_revision;
    use crate::backend::chapter_draft::{ChapterDraftRestorePlan, LoadedFact};

    fn plan(fact_count: usize, text_revision: u64, event_end: u64) -> ChapterDraftRestorePlan {
        ChapterDraftRestorePlan {
            volume_id: "v1".to_string(),
            chapter_id: "c1".to_string(),
            base_generation: "g1".to_string(),
            base_revision: 1,
            base_sha256: "sha".to_string(),
            text: None,
            text_revision,
            text_sha256: String::new(),
            fact_count,
            batch_json: "[]".to_string(),
            facts: if fact_count > 0 {
                vec![LoadedFact {
                    event_id: "e1".to_string(),
                    revision: event_end,
                    projection_json: "{}".to_string(),
                }]
            } else {
                Vec::new()
            },
            event_end_revision: event_end,
            session_id: 7,
            generation: "g1".to_string(),
            mixed_sessions: false,
        }
    }

    #[test]
    fn empty_draft_cleanup_requires_no_pending_facts_or_text() {
        use super::draft_dir_is_empty;
        assert!(draft_dir_is_empty(0, 0, false));
        // 首次事件只有 spool、facts/text 尚未写出：绝不能按旧摘要删除。
        assert!(!draft_dir_is_empty(1, 0, false));
        assert!(!draft_dir_is_empty(0, 1, false));
        assert!(!draft_dir_is_empty(0, 0, true));
    }

    #[test]
    fn replay_terminal_text_commits_at_event_end_revision() {
        // text.txt 落后一个事件（revision 4），事件链到 5：提交必须用 5。
        assert_eq!(restore_commit_revision(&plan(2, 4, 5)), 5);
        // 没有事实时仍用正文 revision。
        assert_eq!(restore_commit_revision(&plan(0, 4, 0)), 4);
    }
}
