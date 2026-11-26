// Core journal 草稿确认与本地草稿 ack 的对账（Issue #1062）。
//
// Core 把平台草稿的原始证明事件保存在与正文同一 durable 决议里
// （`draft_ack_projections`）；本地 pending-acks 只是缓存。重启/进入作品时
// 先从 Core 列出未确认记录，按 commit 顺序写成本地 ack 并重放
// `mark_committed`，成功后才回写 Core。跨进程确认事实源始终是 Core journal，
// 本文件只做幂等桥接。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use writer_core::WriterCoreApi;

use crate::backend::chapter_draft;

/// 单笔草稿 ack 对账失败明细。
///
/// `transaction_id` 为空表示失败发生在章节级步骤（重放/读取本地 ack），
/// 无法归属到某一笔 Core journal 事务。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReconcileFailure {
    pub transaction_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub reason: String,
}

/// 一次对账的汇总。
#[derive(Clone, Debug, Default)]
pub(super) struct ReconcileOutcome {
    /// 完成 replay 的章节数。
    pub chapters: usize,
    /// 失败明细；只在 `Ok` 时为空（有失败时随 `ReconcileError` 返回）。
    pub failures: Vec<ReconcileFailure>,
}

/// 对账失败：`list_pending_chapter_draft_acks` 整批失败时 `failures` 为空，
/// 逐笔失败时 `failures` 列出全部 transaction/chapter，`message` 是完整文本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReconcileError {
    failures: Vec<ReconcileFailure>,
    message: String,
}

impl ReconcileError {
    fn batch(message: String) -> Self {
        Self {
            failures: Vec::new(),
            message,
        }
    }

    fn from_failures(
        project_id: &str,
        chapter_id: Option<&str>,
        failures: Vec<ReconcileFailure>,
    ) -> Self {
        let mut message = format!(
            "core draft ack reconcile failed: project={project_id} chapter={} failures={}",
            chapter_id.unwrap_or("*"),
            failures.len()
        );
        for failure in &failures {
            message.push_str(&format!(
                "; tx={} volume={} chapter={}: {}",
                if failure.transaction_id.is_empty() {
                    "-"
                } else {
                    failure.transaction_id.as_str()
                },
                failure.volume_id.as_str(),
                failure.chapter_id.as_str(),
                failure.reason
            ));
        }
        Self { failures, message }
    }

    pub(super) fn message(&self) -> &str {
        &self.message
    }

    pub(super) fn failures(&self) -> &[ReconcileFailure] {
        &self.failures
    }
}

impl fmt::Display for ReconcileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ReconcileError {}

/// 把 Core 中未确认的草稿提交合并进本地 pending-acks，重放并回写 Core。
///
/// `chapter_id` 为 `None` 时对账项目内全部章节（进入作品/恢复草稿路径）；
/// 指定章节时只处理该章（保存路径，在 freeze 之前调用）。
///
/// 任何一笔 ack 没能落到本地/回写 Core 都返回 `Err`：此时 Core journal 仍
/// 保留原始证明事件，调用方（保存链）必须在对账完成前不 freeze、不提交，
/// 否则旧事实会以旧 base 再次进入证明批次。
pub(super) fn reconcile_core_acks(
    api: &WriterCoreApi,
    projects_root: &str,
    project_id: &str,
    chapter_id: Option<&str>,
) -> Result<ReconcileOutcome, ReconcileError> {
    let root = Path::new(projects_root);
    let acks = match api.list_pending_chapter_draft_acks(project_id.to_string()) {
        Ok(acks) => acks,
        Err(error) => {
            let message = format!(
                "list pending chapter draft acks: project={project_id} chapter={}: {error}",
                chapter_id.unwrap_or("*")
            );
            log::warn!("{message}");
            return Err(ReconcileError::batch(message));
        }
    };
    let mut outcome = ReconcileOutcome::default();
    // 章节 -> 该章事务的卷 ID（失败明细需要卷/章定位）。
    let mut chapters: BTreeMap<String, String> = BTreeMap::new();
    // 有 ack 写盘失败或重放失败的章节：整章不再 replay/放行，避免只结算
    // 一部分就进入保存。
    let mut blocked: BTreeSet<String> = BTreeSet::new();
    for ack in acks {
        if let Some(filter) = chapter_id {
            if ack.chapter_id != filter {
                continue;
            }
        }
        let pending = chapter_draft::PendingAck {
            transaction_id: ack.transaction_id.clone(),
            event_ids: ack.event_ids.clone(),
            base_generation: ack.after.object_generation.clone(),
            base_revision: ack.after.revision,
            base_sha256: ack.after.content_sha256.clone(),
            // Core journal 只保存章节版本坐标，不含平台编辑器快照；这里的
            // ack 只结算事实/base，正文是否已提交由恢复链按 live 摘要确认。
            editor_session_id: 0,
            snapshot_revision: 0,
            snapshot_digest: String::new(),
            commit_sequence: ack.commit_sequence,
            reconciled: false,
        };
        match chapter_draft::record_pending_ack(root, project_id, &ack.chapter_id, &pending) {
            Ok(()) => {
                chapters
                    .entry(ack.chapter_id.clone())
                    .or_insert_with(|| ack.volume_id.clone());
            }
            Err(error) => {
                blocked.insert(ack.chapter_id.clone());
                outcome.failures.push(ReconcileFailure {
                    transaction_id: ack.transaction_id.clone(),
                    volume_id: ack.volume_id.clone(),
                    chapter_id: ack.chapter_id.clone(),
                    reason: format!("record pending ack (kept): {error}"),
                });
            }
        }
    }
    for (chapter, volume_id) in chapters {
        if blocked.contains(&chapter) {
            continue;
        }
        if let Err(error) = chapter_draft::replay_pending_acks(root, project_id, &chapter) {
            outcome.failures.push(ReconcileFailure {
                transaction_id: String::new(),
                volume_id: volume_id.clone(),
                chapter_id: chapter.clone(),
                reason: format!("replay pending acks (kept): {error}"),
            });
            continue;
        }
        outcome.chapters = outcome.chapters.saturating_add(1);
        outcome.failures.extend(flush_reconciled_acks(
            api,
            projects_root,
            project_id,
            &chapter,
            &volume_id,
        ));
    }
    if outcome.failures.is_empty() {
        return Ok(outcome);
    }
    let error = ReconcileError::from_failures(project_id, chapter_id, outcome.failures);
    log::warn!("{error}");
    Err(error)
}

