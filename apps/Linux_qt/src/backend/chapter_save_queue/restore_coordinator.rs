// =============================================================================
// chapter_save_queue/restore_coordinator.rs — 恢复队列的按事务调度
// =============================================================================
//
// Issue #1062 评论 #7213634487 第 B 项：同一章可以同时挂起多份未决草稿。
// 恢复入口按“每轮搬回一份、处理完再搬下一份”推进，直到本轮候选都拿到
// durable 状态；这里集中放队列调度所需的归档事务快照与 materialize 步骤，
// 不按 chapter_id/timestamp 代表当前事务。

use std::path::Path;

use super::kept_draft::RestoreCounters;

/// 本轮把可自动复制的归档事务搬回活动槽位；每章一次一份，其余等后续轮次。
pub(super) fn materialize_pending_retries(
    projects_root: &Path,
    project_id: &str,
    skip: &std::collections::BTreeSet<String>,
    noted: &mut std::collections::BTreeSet<String>,
    counters: &mut RestoreCounters,
) {
    for candidate in super::super::chapter_draft::list_retries(projects_root, project_id) {
        let chapter_id = candidate.marker.chapter_id.clone();
        if skip.contains(&chapter_id) {
            continue;
        }
        match super::super::chapter_draft::materialize_retry_candidate(
            projects_root,
            project_id,
            &chapter_id,
            &candidate,
        ) {
            Ok(super::super::chapter_draft::RetryMaterialize::Moved) => {
                counters.note(&chapter_id, "retry slot materialized");
            }
            Ok(super::super::chapter_draft::RetryMaterialize::ActiveDraftExists) => {
                let key = format!("active:{}", candidate.marker.archived_path);
                if noted.insert(key) {
                    counters.note(
                        &chapter_id,
                        "retry deferred by newer active draft (old copy kept)",
                    );
                }
            }
            Ok(super::super::chapter_draft::RetryMaterialize::AckOnlyPending) => {
                let key = format!("ack:{}", candidate.marker.archived_path);
                if noted.insert(key) {
                    counters.note(
                        &chapter_id,
                        "retry deferred by pending draft ack (old copy kept)",
                    );
                }
            }
            Ok(super::super::chapter_draft::RetryMaterialize::ArchivedMissing) => {
                counters.note(&chapter_id, "retry archived copy missing (marker cleared)");
            }
            Ok(super::super::chapter_draft::RetryMaterialize::NoMarker) => {}
            Err(error) => {
                counters.failed = counters.failed.saturating_add(1);
                counters.note(
                    &chapter_id,
                    format!("retry materialize failed kept: {error}"),
                );
            }
        }
    }
}

/// 章节 -> 本轮可见的归档事务副本（archived_path），用于识别本轮新失败。
pub(super) fn retry_snapshot(
    projects_root: &Path,
    project_id: &str,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    let mut snapshot: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for candidate in super::super::chapter_draft::list_retries(projects_root, project_id) {
        snapshot
            .entry(candidate.marker.chapter_id.clone())
            .or_default()
            .insert(candidate.marker.archived_path);
    }
    snapshot
}
