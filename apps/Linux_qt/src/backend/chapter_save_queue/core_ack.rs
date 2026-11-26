// Core journal 草稿确认与本地草稿 ack 的对账（Issue #1062）。
//
// Core 把平台草稿的原始证明事件保存在与正文同一 durable 决议里
// （`draft_ack_projections`）；本地 pending-acks 只是缓存。重启/进入作品时
// 先从 Core 列出未确认记录，按 commit 顺序写成本地 ack 并重放
// `mark_committed`，成功后才回写 Core。跨进程确认事实源始终是 Core journal，
// 本文件只做幂等桥接。

use std::collections::BTreeSet;
use std::path::Path;

use writer_core::WriterCoreApi;

use crate::backend::chapter_draft;

/// 把 Core 中未确认的草稿提交合并进本地 pending-acks，重放并回写 Core。
///
/// `chapter_id` 为 `None` 时对账项目内全部章节（进入作品/恢复草稿路径）；
/// 指定章节时只处理该章（保存路径，在 freeze 之前调用）。
///
/// 返回本次对账涉及的章节数；失败只记诊断，不阻断保存/恢复本身。
pub(super) fn reconcile_core_acks(
    api: &WriterCoreApi,
    projects_root: &str,
    project_id: &str,
    chapter_id: Option<&str>,
) -> usize {
    let root = Path::new(projects_root);
    let acks = match api.list_pending_chapter_draft_acks(project_id.to_string()) {
        Ok(acks) => acks,
        Err(error) => {
            log::warn!("list pending chapter draft acks {project_id}: {error}");
            return 0;
        }
    };
    let mut chapters = BTreeSet::new();
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
            reconciled: false,
        };
        if let Err(error) =
            chapter_draft::record_pending_ack(root, project_id, &ack.chapter_id, &pending)
        {
            log::warn!(
                "record core draft ack {project_id}/{} {}: {error}",
                ack.chapter_id,
                ack.transaction_id
            );
            continue;
        }
        chapters.insert(ack.chapter_id);
    }
    let mut reconciled_chapters = 0usize;
    for chapter in chapters {
        if let Err(error) = chapter_draft::replay_pending_acks(root, project_id, &chapter) {
            log::warn!("replay pending acks {project_id}/{chapter}: {error}");
            continue;
        }
        reconciled_chapters = reconciled_chapters.saturating_add(1);
        flush_reconciled_acks(api, projects_root, project_id, &chapter);
    }
    reconciled_chapters
}

/// 回写某章所有本地已完成、Core 尚未确认的 ack。
pub(super) fn flush_reconciled_acks(
    api: &WriterCoreApi,
    projects_root: &str,
    project_id: &str,
    chapter_id: &str,
) {
    let root = Path::new(projects_root);
    let acks = match chapter_draft::reconciled_acks(root, project_id, chapter_id) {
        Ok(acks) => acks,
        Err(error) => {
            log::warn!("read reconciled acks {project_id}/{chapter_id}: {error}");
            return;
        }
    };
    for (ack, _path) in acks {
        if acknowledge_one(
            api,
            projects_root,
            project_id,
            chapter_id,
            &ack.transaction_id,
            &ack.event_ids,
        ) {
            chapter_draft::complete_pending_ack(root, project_id, chapter_id, &ack.transaction_id);
        }
    }
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
    if acknowledge_one(
        api,
        projects_root,
        project_id,
        chapter_id,
        transaction_id,
        event_ids,
    ) {
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
    _projects_root: &str,
    project_id: &str,
    _chapter_id: &str,
    transaction_id: &str,
    event_ids: &[String],
) -> bool {
    match api.acknowledge_chapter_commit_draft(
        project_id.to_string(),
        transaction_id.to_string(),
        event_ids.to_vec(),
    ) {
        Ok(_) => true,
        Err(error) => {
            log::warn!("acknowledge chapter commit draft {project_id}/{transaction_id}: {error}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #1062 评论 7191076411 第 1 项：Core journal 是草稿 ack 的唯一
    /// 事实源；重启后即使本地没有草稿文件，也要按 Core 未确认记录回写并清理
    /// journal，不能留下永远无法确认的提交。
    #[test]
    fn reconcile_core_acks_finishes_journal_without_local_draft() {
        let temp = tempfile::tempdir().unwrap();
        let projects_root = temp.path().join("projects");
        let project_root = projects_root.join("p1");
        let tx_dir = project_root.join("app-meta/chapter-commits").join("tx-1");
        std::fs::create_dir_all(&tx_dir).unwrap();
        let manifest = serde_json::json!({
            "schemaVersion": 1,
            "transactionId": "tx-1",
            "projectRoot": project_root.to_string_lossy(),
            "key": {"projectId": "p1", "volumeId": "v1", "chapterId": "c1"},
            "mutation": "replace",
            "source": "api",
            "createdAtMs": 1,
            "occurredAtMs": 1,
            "commitSequence": 1,
            "eventIds": ["evt-1"],
            "proofEvents": [],
            "syncProofFacts": [],
            "expected": null,
            "before": {"objectGeneration": "gen-1", "revision": 2, "contentSha256": "before"},
            "after": {"objectGeneration": "gen-1", "revision": 3, "contentSha256": "after"},
            "bodyChanged": true,
            "contentLen": 3,
            "contentMd5": "md5",
            "wordCount": 1,
            "updatedAt": "2026-01-01T00:00:00Z",
            "additionalFiles": [],
            "phase": "applied",
            "pendingProjections": [],
            "draftAckProjections": ["evt-1"]
        });
        std::fs::write(
            tx_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let api = WriterCoreApi::new(temp.path(), projects_root.clone());
        reconcile_core_acks(&api, projects_root.to_str().unwrap(), "p1", None);

        assert!(
            !tx_dir.exists(),
            "Core journal must be cleaned after the platform acknowledged its draft facts"
        );
        let ack_dir = project_root
            .join("app-meta/chapter-drafts/c1")
            .join("pending-acks");
        let leftovers: Vec<_> = std::fs::read_dir(&ack_dir)
            .map(|entries| entries.flatten().collect())
            .unwrap_or_default();
        assert!(
            leftovers.is_empty(),
            "local ack cache must be removed after Core confirmation: {leftovers:?}"
        );
    }
}
