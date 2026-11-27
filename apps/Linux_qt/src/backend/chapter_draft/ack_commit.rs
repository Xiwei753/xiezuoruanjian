// 草稿 ack 决议：把 Core durable receipt 落到本地事实索引/基线/正文。
//
// 拆出以控制 chapter_draft.rs 体积（Issue #1062 复审）；行为与调用方不变。

use super::*;

use std::collections::HashSet;

/// 本次 Core 回执对应的编辑器快照；只有正文仍是这份快照时才收口。
#[derive(Clone, Debug)]
pub(crate) struct TextSettlement {
    pub session_id: u64,
    pub snapshot_revision: u64,
    pub snapshot_digest: String,
}

/// 本地草稿对 Core 草稿 ack 的最终决议。
///
/// Issue #1062 评论 #7209683560 第 1 项：不能让调用方用 bool 猜“成功/没草稿/
/// 已终结”。只有 [`DraftAckDecision::settled`] 为真的决议才允许把 Core ack
/// 标成 reconciled 并回写 Core journal。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DraftAckDecision {
    /// 本地持久决议已写；`removed` 表示整章草稿已结算删除。
    Applied { removed: bool },
    /// 本地没有草稿（目录不存在，或没有可恢复事务内容）。
    NoDraft,
    /// terminal tombstone 已确认：重放同一事务幂等。
    Terminal,
}

impl DraftAckDecision {
    /// 本地已有明确终结/已应用的 durable 决议。
    pub(crate) fn settled(self) -> bool {
        !matches!(self, Self::NoDraft)
    }

    /// 整章草稿目录是否已结算删除。
    pub(crate) fn removed(self) -> bool {
        matches!(self, Self::Applied { removed: true })
    }
}

/// 只结算事实与 base；正文收口由 [`mark_committed_with_settlement`] 承担。
pub(crate) fn mark_committed(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    transaction_id: &str,
    acknowledged: &[String],
    base_generation: &str,
    base_revision: u64,
    base_sha256: &str,
) -> Result<bool, String> {
    mark_committed_with_settlement(
        projects_root,
        project_id,
        chapter_id,
        transaction_id,
        acknowledged,
        base_generation,
        base_revision,
        base_sha256,
        None,
        0,
    )
    .map(DraftAckDecision::removed)
}

