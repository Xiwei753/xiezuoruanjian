// =============================================================================
// chapter_save_queue/restore_draft.rs — 项目进入时的草稿恢复
// =============================================================================
//
// Issue #1062 评论 7191076411 第 4 项：恢复必须在任何 replay 之前比较草稿
// 原始 base 与磁盘版本；外部同步改过正文时用草稿记录的原始基线重放出
// incoming，向 Core 传原始 base（生成正确的 base.md/local.md/incoming.md），
// 绝不拿外部新正文冒充历史基线。

use std::path::{Path, PathBuf};

use writer_core::api::types::ChapterVersionDto;
use writer_core::api::WriterCoreApi;

use super::draft_jobs::{draft_restore_state, retry_pending_inputs, ChapterDraftRestoreStatus};
use super::kept_draft::{KeptDisposition, RestoreCounters};
use super::{ChapterRestoreCommand, ChapterSaveWorkerState};

pub(crate) fn version_display(version: &ChapterVersionDto) -> String {
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
pub(crate) fn restore_commit_revision(
    plan: &super::super::chapter_draft::ChapterDraftRestorePlan,
) -> u64 {
    if plan.fact_count > 0 {
        plan.event_end_revision
    } else {
        plan.text_revision
    }
}

/// 事件链中 revision <= 上限的投影 JSON 数组；为空返回空串。
pub(crate) fn batch_json_through_revision(
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

/// 草稿记录的 base 是否就是刚读到的磁盘版本（generation/revision/sha 全一致）。
///
/// Issue #1062 评论 7191076411 第 4 项：恢复决议必须以草稿记录**原始 base**
/// 为准，不能用刚读到的磁盘正文冒充基线。base 为空（generation 空且 revision
/// 为 0）视为不可校验，返回 false。
pub(crate) fn base_matches_plan(
    plan: &super::super::chapter_draft::ChapterDraftRestorePlan,
    version: &ChapterVersionDto,
) -> bool {
    let base_known = !plan.base_generation.is_empty() || plan.base_revision > 0;
    base_known
        && plan.base_generation == version.object_generation
        && plan.base_revision == version.revision
        && plan.base_sha256 == version.content_sha256
}

/// 从一条草稿事实的原始投影 JSON 取回事件链起点的正文基线。
///
/// `ChapterProofProjection` 以 camelCase 序列化，`baselineText` 是编辑器会话
/// 第一条事实随带的旧正文；缺失、null 或空串（老客户端只上报计数）返回
/// None，JSON 损坏也返回 None，由调用方按“基线不可恢复”保留草稿。
pub(crate) fn baseline_text_from_projection(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    // Issue #1062 评论 7192111899 第 3 项：空章基线是合法的 `""`；只有字段
    // 完全缺失（或非字符串）才表示“没有基线”。
    value.get("baselineText")?.as_str().map(str::to_string)
}

/// 草稿事件链第一条事实携带的原始基线正文，用于外部变更后的冲突材料。
///
/// 不能用 `opened.content`：外部同步/MCP 改过正文后它就是外部新版本，把它
/// 当作 replay baseline 或 Core 的 `base_content` 都会丢失旧基线。
pub(crate) fn original_base_text(
    plan: &super::super::chapter_draft::ChapterDraftRestorePlan,
) -> Option<String> {
    baseline_text_from_projection(&plan.facts.first()?.projection_json)
}

/// 无事实草稿是否仍是本次文档加载基线（编辑器坐标未变、摘要一致）。
pub(crate) fn unchanged_baseline(
    plan: &super::super::chapter_draft::ChapterDraftRestorePlan,
) -> bool {
    plan.text.is_some()
        && plan.text_revision == plan.editor_base_revision
        && !plan.editor_base_sha256.is_empty()
        && plan.text_sha256 == plan.editor_base_sha256
}

/// 项目进入时按同一批次协议重放未完成草稿。
///
/// 决议顺序（都在任何 replay 之前用草稿原始 base 判定）：
/// - 混合会话：不 replay 不保存，直接保留；
/// - base 为空不可校验：直接保留，不调用保存；
/// - base 与磁盘不一致（外部同步/MCP 改过、generation 变了）：用第一条事实
///   随带的原始基线 replay 出草稿终态，再以草稿 base 为 expected、原始基线
///   为 `base_content` 调一次保存让 Core 落冲突材料，然后保留草稿；
/// - base 一致：用草稿事实 replay（或草稿正文）作为内容提交，成功才删草稿；
/// - 无事实且无正文：直接删除。
pub(super) fn handle_restore_project(
    state: &mut ChapterSaveWorkerState,
    command: ChapterRestoreCommand,
) {
    let projects_root = PathBuf::from(&command.context.projects_root);
    let mut counters = RestoreCounters::default();
    if let Ok(mut states) = draft_restore_state().lock() {
        states.insert(
            command.project_id.clone(),
            ChapterDraftRestoreStatus {
                running: true,
                pending: true,
                ..ChapterDraftRestoreStatus::default()
            },
        );
    }
    // Issue #1062 评论 7195690539 第 1 项：可自动重试的事务平时停在不可写归档
    // 区，进入项目时先搬回活动槽位；活动槽位已有新会话草稿时绝不覆盖，保留
    // 归档。
    //
    // Issue #1062 评论 #7210163429 第 1/2 项：重试账本按归档事务记账，同章可
    // 以同时挂起多份旧草稿；这里逐份 materialize，一次只把一份放进活动槽位，
    // 其余继续等下一次入口。清理只针对被搬走/归档丢失的那一条标记。
    // Issue #1062 评论 #7209683560 第 3 项：有标记文件但解析失败的章节保留
    // 历史归档、显式报告，本轮不能冒充活动草稿。
    for chapter_id in
        super::super::chapter_draft::unreadable_retry_chapters(&projects_root, &command.project_id)
    {
        counters.failed += 1;
        counters.note(&chapter_id, "retry marker unreadable kept");
    }
    // Issue #1062 评论 7191076411 第 1 项（平台侧）：Core durable journal 是
    // 草稿 ack 的唯一事实源；进入项目时按 Core 记录对账。
    // Issue #1062 评论 #7209683560 第 1 项：对账必须在 retry 副本搬回活动
    // 槽位之后执行，否则 Core ack 会在空槽位伪造 pending-ack。
    // Issue #1062 评论 7192111899 第 2 项：对账失败必须阻止对应章节的旧事实
    // replay/提交；Core journal 仍保留原始证明事件，下一次入口重新对账。
    let mut blocked_chapters: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let mut reconcile_failed_all = false;
    {
        let api = state.api_for(&command.context);
        match super::core_ack::reconcile_core_acks(
            api,
            command.context.projects_root(),
            &command.project_id,
            None,
        ) {
            Ok(outcome) => {
                // 延后的 ack 只记录，不阻塞当前编辑；Core journal 原样保留。
                for skipped in outcome.skipped {
                    counters.note(
                        &skipped.chapter_id,
                        format!("core_draft_ack_deferred: {}", skipped.reason),
                    );
                }
            }
            Err(error) => {
                if error.failures().is_empty() {
                    reconcile_failed_all = true;
                } else {
                    for failure in error.failures() {
                        blocked_chapters.insert(failure.chapter_id.clone());
                    }
                }
                counters
                    .messages
                    .push(format!("core_draft_ack_reconcile_failed: {error}"));
            }
        }
    }
    // Issue #1062 评论 #7213634487 第 B 项：同章多事务按恢复队列逐个处理——
    // 每轮只把一份 retry 副本搬回活动槽位并处理，成功/归档后立即接下一份，
    // 直到该章本轮候选都拿到 durable 状态或明确延后。
    let api = state.api_for(&command.context);
    let mut noted_retries: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut reported_unreadable: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let mut give_up: std::collections::BTreeSet<String> = blocked_chapters.clone();
    loop {
        super::restore_coordinator::materialize_pending_retries(
            &projects_root,
            &command.project_id,
            &give_up,
            &mut noted_retries,
            &mut counters,
        );
        let discovery = match super::super::chapter_draft::discover_project_drafts(
            &projects_root,
            &command.project_id,
        ) {
            Ok(discovery) => discovery,
            Err(error) => {
                counters.failed = counters.failed.saturating_add(1);
                counters.note("", format!("draft discovery failed (kept): {error}"));
                break;
            }
        };
        for error in &discovery.scan_errors {
            counters.failed = counters.failed.saturating_add(1);
            counters.note("", format!("draft discovery: {error}"));
        }
        // Issue #1062 评论 #7214283385 第 1 项：Core durable ack 对账必须在
        // materialize 之后按事务推进。进入作品前的那次全局对账只能覆盖
        // “已无本地草稿”的 ack；被归档区的 retry 事务会在那一轮被 skipped，
        // 搬回活动槽位后必须立刻用同一章节的 Core 记录重新对账，再走
        // restore_plan_strict/replay，否则已提交事实会被当成未提交重放。
        // 对账失败时保留草稿与 Core journal，并把本章标为 Deferred，不 replay。
        if !reconcile_failed_all {
            for draft in &discovery.drafts {
                let chapter_id = draft.chapter_id.as_str();
                if give_up.contains(chapter_id)
                    || blocked_chapters.contains(chapter_id)
                    || counters.blocked_chapters.iter().any(|id| id == chapter_id)
                {
                    continue;
                }
                match super::core_ack::reconcile_core_acks(
                    api,
                    command.context.projects_root(),
                    &command.project_id,
                    Some(chapter_id),
                ) {
                    Ok(outcome) => {
                        for skipped in outcome.skipped {
                            counters.note(
                                &skipped.chapter_id,
                                format!("core_draft_ack_deferred: {}", skipped.reason),
                            );
                        }
                    }
                    Err(error) => {
                        if error.failures().is_empty() {
                            reconcile_failed_all = true;
                            counters
                                .messages
                                .push(format!("core_draft_ack_reconcile_failed: {error}"));
                        } else {
                            for failure in error.failures() {
                                blocked_chapters.insert(failure.chapter_id.clone());
                                give_up.insert(failure.chapter_id.clone());
                                counters.block(&failure.chapter_id);
                                counters.note(
                                    &failure.chapter_id,
                                    format!(
                                        "core_draft_ack_reconcile_failed kept (retryable): {}",
                                        failure.reason
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
        for unreadable in &discovery.unreadable {
            let key = format!("{}\u{1f}{}", unreadable.chapter_id, unreadable.path);
            if !reported_unreadable.insert(key) {
                continue;
            }
            counters.note(
                &unreadable.chapter_id,
                format!(
                    "draft unreadable: {} path={}",
                    unreadable.error, unreadable.path
                ),
            );
            // 读不出来也只是“未决”，不是阻断：先把它搬出可写槽位；只有隔离
            // 失败（原目录仍可能被新会话覆写）才阻断该章。
            counters.keep(
                &projects_root,
                &command.project_id,
                &unreadable.chapter_id,
                super::kept_draft::KeptDisposition::Archive,
            );
            if counters
                .blocked_chapters
                .iter()
                .any(|id| id == &unreadable.chapter_id)
            {
                give_up.insert(unreadable.chapter_id.clone());
            }
        }
        let drafts: Vec<super::super::chapter_draft::ChapterDraft> = discovery
            .drafts
            .into_iter()
            .filter(|draft| {
                !give_up.contains(&draft.chapter_id)
                    && !counters
                        .blocked_chapters
                        .iter()
                        .any(|id| id == &draft.chapter_id)
            })
            .collect();
        if drafts.is_empty() {
            break;
        }
        let chapters: Vec<String> = drafts
            .iter()
            .map(|draft| draft.chapter_id.clone())
            .collect();
        let retries_before =
            super::restore_coordinator::retry_snapshot(&projects_root, &command.project_id);
        for draft in drafts {
            let chapter_id = draft.chapter_id.clone();
            if reconcile_failed_all || blocked_chapters.contains(&chapter_id) {
                counters.failed += 1;
                counters.kept += 1;
                counters.note(
                    &chapter_id,
                    "core_draft_ack_reconcile_failed kept (retryable)",
                );
                counters.keep(
                    &projects_root,
                    &command.project_id,
                    &chapter_id,
                    KeptDisposition::Retry {
                        volume_id: draft.volume_id.clone(),
                        reason: "core_draft_ack_reconcile_failed".to_string(),
                        attempts: 0,
                    },
                );
                continue;
            }
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
                counters.failed += 1;
                counters.kept += 1;
                counters.note(
                    &chapter_id,
                    format!("pending_fact_retry_failed kept (retryable): {error}"),
                );
                counters.keep(
                    &projects_root,
                    &command.project_id,
                    &chapter_id,
                    KeptDisposition::Retry {
                        volume_id: draft.volume_id.clone(),
                        reason: format!("pending_fact_retry_failed: {error}"),
                        attempts: 0,
                    },
                );
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
                    counters.failed += 1;
                    counters.kept += 1;
                    counters.note(
                        &chapter_id,
                        format!("pending_inputs_unreadable kept (retryable): {error}"),
                    );
                    counters.keep(
                        &projects_root,
                        &command.project_id,
                        &chapter_id,
                        KeptDisposition::Retry {
                            volume_id: draft.volume_id.clone(),
                            reason: format!("pending_inputs_unreadable: {error}"),
                            attempts: 0,
                        },
                    );
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
                Ok(None) => {
                    // meta 缺失/不可读但有磁盘内容时，必须作为“未决”报告，
                    // 不能像“项目本来没有草稿”一样静默跳过。
                    if let Some(error) = draft.draft_error.as_deref() {
                        counters.failed += 1;
                        counters.kept += 1;
                        counters.note(&chapter_id, format!("draft_unreadable kept: {error}"));
                        counters.keep(
                            &projects_root,
                            &command.project_id,
                            &chapter_id,
                            KeptDisposition::Archive,
                        );
                    }
                    continue;
                }
                Err(error) => {
                    counters.failed += 1;
                    counters.kept += 1;
                    // Issue #1062 评论 #7209683560 第 2 项：pending-inputs spool
                    // 损坏仍然代表未转换的原始编辑事实，必须保留原件等待重试，
                    // 不能按“永久不可恢复”归档。
                    let pending_retry =
                        error.contains(super::super::chapter_draft::PENDING_INPUTS_UNREADABLE);
                    counters.note(
                        &chapter_id,
                        if pending_retry {
                            format!("pending_inputs_unreadable kept (retryable): {error}")
                        } else {
                            format!("draft_unreadable kept: {error}")
                        },
                    );
                    counters.keep(
                        &projects_root,
                        &command.project_id,
                        &chapter_id,
                        if pending_retry {
                            KeptDisposition::Retry {
                                volume_id: draft.volume_id.clone(),
                                reason: "pending_inputs_unreadable".to_string(),
                                attempts: 0,
                            }
                        } else {
                            KeptDisposition::Archive
                        },
                    );
                    continue;
                }
            };
            if draft_dir_is_empty(pending_after.len(), plan.fact_count, plan.text.is_some()) {
                // 只有 meta 事务头的空壳草稿：无 pending、无事实、无正文。
                counters.settle_removed(&projects_root, &command.project_id, &chapter_id);
                continue;
            }
            let opened =
                match api.open_chapter(&command.project_id, &plan.volume_id, &plan.chapter_id) {
                    Ok(content) => content,
                    Err(error) => {
                        counters.failed += 1;
                        counters.kept += 1;
                        counters.note(
                            &chapter_id,
                            format!("cannot read committed chapter kept (retryable): {error}"),
                        );
                        counters.keep(
                            &projects_root,
                            &command.project_id,
                            &chapter_id,
                            KeptDisposition::Retry {
                                volume_id: plan.volume_id.clone(),
                                reason: format!("cannot read committed chapter: {error}"),
                                attempts: 0,
                            },
                        );
                        continue;
                    }
                };
            // Issue #1062 评论 7191076411 第 4 项：草稿 base 是编辑器载入时的
            // 持久化版本，不是刚读到的磁盘版本。必须在任何 replay / 保存之前
            // 先判定外部更新，否则旧事实第一条 before 摘要不匹配会先报
            // replay_failed，永远走不到冲突材料路径。
            let draft_base = ChapterVersionDto {
                object_generation: plan.base_generation.clone(),
                revision: plan.base_revision,
                content_sha256: plan.base_sha256.clone(),
            };
            let base_known = !plan.base_generation.is_empty() || plan.base_revision > 0;
            let base_matches = base_matches_plan(&plan, &opened.version);
            let commit_revision = restore_commit_revision(&plan);
            // Issue #1062 评论 7193847871 第 3 项：无事实草稿只要正文已经等于
            // 当前 live 正文（Core 原始 SHA-256 协议）或 meta 已记录为确认快照，
            // 就是已提交内容的残留；stale base 不能把这种情况判成外部冲突。
            if plan.fact_count == 0 {
                let committed_live = plan.text.as_ref().is_some_and(|text| {
                    writer_core::chapter::content_sha256(text) == opened.version.content_sha256
                });
                let committed_record = plan.text.is_some()
                    && !plan.committed_text_sha256.is_empty()
                    && plan.text_revision == plan.committed_text_revision
                    && plan.text_sha256 == plan.committed_text_sha256;
                if committed_live || committed_record {
                    counters.settle_removed(&projects_root, &command.project_id, &plan.chapter_id);
                    continue;
                }
            }
            // 有任何未确认事实来自别的会话/generation 时，绝不能把混合会话
            // 提交成“已完成”：既不 replay 也不保存，全部原件原样保留。
            if plan.mixed_sessions {
                counters.kept += 1;
                counters.note(
                    &plan.chapter_id,
                    format!(
                        "external_change_kept (mixed_sessions) draft_base={} disk_version={} {}",
                        version_display(&draft_base),
                        version_display(&opened.version),
                        plan.session_summary()
                    ),
                );
                counters.keep(
                    &projects_root,
                    &command.project_id,
                    &plan.chapter_id,
                    KeptDisposition::Archive,
                );
                continue;
            }
            // 草稿没有记录可校验的 base：沿用既有行为，不 replay、不保存。
            if !base_known {
                counters.kept += 1;
                counters.note(
                    &plan.chapter_id,
                    format!(
                        "external_change_kept (base_unverifiable) draft_base={} disk_version={} {}",
                        version_display(&draft_base),
                        version_display(&opened.version),
                        plan.session_summary()
                    ),
                );
                counters.keep(
                    &projects_root,
                    &command.project_id,
                    &plan.chapter_id,
                    KeptDisposition::Archive,
                );
                continue;
            }
            // 外部同步/MCP 改过正文：用事件链第一条事实携带的原始基线 replay 出
            // 草稿终态，再以草稿原始 base 为 expected、原始基线为 base_content
            // 调一次保存，让 Core 落出 base.md=旧基线、local.md=外部版本、
            // incoming.md=草稿终态的冲突材料；草稿本身原样保留。
            if !base_matches {
                let origin = match original_base_text(&plan) {
                    Some(origin) => origin,
                    None => {
                        counters.kept += 1;
                        counters.note(
                            &plan.chapter_id,
                            format!(
                                "base_unrecoverable kept (draft_base={} disk_version={})",
                                version_display(&draft_base),
                                version_display(&opened.version)
                            ),
                        );
                        counters.keep(
                            &projects_root,
                            &command.project_id,
                            &plan.chapter_id,
                            KeptDisposition::Archive,
                        );
                        continue;
                    }
                };
                let incoming = match api.replay_editor_proof_batch(
                    &plan.chapter_id,
                    &origin,
                    &plan.batch_json,
                ) {
                    Ok(text) => text,
                    Err(error) => {
                        counters.kept += 1;
                        counters.note(
                            &plan.chapter_id,
                            format!("original_base_replay_failed kept (retryable): {error}"),
                        );
                        counters.keep(
                            &projects_root,
                            &command.project_id,
                            &plan.chapter_id,
                            KeptDisposition::Retry {
                                volume_id: plan.volume_id.clone(),
                                reason: format!("original_base_replay_failed: {error}"),
                                attempts: 0,
                            },
                        );
                        continue;
                    }
                };
                let result = api.save_chapter_content_versioned_with_proof_batch(
                    &command.project_id,
                    &plan.volume_id,
                    &plan.chapter_id,
                    &incoming,
                    Some(draft_base.clone()),
                    Some(origin),
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
                counters.kept += 1;
                counters.note(
                &plan.chapter_id,
                format!(
                    "external_change_kept (external_change) draft_base={} disk_version={} incoming_len={} {}",
                    version_display(&draft_base),
                    version_display(&opened.version),
                    incoming.len(),
                    plan.session_summary()
                ),
            );
                counters.keep(
                    &projects_root,
                    &command.project_id,
                    &plan.chapter_id,
                    KeptDisposition::Archive,
                );
                continue;
            }
            // Issue #1062 复审第 1 项：事实落盘与正文草稿落盘是两条独立节流，
            // 退出/崩溃时 facts 可能已经包含最后几次输入，而 text.txt 还是较早
            // 的 revision。这属于正常状态，不能一律判成 replay_text_mismatch。
            let content = if plan.fact_count == 0 {
                let Some(text) = plan.text.clone() else {
                    counters.settle_removed(&projects_root, &command.project_id, &plan.chapter_id);
                    continue;
                };
                // Issue #1062 评论 7193130307 第 1 项：没有逐字事实时不能再用
                // “编辑器 revision > 章节 revision”判断；先看草稿正文是否已经
                // 就是当前 Core 版本（原始 SHA-256 协议），是则幂等清理。
                if writer_core::chapter::content_sha256(&text) == opened.version.content_sha256 {
                    counters.settle_removed(&projects_root, &command.project_id, &plan.chapter_id);
                    continue;
                }
                // 回执已确认收口过这份正文（meta 记录了编辑器 revision+摘要）：
                // 即使 live 随后被同步改过，也不能把已提交正文当未证明新编辑。
                let committed_snapshot = !plan.committed_text_sha256.is_empty()
                    && plan.text_revision == plan.committed_text_revision
                    && plan.text_sha256 == plan.committed_text_sha256;
                if committed_snapshot {
                    counters.settle_removed(&projects_root, &command.project_id, &plan.chapter_id);
                    continue;
                }
                // 不是 live 正文：只有仍是本次文档加载基线（同一编辑器会话、
                // revision 未变、摘要一致）才说明“没有新输入”；即使如此也与
                // live 不一致，属于外部更改，保留草稿等待人工/后续同步处理。
                if unchanged_baseline(&plan) {
                    counters.failed += 1;
                    counters.kept += 1;
                    counters.note(
                        &plan.chapter_id,
                        format!(
                            "external_change_without_proof kept (textRevision={} editorBase={})",
                            plan.text_revision, plan.editor_base_revision
                        ),
                    );
                    counters.keep(
                        &projects_root,
                        &command.project_id,
                        &plan.chapter_id,
                        KeptDisposition::Archive,
                    );
                    continue;
                }
                counters.failed += 1;
                counters.kept += 1;
                counters.note(
                    &plan.chapter_id,
                    format!(
                        "draft_text_without_proof kept (textRevision={} editorBase={} textSha={})",
                        plan.text_revision, plan.editor_base_revision, plan.text_sha256
                    ),
                );
                counters.keep(
                    &projects_root,
                    &command.project_id,
                    &plan.chapter_id,
                    KeptDisposition::Archive,
                );
                continue;
            } else {
                if plan.text_revision > plan.event_end_revision {
                    // 正文比完整事件链还新：说明有编辑事件没落盘，保留草稿等
                    // 下一次重试，不能用“缺少事实”的正文覆盖。
                    counters.failed += 1;
                    counters.kept += 1;
                    counters.note(
                        &plan.chapter_id,
                        format!(
                            "text_ahead_of_events kept (retryable, textRevision={} eventEnd={})",
                            plan.text_revision, plan.event_end_revision
                        ),
                    );
                    counters.keep(
                        &projects_root,
                        &command.project_id,
                        &plan.chapter_id,
                        KeptDisposition::Retry {
                            volume_id: plan.volume_id.clone(),
                            reason: "text_ahead_of_events".to_string(),
                            attempts: 0,
                        },
                    );
                    continue;
                }
                if let Some(draft_text) = plan.text.as_ref() {
                    if plan.text_revision < plan.event_end_revision {
                        // 正文落后于事件链：先核对正文确实等于事件链在
                        // text_revision 的中间态，再继续 replay 后续事件。
                        let prefix = match batch_json_through_revision(&plan, plan.text_revision) {
                            Ok(json) if !json.is_empty() => json,
                            Ok(_) => {
                                counters.failed += 1;
                                counters.kept += 1;
                                counters.note(
                                &plan.chapter_id,
                                format!(
                                    "draft_text_precedes_events kept (textRevision={} firstEvent={})",
                                    plan.text_revision,
                                    plan.facts.first().map_or(0, |fact| fact.revision)
                                ),
                            );
                                counters.keep(
                                    &projects_root,
                                    &command.project_id,
                                    &plan.chapter_id,
                                    KeptDisposition::Archive,
                                );
                                continue;
                            }
                            Err(error) => {
                                counters.failed += 1;
                                counters.kept += 1;
                                counters.note(
                                    &plan.chapter_id,
                                    format!("draft_prefix_encode_failed kept: {error}"),
                                );
                                counters.keep(
                                    &projects_root,
                                    &command.project_id,
                                    &plan.chapter_id,
                                    KeptDisposition::Archive,
                                );
                                continue;
                            }
                        };
                        match api.replay_editor_proof_batch(
                            &plan.chapter_id,
                            &opened.content,
                            &prefix,
                        ) {
                            Ok(mid) if mid == *draft_text => {}
                            Ok(_) => {
                                counters.failed += 1;
                                counters.kept += 1;
                                counters.note(
                                    &plan.chapter_id,
                                    format!(
                                        "draft_text_mismatch_at_revision kept (textRevision={})",
                                        plan.text_revision
                                    ),
                                );
                                counters.keep(
                                    &projects_root,
                                    &command.project_id,
                                    &plan.chapter_id,
                                    KeptDisposition::Archive,
                                );
                                continue;
                            }
                            Err(error) => {
                                counters.failed += 1;
                                counters.kept += 1;
                                counters.note(
                                    &plan.chapter_id,
                                    format!("draft_prefix_replay_failed kept (retryable): {error}"),
                                );
                                counters.keep(
                                    &projects_root,
                                    &command.project_id,
                                    &plan.chapter_id,
                                    KeptDisposition::Retry {
                                        volume_id: plan.volume_id.clone(),
                                        reason: format!("draft_prefix_replay_failed: {error}"),
                                        attempts: 0,
                                    },
                                );
                                continue;
                            }
                        }
                    }
                }
                match api.replay_editor_proof_batch(
                    &plan.chapter_id,
                    &opened.content,
                    &plan.batch_json,
                ) {
                    Ok(text) => {
                        if plan.text_revision == plan.event_end_revision {
                            if let Some(draft_text) = plan.text.as_ref() {
                                if text != *draft_text {
                                    counters.failed += 1;
                                    counters.kept += 1;
                                    counters.note(
                                    &plan.chapter_id,
                                    format!(
                                        "replay_text_mismatch failed kept (replayed_len={} draft_len={})",
                                        text.len(),
                                        draft_text.len()
                                    ),
                                );
                                    counters.keep(
                                        &projects_root,
                                        &command.project_id,
                                        &plan.chapter_id,
                                        KeptDisposition::Archive,
                                    );
                                    continue;
                                }
                            }
                        }
                        text
                    }
                    Err(error) => {
                        counters.failed += 1;
                        counters.kept += 1;
                        counters.note(
                            &plan.chapter_id,
                            format!("replay_failed kept (retryable): {error}"),
                        );
                        counters.keep(
                            &projects_root,
                            &command.project_id,
                            &plan.chapter_id,
                            KeptDisposition::Retry {
                                volume_id: plan.volume_id.clone(),
                                reason: format!("replay_failed: {error}"),
                                attempts: 0,
                            },
                        );
                        continue;
                    }
                }
            };
            // 全链 replay 成功后提交的正文是事件终态，proof snapshot revision
            // 已在上面按 event_end_revision 计算；base 已校验一致，磁盘正文就是
            // 草稿基线，可以安全提交成新版本，成功才删草稿。
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
                    counters.settle_removed(&projects_root, &command.project_id, &plan.chapter_id);
                    counters.restored += 1;
                }
                Err(error) => {
                    counters.failed += 1;
                    counters.kept += 1;
                    counters.note(
                        &plan.chapter_id,
                        format!("save_draft_failed kept (retryable): {error}"),
                    );
                    counters.keep(
                        &projects_root,
                        &command.project_id,
                        &plan.chapter_id,
                        KeptDisposition::Retry {
                            volume_id: plan.volume_id.clone(),
                            reason: format!("save_draft_failed: {error}"),
                            attempts: 0,
                        },
                    );
                }
            }
            continue;
        }
        // 本轮刚失败/新归档或活动目录仍在的章节，本入口不再无条件重试；
        // 它们的 durable 决议（retry-intent/kept）已经登记，下次进入项目继续。
        let retries_after =
            super::restore_coordinator::retry_snapshot(&projects_root, &command.project_id);
        for chapter_id in chapters {
            let grew = retries_after.get(&chapter_id).is_some_and(|paths| {
                paths.iter().any(|path| {
                    !retries_before
                        .get(&chapter_id)
                        .is_some_and(|before| before.contains(path))
                })
            });
            let active = super::super::chapter_draft::draft_active_dir_exists(
                &projects_root,
                &command.project_id,
                &chapter_id,
            );
            if grew || active {
                give_up.insert(chapter_id);
            }
        }
        let remaining: Vec<String> =
            super::super::chapter_draft::retry_chapters(&projects_root, &command.project_id)
                .into_iter()
                .filter(|chapter_id| !give_up.contains(chapter_id))
                .collect();
        if remaining.is_empty() {
            break;
        }
    }
    // Issue #1062 评论 #7210163429 第 1/2 项：重试标记按归档事务记账，这里
    // 不再按 chapter_id 收敛“同章过期标记”——那会把别的事务的恢复指针一起
    // 删除。每条标记只会被自己的 materialize/归档丢失路径精确清理。
    // Issue #1062 评论 #7214283385 第 3 项：扫描完整性是显式决议。不完整
    // 扫描中的“没发现”不能当成“没有草稿”：项目级 remaining 计入扫描错误，
    // 状态保持 pending，QML 只有拿到一次完整发现才允许进入编辑。
    let (remaining, deferred, scan_incomplete, scan_errors) =
        match super::super::chapter_draft::discover_project_drafts(
            &projects_root,
            &command.project_id,
        ) {
            Ok(discovery) => (
                discovery.drafts.len() + discovery.unreadable.len() + discovery.scan_errors.len(),
                super::super::chapter_draft::retry_chapters(&projects_root, &command.project_id)
                    .len(),
                !discovery.is_complete(),
                discovery.scan_errors,
            ),
            Err(error) => {
                counters
                    .messages
                    .push(format!("draft discovery failed after restore: {error}"));
                (
                    1,
                    super::super::chapter_draft::retry_chapters(
                        &projects_root,
                        &command.project_id,
                    )
                    .len(),
                    true,
                    vec![error],
                )
            }
        };
    for error in &scan_errors {
        counters
            .messages
            .push(format!("draft discovery incomplete: {error}"));
    }
    if scan_incomplete {
        counters
            .messages
            .push("draft discovery incomplete (project recovery unresolved)".to_string());
    }
    if let Ok(mut states) = draft_restore_state().lock() {
        states.insert(
            command.project_id,
            counters.status_with(remaining, deferred, scan_incomplete),
        );
    }
}

/// 已成功读取的草稿清单显示“无 pending、无事实、无正文”时才允许删除。
///
/// 调用方只能传入 `restore_plan_strict` 成功返回的 plan；读取失败/未知
/// 状态必须在调用前就作为 kept 报告，不能落到这个判断里。
pub(crate) fn draft_dir_is_empty(pending_inputs: usize, fact_count: usize, has_text: bool) -> bool {
    pending_inputs == 0 && fact_count == 0 && !has_text
}
