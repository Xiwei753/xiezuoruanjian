use super::*;

use super::*;

/// 写入一笔已 durable、等待平台草稿确认的 Core journal 事务。
fn write_pending_journal(project_root: &Path, transaction_id: &str) -> std::path::PathBuf {
    let tx_dir = project_root
        .join("app-meta/chapter-commits")
        .join(transaction_id);
    std::fs::create_dir_all(&tx_dir).unwrap();
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "transactionId": transaction_id,
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
    tx_dir
}

/// Issue #1062 评论 7191076411 第 1 项：Core journal 是草稿 ack 的唯一
/// 事实源；重启后即使本地没有草稿文件，也要按 Core 未确认记录回写并清理
/// journal，不能留下永远无法确认的提交。
#[test]
fn reconcile_core_acks_finishes_journal_without_local_draft() {
    let temp = tempfile::tempdir().unwrap();
    let projects_root = temp.path().join("projects");
    let project_root = projects_root.join("p1");
    let tx_dir = write_pending_journal(&project_root, "tx-1");

    let api = WriterCoreApi::new(temp.path(), projects_root.clone());
    let outcome = reconcile_core_acks(&api, projects_root.to_str().unwrap(), "p1", None)
        .expect("Core journal acks must reconcile without a local draft");
    assert_eq!(outcome.chapters, 1);
    assert!(
        outcome.failures.is_empty(),
        "no reconcile failure expected: {:?}",
        outcome.failures
    );

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

/// 评论 #7192111899 第 2 项：本地 ack 写盘失败时对账必须返回 Err，且
/// Core journal 不能被当作已确认清掉——否则旧事实仍会以旧 base 进入保存链。
#[test]
fn reconcile_failure_blocks_before_freeze() {
    let temp = tempfile::tempdir().unwrap();
    let projects_root = temp.path().join("projects");
    let project_root = projects_root.join("p1");
    let tx_dir = write_pending_journal(&project_root, "tx-1");
    // 草稿章节目录被普通文件占用：create_dir_all(pending-acks) 必然失败。
    let chapter_dir = project_root.join("app-meta/chapter-drafts/c1");
    std::fs::create_dir_all(chapter_dir.parent().unwrap()).unwrap();
    std::fs::write(&chapter_dir, b"not a directory").unwrap();

    let api = WriterCoreApi::new(temp.path(), projects_root.clone());
    let error = reconcile_core_acks(&api, projects_root.to_str().unwrap(), "p1", Some("c1"))
        .expect_err("record_pending_ack failure must fail the reconcile");

    let message = error.message();
    assert!(
        message.contains("tx-1"),
        "error must name the transaction: {message}"
    );
    assert!(
        message.contains("c1"),
        "error must name the chapter: {message}"
    );
    assert_eq!(error.failures().len(), 1, "{:?}", error.failures());
    assert_eq!(error.failures()[0].transaction_id, "tx-1");
    assert_eq!(error.failures()[0].volume_id, "v1");
    assert_eq!(error.failures()[0].chapter_id, "c1");
    assert!(
        tx_dir.exists(),
        "Core journal must stay until the local ack is recorded"
    );
}

/// `list_pending_chapter_draft_acks` 失败时整批对账直接返回 Err，调用方
/// 必须在 freeze 前停下。
#[test]
fn reconcile_list_failure_returns_err() {
    let temp = tempfile::tempdir().unwrap();
    let projects_root = temp.path().join("projects");
    let project_root = projects_root.join("p1");
    std::fs::create_dir_all(project_root.join("app-meta")).unwrap();
    // journal 根目录被普通文件占用：read_dir 返回非 NotFound 错误。
    std::fs::write(
        project_root.join("app-meta/chapter-commits"),
        b"not a directory",
    )
    .unwrap();

    let api = WriterCoreApi::new(temp.path(), projects_root.clone());
    let error = reconcile_core_acks(&api, projects_root.to_str().unwrap(), "p1", None)
        .expect_err("unreadable Core journal must fail the reconcile");

    let message = error.message();
    assert!(
        message.contains("list pending chapter draft acks"),
        "error must name the failing step: {message}"
    );
    assert!(
        message.contains("p1"),
        "error must name the project: {message}"
    );
    assert!(
        error.failures().is_empty(),
        "batch failure has no per-transaction detail: {:?}",
        error.failures()
    );
}