/// 同一 meta durable 决议：事实 ack + base 推进 + 精确正文收口（tombstone）。
///
/// Issue #1062 评论 7193847871 第 2 项：正文清除必须与事实 ack 一起先写
/// meta，成功后才能删除 text.txt；meta 写失败返回 Err，调用方不得向 Core
/// 回写 draft ack。
pub(crate) fn mark_committed_with_settlement(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    transaction_id: &str,
    acknowledged: &[String],
    base_generation: &str,
    base_revision: u64,
    base_sha256: &str,
    settlement: Option<&TextSettlement>,
    commit_sequence: u64,
) -> Result<DraftAckDecision, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Ok(DraftAckDecision::NoDraft);
    };
    match ensure_state_result(&location) {
        Ok(true) => {}
        Ok(false) => {
            // Issue #1062 评论 7194373427 第 4 项：terminal tombstone 已经是
            // durable 决议，目录只是等待回收；重放同一事务必须幂等成功，
            // 否则 Core journal 的 ack 会被永久扣下。
            return Ok(if store::is_terminal(&location) {
                DraftAckDecision::Terminal
            } else {
                DraftAckDecision::NoDraft
            });
        }
        Err(error) => return Err(error),
    }
    let acknowledged_set: HashSet<&str> = acknowledged.iter().map(String::as_str).collect();
    // 先在不完整索引上合并事实；失败时保留全部待 GC 事实与 Core durable
    // receipt 对应关系，延后清理，不能基于缺失文件推进 ack 决议。
    let (index, acked) = {
        let drafts = lock_registry();
        let Some(state) = drafts.get(&location.key) else {
            return Ok(DraftAckDecision::NoDraft);
        };
        (
            state.facts.clone(),
            state
                .acked_event_ids
                .iter()
                .cloned()
                .collect::<HashSet<String>>(),
        )
    };
    let merged = store::merge_index(&location.dir, &index, &acked).map_err(|error| {
        format!(
            "chapter draft ack cleanup deferred (facts kept): project={project_id} chapter={chapter_id}: {error}"
        )
    })?;
    let mut remaining = Vec::with_capacity(merged.len());
    let mut removed = Vec::new();
    for fact in merged {
        if acknowledged_set.contains(fact.event_id.as_str()) {
            removed.push(store::fact_file_path(
                &location.dir,
                fact.seq,
                &fact.event_id,
            ));
        } else {
            remaining.push(fact);
        }
    }
    // Issue #1062 复审第 4 项：先把 ack 决议（剩余索引 + 已确认 event ID）
    // 持久化，再删除事实文件。顺序反过来时，两步之间崩溃会让 meta 继续
    // 引用已删文件，下一次 freeze 把已提交事件当成损坏而永久卡住。
    let (empty, meta, previous, settle_text) = {
        let mut drafts = lock_registry();
        let Some(state) = drafts.get_mut(&location.key) else {
            return Ok(DraftAckDecision::NoDraft);
        };
        let previous = PersistedDraftHead {
            facts: state.facts.clone(),
            base_generation: state.base_generation.clone(),
            base_revision: state.base_revision,
            base_sha256: state.base_sha256.clone(),
            acked_event_ids: state.acked_event_ids.clone(),
            acked_through_revision: state.acked_through_revision,
            applied_transactions: state.applied_transactions.clone(),
            applied_commit_sequence: state.applied_commit_sequence,
            text: state.text.clone(),
            text_revision: state.text_revision,
            text_sha256: state.text_sha256.clone(),
            persisted_text_revision: state.persisted_text_revision,
            persisted_text_sha256: state.persisted_text_sha256.clone(),
            persisted_text_blob: state.persisted_text_blob.clone(),
            text_dirty: state.text_dirty,
            committed_text_revision: state.committed_text_revision,
            committed_text_sha256: state.committed_text_sha256.clone(),
            updated_at_ms: state.updated_at_ms,
        };
        let mut acked_ids = state.acked_event_ids.clone();
        for event_id in acknowledged {
            if !acked_ids.iter().any(|existing| existing == event_id) {
                acked_ids.push(event_id.clone());
            }
        }
        let overflow = acked_ids.len().saturating_sub(MAX_MEMORY_FACT_INDEX);
        if overflow > 0 {
            acked_ids.drain(0..overflow);
        }
        state.facts = remaining;
        state.trim_index();
        state.acked_event_ids = acked_ids;
        state.acked_through_revision = state.acked_through_revision.max(base_revision);
        // Issue #1062 评论 7191076411 第 1 项：多笔 ack 只允许同一对象
        // generation 的递增 revision；旧事务重放幂等，不得把 base 降回去。
        // Issue #1062 评论 7194373427 第 3 项：同一事务 ID 或已应用过的
        // commit_sequence 都视为重放；ordering 由项目级序号决定，generation
        // 字符串不参与顺序判断。
        let replayed = (!transaction_id.is_empty()
            && state
                .applied_transactions
                .iter()
                .any(|applied| applied == transaction_id))
            || (commit_sequence != 0 && commit_sequence <= state.applied_commit_sequence);
        let same_object = state.base_generation.is_empty()
            || base_generation.is_empty()
            || state.base_generation == base_generation;
        let advances = !replayed
            && (state.base_generation.is_empty()
                || (same_object && base_revision >= state.base_revision)
                // 外部替换会生成新对象 generation；Core 回执是权威顺序，
                // 但在同一 generation 内绝不接受更小的 revision。
                || !same_object);
        if advances {
            state.base_generation = base_generation.to_string();
            state.base_revision = base_revision;
            state.base_sha256 = base_sha256.to_string();
        }
        if commit_sequence > state.applied_commit_sequence {
            state.applied_commit_sequence = commit_sequence;
        }
        if !transaction_id.is_empty() && !replayed {
            state.applied_transactions.push(transaction_id.to_string());
            let overflow = state
                .applied_transactions
                .len()
                .saturating_sub(MAX_MEMORY_FACT_INDEX);
            if overflow > 0 {
                state.applied_transactions.drain(0..overflow);
            }
        }
        // Issue #1062 评论 7193847871 第 2 项：正文收口与事实 ack 在同一个
        // meta 决议里持久化；只有 meta 写成功后才删除 text.txt，Core journal
        // 的确认也必须等这一步成功。
        let settle_text = settlement.is_some_and(|settlement| {
            state.text.is_some()
                && state.session_id == settlement.session_id
                && state.text_revision == settlement.snapshot_revision
                && (settlement.snapshot_digest.is_empty()
                    || writer_core::writing_stats::proof::content_digest_of_text(
                        state.text.as_deref().unwrap_or_default(),
                    ) == settlement.snapshot_digest)
        });
        if settle_text {
            let settlement = settlement.expect("settlement checked above");
            state.text = None;
            state.text_dirty = false;
            // tombstone：meta 记录已确认快照的 revision/摘要，恢复链据此把
            // 缺失/残留的 text.txt 当作已提交内容，而不是损坏草稿。
            state.persisted_text_revision = settlement.snapshot_revision;
            state.persisted_text_sha256 = settlement.snapshot_digest.clone();
            // tombstone：不再引用任何正文 blob；旧的未确认 blob 在 meta 写成功
            // 后统一回收。
            state.persisted_text_blob = String::new();
            state.committed_text_revision = settlement.snapshot_revision;
            state.committed_text_sha256 = settlement.snapshot_digest.clone();
        }
        state.updated_at_ms = now_ms();
        state.meta_dirty = true;
        // Issue #1062 评论 7191076411 第 3 项：只有锁内状态确认没有剩余的事实
        // 与正文（包括尚未写出/转换的失败事实与待转换 input）时才允许删除
        // 整章草稿；空证明批次不能推断章节草稿为空。
        let empty = state.facts.is_empty()
            && state.failed_facts.is_empty()
            && state.failed_inputs.is_empty()
            && state.text.is_none();
        // Issue #1062 评论 7194373427 第 4 项：即使结算后为空，也先写 terminal
        // tombstone；目录回收是独立、可重试的 GC，不能反过来决定事务是否成立。
        let meta = Some(if empty {
            store::terminal_meta(&location, state)
        } else {
            store::meta_from_state(
                &location,
                state,
                state.persisted_text_revision,
                &state.persisted_text_sha256,
                &state.persisted_text_blob,
            )
        });
        (empty, meta, previous, settle_text)
    };
    if let Some(meta) = meta {
        match store::write_meta(&location.dir, &meta) {
            Ok(()) => {
                if let Some(state) = lock_registry().get_mut(&location.key) {
                    state.meta_dirty = false;
                    state.last_persist = Some(Instant::now());
                }
            }
            Err(error) => {
                if let Some(state) = lock_registry().get_mut(&location.key) {
                    state.facts = previous.facts;
                    state.base_generation = previous.base_generation;
                    state.base_revision = previous.base_revision;
                    state.base_sha256 = previous.base_sha256;
                    state.acked_event_ids = previous.acked_event_ids;
                    state.acked_through_revision = previous.acked_through_revision;
                    state.applied_transactions = previous.applied_transactions;
                    state.applied_commit_sequence = previous.applied_commit_sequence;
                    state.text = previous.text;
                    state.text_revision = previous.text_revision;
                    state.text_sha256 = previous.text_sha256;
                    state.persisted_text_revision = previous.persisted_text_revision;
                    state.persisted_text_sha256 = previous.persisted_text_sha256;
                    state.persisted_text_blob = previous.persisted_text_blob;
                    state.text_dirty = previous.text_dirty;
                    state.committed_text_revision = previous.committed_text_revision;
                    state.committed_text_sha256 = previous.committed_text_sha256;
                    state.updated_at_ms = previous.updated_at_ms;
                    state.meta_dirty = true;
                }
                return Err(format!(
                    "persist draft ack meta failed (facts kept): {project_id}/{chapter_id}: {error}"
                ));
            }
        }
    }
    if empty {
        lock_registry().remove(&location.key);
        if let Err(error) = store::remove_settled_storage(&location) {
            // 决议已 durable；目录回收失败只延迟空间释放，不能扣下 Core ack。
            log::warn!(
                "settled chapter draft gc deferred: project={project_id} chapter={chapter_id}: {error}"
            );
        }
        return Ok(DraftAckDecision::Applied { removed: true });
    }
    if settle_text {
        // meta 已写 tombstone：回收所有未引用正文 blob 与旧 text.txt。
        if let Err(error) = store::gc_text_blobs(&location.dir, None) {
            log::warn!(
                "gc committed chapter draft text deferred: project={project_id} chapter={chapter_id}: {error}"
            );
        }
    }
    // ack 决议已持久化，现在才回收已确认前缀文件。删除失败只会留下孤儿
    // 文件：索引与 acked 记录都已不含它们，后续读取会跳过，不会卡住恢复。
    for path in removed {
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("remove chapter draft fact {}: {error}", path.display());
            }
        }
    }
    Ok(DraftAckDecision::Applied { removed: false })
}

