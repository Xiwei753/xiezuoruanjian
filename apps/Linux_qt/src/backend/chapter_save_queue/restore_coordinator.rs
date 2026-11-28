// =============================================================================
// chapter_save_queue/restore_coordinator.rs — 恢复队列的按事务消费
// =============================================================================
//
// Issue #1067 复审第 7 项：恢复入口的队列以真实归档事务（chapter_id +
// marker.archived_path）为单位，`list_retries` 只取一次、按列表顺序逐条
// materialize；每条候选无论成功失败都只消费一次，失败时计数并保留归档原件，
// 不再靠 chapter_id、会话坐标、重新枚举或路径集合增长推断本轮进度。
//
// 活动槽位被同章另一份草稿占着时，由调用方先按真实结算规则处理活动稿，再
// 回来消费这条归档事务；这里只报告槽位占用的种类，不按会话身份去重。

use std::path::Path;

use super::kept_draft::RestoreCounters;

/// 一条归档事务候选在本轮的消费结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MaterializeOutcome {
    /// 已搬回活动槽位：调用方必须立刻处理这一份草稿。
    Moved,
    /// 活动槽位被同章另一份活动草稿占着：调用方先结算活动稿，再回来搬回
    /// 这条归档事务。
    ActiveDraftOccupies,
    /// 活动槽位只剩待对账 ack 文件：调用方先让 Core journal 对账消费 ack，
    /// 再回来搬回这条归档事务。
    PendingAckOccupies,
    /// 本轮已消费但没有可处理的活动草稿（归档丢失、标记缺失或搬运失败）：
    /// 原件保持原样，不回到本轮队列。
    Consumed,
}

/// 消费一条归档事务候选：能搬回就搬回，不能搬回也只影响这一条。
///
/// 每份归档副本有独立的标记文件，同章多份事务各自调用一次即可；失败时
/// 不删除归档目录，只记 counters.failed/note，由下一次进入项目时再试。
/// 槽位占用只报告种类，处理顺序（先结算活动槽位再搬回）由调用方决定。
pub(super) fn materialize_candidate(
    projects_root: &Path,
    project_id: &str,
    candidate: &super::super::chapter_draft::RetryCandidate,
    counters: &mut RestoreCounters,
) -> MaterializeOutcome {
    let chapter_id = candidate.marker.chapter_id.clone();
    match super::super::chapter_draft::materialize_retry_candidate(
        projects_root,
        project_id,
        &chapter_id,
        candidate,
    ) {
        Ok(super::super::chapter_draft::RetryMaterialize::Moved) => {
            counters.note(&chapter_id, "retry slot materialized");
            MaterializeOutcome::Moved
        }
        Ok(super::super::chapter_draft::RetryMaterialize::ActiveDraftExists) => {
            MaterializeOutcome::ActiveDraftOccupies
        }
        Ok(super::super::chapter_draft::RetryMaterialize::AckOnlyPending) => {
            MaterializeOutcome::PendingAckOccupies
        }
        Ok(super::super::chapter_draft::RetryMaterialize::ArchivedMissing) => {
            counters.note(&chapter_id, "retry archived copy missing (marker cleared)");
            MaterializeOutcome::Consumed
        }
        Ok(super::super::chapter_draft::RetryMaterialize::NoMarker) => {
            MaterializeOutcome::Consumed
        }
        Err(error) => {
            counters.failed = counters.failed.saturating_add(1);
            counters.note(&chapter_id, format!("retry materialize failed kept: {error}"));
            MaterializeOutcome::Consumed
        }
    }
}
