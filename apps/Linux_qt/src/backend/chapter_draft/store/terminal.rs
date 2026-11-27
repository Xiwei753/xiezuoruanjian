// =============================================================================
// chapter_draft/store/terminal.rs — 结算 tombstone 与目录 GC
// =============================================================================
//
// Issue #1062 评论 7194373427 第 4 项 / 7195690539（附带结算链）：terminal
// 决议是 durable 的结算事实，必须先落 meta 再回收目录；读取端看到 terminal
// 只做可重试 GC，不再把它当成待恢复草稿。

use super::super::DraftState;
use super::{meta_from_state, read_meta_checked, DraftLocation, MetaData};

// ── 结算 tombstone ────────────────────────────────────────────────────────

/// 结算决议的终态 meta：事实/正文都已确认，只保留身份与已提交快照。
///
/// Issue #1062 评论 7194373427 第 4 项：目录回收可能失败，但这份 meta 一旦
/// durable，章节草稿就已经结算完成；读取端不得再把它当成待恢复草稿。
pub(super) fn terminal_meta(location: &DraftLocation, state: &DraftState) -> MetaData {
    let mut meta = meta_from_state(
        location,
        state,
        state.persisted_text_revision,
        &state.persisted_text_sha256,
        "",
    );
    meta.facts.clear();
    meta.acked_event_ids.clear();
    meta.terminal = true;
    meta
}

/// 该目录是否已经写下 terminal 决议（只读 meta，不改状态）。
pub(super) fn is_terminal(location: &DraftLocation) -> bool {
    read_meta_checked(&location.dir, &location.project_id, &location.chapter_id)
        .is_ok_and(|meta| meta.terminal)
}

/// 删除已经结算的目录与旧格式文件。
///
/// 失败必须返回 Err：调用方记录后按“目录待回收”处理，但**不能**把它当成
/// 事务结算失败，也不能据此扣下已经 durable 的 Core ack。
pub(super) fn remove_settled_storage(location: &DraftLocation) -> Result<(), String> {
    if location.dir.exists() {
        std::fs::remove_dir_all(&location.dir).map_err(|error| {
            format!(
                "remove settled chapter draft dir {}: {error}",
                location.dir.display()
            )
        })?;
    }
    let legacy = location.legacy_path();
    if legacy.exists() {
        std::fs::remove_file(&legacy).map_err(|error| {
            format!(
                "remove settled legacy chapter draft {}: {error}",
                legacy.display()
            )
        })?;
    }
    Ok(())
}
