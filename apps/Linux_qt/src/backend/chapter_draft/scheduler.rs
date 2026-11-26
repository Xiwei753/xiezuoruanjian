// 草稿内存占用与到期落盘调度（Issue #1062 复审第 2 项）。

use std::time::Instant;

use super::store::DraftLocation;
use super::{lock_registry, pending, persist_current, DRAFT_PERSIST_INTERVAL};

/// 内存占用：内存索引事实条数、正文总字节数（不含 projection 历史）。
pub(crate) fn draft_metrics() -> (usize, usize) {
    let registry = lock_registry();
    let mut facts = 0usize;
    let mut bytes = 0usize;
    for state in registry.values() {
        facts = facts.saturating_add(state.facts.len());
        bytes = bytes.saturating_add(state.text.as_ref().map_or(0, String::len));
    }
    (facts, bytes)
}

/// 最早需要落盘的草稿 deadline。
///
/// Issue #1062 复审第 2 项：2s 节流不能只在下次编辑时才重新计算——actor
/// 用它驱动 `recv_timeout`，到期主动写盘，避免“第一次写盘后的第二次更新
/// 停在内存里，停笔后永远不落盘”。
pub(crate) fn next_persist_deadline() -> Option<Instant> {
    let drafts = lock_registry();
    let mut earliest: Option<Instant> = None;
    for state in drafts.values() {
        // failed_inputs 的重试需要 Core API，由下一次保存/恢复负责；这里只
        // 驱动作业内可完成的正文/meta 落盘与失败事实重试。
        if !state.text_dirty && !state.meta_dirty && state.failed_facts.is_empty() {
            continue;
        }
        let deadline = state
            .last_persist
            .map_or_else(Instant::now, |last| last + DRAFT_PERSIST_INTERVAL);
        earliest = Some(earliest.map_or(deadline, |current| current.min(deadline)));
    }
    earliest
}

/// 写入所有已到期的 dirty 草稿（含失败事实重试）；返回成功写入的条数。
pub(crate) fn flush_due() -> usize {
    let now = Instant::now();
    let due: Vec<(std::path::PathBuf, String, String)> = {
        let drafts = lock_registry();
        drafts
            .values()
            .filter(|state| {
                (state.text_dirty || state.meta_dirty || !state.failed_facts.is_empty())
                    && state
                        .last_persist
                        .is_none_or(|last| now.duration_since(last) >= DRAFT_PERSIST_INTERVAL)
            })
            .map(|state| {
                (
                    state.projects_root.clone(),
                    state.project_id.clone(),
                    state.chapter_id.clone(),
                )
            })
            .collect()
    };
    let mut written = 0usize;
    for (root, project_id, chapter_id) in due {
        let Some(location) = DraftLocation::new(&root, &project_id, &chapter_id) else {
            continue;
        };
        if let Err(error) = pending::retry_failed_facts(&location) {
            log::warn!("chapter draft flush retry failed: {project_id}/{chapter_id}: {error}");
        }
        match persist_current(&root, &project_id, &chapter_id, true) {
            Ok(()) => written = written.saturating_add(1),
            Err(error) => {
                log::warn!("chapter draft flush failed: {project_id}/{chapter_id}: {error}");
            }
        }
    }
    written
}