/// mark_committed 写 meta 失败时要恢复的草稿头（索引/基线/ack 记录）。
struct PersistedDraftHead {
    facts: Vec<DraftFactIndex>,
    base_generation: String,
    base_revision: u64,
    base_sha256: String,
    acked_event_ids: Vec<String>,
    acked_through_revision: u64,
    applied_transactions: Vec<String>,
    applied_commit_sequence: u64,
    text: Option<Arc<str>>,
    text_revision: u64,
    text_sha256: String,
    persisted_text_revision: u64,
    persisted_text_sha256: String,
    persisted_text_blob: String,
    text_dirty: bool,
    committed_text_revision: u64,
    committed_text_sha256: String,
    updated_at_ms: i64,
}

/// 正文候选是否已被 durable 证明水位覆盖。
///
/// 正文 revision 不得超过基线 revision 或已记录事实的最大 revision；没有被
/// 证明覆盖的正文只能留在内存里当候选，不能写进可恢复/可提交的 text.txt。
pub(super) fn text_is_verified(state: &DraftState) -> bool {
    // Issue #1062 评论 7192111899 第 1 项：编辑器 revision 与章节持久化
    // revision 是两套坐标（同一章保存过 30 次后，重新打开仍从 0/1 递增），
    // 章节 base_revision 绝不能参与证明水位比较。
    //
    // 1) 有覆盖该快照 revision 的事实：必须同一编辑器会话，且事实记录的事件
    //    终态摘要与快照正文摘要一致。
    if let Some(fact) = state
        .facts
        .iter()
        .find(|fact| fact.revision == state.text_revision)
    {
        if fact.after_text_sha256.is_empty() {
            // 旧格式事实没有事件终态摘要：只能按同一会话的水位判断。
            return fact.session_id == state.session_id;
        }
        return fact.session_id == state.session_id && fact.after_text_sha256 == state.text_sha256;
    }
    if let Some(latest) = state.facts.last() {
        // Issue #1062 评论 7193130307 第 3 项：只有旧格式事实（没有事件终态
        // 摘要）才允许按水位判断；新事实必须存在覆盖该快照 revision 的精确
        // 记录并核对摘要，不能用“后一条事实 revision 更大”猜前一份快照已被
        // 证明。
        return latest.after_text_sha256.is_empty()
            && latest.session_id == state.session_id
            && state.text_revision <= latest.revision;
    }
    // 2) 没有事实：只有仍是本次文档加载基线的正文（revision 未变且摘要一致）
    //    才算“确实没有修改”，可以发布为可提交草稿。
    !state.editor_base_sha256.is_empty()
        && state.text_revision == state.editor_base_revision
        && state.text_sha256 == state.editor_base_sha256
}

/// 该章草稿是否有一份已被证明覆盖、可以发布的正文候选。
///
/// 读内存 registry；没有状态（从未编辑或已被结算删除）时返回 false。
pub(crate) fn text_ready_for_commit(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> bool {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return false;
    };
    let drafts = lock_registry();
    drafts
        .get(&location.key)
        .is_some_and(|state| state.text.is_some() && text_is_verified(state))
}