/// 回写某章所有本地已完成、Core 尚未确认的 ack。
///
/// 返回失败明细（空表示全部回写成功）；回写失败的本地 ack 文件保留，下一次
/// 对账重试。调用方不得在失败时继续 freeze/提交本批保存。
pub(super) fn flush_reconciled_acks(
    api: &WriterCoreApi,
    projects_root: &str,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
) -> Vec<ReconcileFailure> {
    let root = Path::new(projects_root);
    let acks = match chapter_draft::reconciled_acks(root, project_id, chapter_id) {
        Ok(acks) => acks,
        Err(error) => {
            return vec![ReconcileFailure {
                transaction_id: String::new(),
                volume_id: volume_id.to_string(),
                chapter_id: chapter_id.to_string(),
                reason: format!("read reconciled acks: {error}"),
            }];
        }
    };
    let mut failures = Vec::new();
    for (ack, _path) in acks {
        match acknowledge_one(api, project_id, &ack.transaction_id, &ack.event_ids) {
            Ok(()) => {
                chapter_draft::complete_pending_ack(
                    root,
                    project_id,
                    chapter_id,
                    &ack.transaction_id,
                );
            }
            Err(error) => {
                // 本地 ack 文件保留：Core journal 未确认，下一次对账必须重试。
                failures.push(ReconcileFailure {
                    transaction_id: ack.transaction_id.clone(),
                    volume_id: volume_id.to_string(),
                    chapter_id: chapter_id.to_string(),
                    reason: format!("acknowledge chapter commit draft (kept): {error}"),
                });
            }
        }
    }
    failures
}

/// 回写单笔 Core 草稿确认；成功后删除本地 ack 文件。
///
/// 返回 `true` 表示 Core 已确认（或该 journal 已不存在，属于已清理的幂等
/// 重放），本地文件已删除。
pub(super) fn acknowledge_core_transaction(
    api: &WriterCoreApi,
    projects_root: &str,
    project_id: &str,
    chapter_id: &str,
    transaction_id: &str,
    event_ids: &[String],
) -> bool {
    if event_ids.is_empty() {
        return false;
    }
    chapter_draft::reconciled_ack(
        Path::new(projects_root),
        project_id,
        chapter_id,
        transaction_id,
    );
    if acknowledge_one(api, project_id, transaction_id, event_ids).is_ok() {
        chapter_draft::complete_pending_ack(
            Path::new(projects_root),
            project_id,
            chapter_id,
            transaction_id,
        );
        true
    } else {
        false
    }
}

fn acknowledge_one(
    api: &WriterCoreApi,
    project_id: &str,
    transaction_id: &str,
    event_ids: &[String],
) -> Result<(), String> {
    api.acknowledge_chapter_commit_draft(
        project_id.to_string(),
        transaction_id.to_string(),
        event_ids.to_vec(),
    )
    .map(|_| ())
    .map_err(|error| {
        log::warn!("acknowledge chapter commit draft {project_id}/{transaction_id}: {error}");
        error.to_string()
    })
}

#[cfg(test)]
mod tests;
