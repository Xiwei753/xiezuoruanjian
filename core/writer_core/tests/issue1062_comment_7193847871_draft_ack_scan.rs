//! Issue #1062 评论 7193847871 第 1 项：扫描 journal 时，manifest 损坏或
//! 存在但读不出来必须让 `list_pending_chapter_draft_acks` 返回 Err，不能
//! 静默跳过并返回 Ok 空列表——否则 Core 已 durable 但 manifest 暂时损坏的
//! 事务会从待确认列表消失，Qt `reconcile_core_acks` 会放行下一次保存。
//!
//! `chapter::commit` 是 `pub(crate)`，集成测试经公开 API
//! `WriterCoreApi::list_pending_chapter_draft_acks` 调用同一个扫描函数；
//! 它也是 Qt `reconcile_core_acks` 的真实入口。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use writer_core::api::WriterCoreApi;

const PROJECT_ID: &str = "p1";
/// 与 `core/writer_core/src/chapter/commit.rs` 的 `CONFLICT_DIR` 一致。
const CONFLICT_DIR: &str = "conflicts";

fn projects_root(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().join("projects")
}

fn project_root(temp: &tempfile::TempDir) -> PathBuf {
    projects_root(temp).join(PROJECT_ID)
}

fn journal_root(temp: &tempfile::TempDir) -> PathBuf {
    project_root(temp).join("app-meta/chapter-commits")
}

fn api(temp: &tempfile::TempDir) -> WriterCoreApi {
    WriterCoreApi::new(temp.path(), projects_root(temp))
}

/// 合法 durable manifest；字段形状与 Qt `core_ack/tests.rs` 的 pending
/// journal 一致。
fn manifest_json(
    project_root: &Path,
    transaction_id: &str,
    event_ids: &[&str],
    commit_sequence: u64,
) -> serde_json::Value {
    json!({
        "schemaVersion": 1,
        "transactionId": transaction_id,
        "projectRoot": project_root.to_string_lossy(),
        "key": {"projectId": PROJECT_ID, "volumeId": "v1", "chapterId": "c1"},
        "mutation": "replace",
        "source": "api",
        "createdAtMs": 1,
        "occurredAtMs": 2,
        "commitSequence": commit_sequence,
        "eventIds": event_ids,
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
        "draftAckProjections": event_ids,
    })
}

fn write_manifest(tx_dir: &Path, value: &serde_json::Value) {
    fs::create_dir_all(tx_dir).unwrap();
    fs::write(
        tx_dir.join("manifest.json"),
        serde_json::to_string_pretty(value).unwrap(),
    )
    .unwrap();
}

/// manifest JSON 损坏时必须报错，并保留 journal 目录供诊断。
#[test]
fn corrupt_manifest_fails_instead_of_hiding_pending_acks() {
    let temp = tempfile::tempdir().unwrap();
    let tx_dir = journal_root(&temp).join("tx-corrupt");
    fs::create_dir_all(&tx_dir).unwrap();
    let manifest_path = tx_dir.join("manifest.json");
    fs::write(&manifest_path, "{\"schemaVersion\": 1, \"transactionId\": ").unwrap();

    let error = api(&temp)
        .list_pending_chapter_draft_acks(PROJECT_ID.to_string())
        .expect_err("corrupt durable manifest must not be reported as no pending acks");
    let message = error.to_string();
    assert!(
        message.contains("tx-corrupt"),
        "error must name the transaction directory: {message}"
    );
    assert!(
        message.contains("manifest.json"),
        "error must include the manifest path: {message}"
    );

    assert!(
        tx_dir.exists(),
        "journal directory must be preserved for diagnosis"
    );
    assert!(
        manifest_path.exists(),
        "corrupt manifest must not be deleted by the scan"
    );
}

/// manifest 存在但读不出来（这里是目录占位）也必须报错，不能当成残留材料。
#[test]
fn unreadable_manifest_entry_fails_instead_of_being_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let tx_dir = journal_root(&temp).join("tx-unreadable");
    fs::create_dir_all(tx_dir.join("manifest.json")).unwrap();

    let error = api(&temp)
        .list_pending_chapter_draft_acks(PROJECT_ID.to_string())
        .expect_err("existing but unreadable manifest must fail the scan");
    let message = error.to_string();
    assert!(
        message.contains("tx-unreadable"),
        "error must name the transaction directory: {message}"
    );
    assert!(
        message.contains("manifest.json"),
        "error must include the manifest path: {message}"
    );
}

/// 合法 applied manifest 仍按原逻辑返回，并带上事务与事件 ID。
#[test]
fn valid_applied_manifest_is_listed() {
    let temp = tempfile::tempdir().unwrap();
    let root = project_root(&temp);
    let tx_dir = journal_root(&temp).join("tx-valid");
    write_manifest(
        &tx_dir,
        &manifest_json(&root, "tx-valid", &["evt-1", "evt-2"], 7),
    );

    let acks = api(&temp)
        .list_pending_chapter_draft_acks(PROJECT_ID.to_string())
        .expect("valid applied manifest must be listed");
    assert_eq!(acks.len(), 1, "exactly one pending ack expected: {acks:?}");
    assert_eq!(acks[0].transaction_id, "tx-valid");
    assert_eq!(acks[0].project_id, PROJECT_ID);
    assert_eq!(acks[0].volume_id, "v1");
    assert_eq!(acks[0].chapter_id, "c1");
    assert_eq!(acks[0].event_ids, vec!["evt-1", "evt-2"]);
    assert_eq!(acks[0].commit_sequence, 7);
    assert_eq!(acks[0].after.revision, 3);
}

/// 只有残留材料、没有 manifest 的目录按 Prepared 跳过，且不删除材料。
#[test]
fn residual_directory_without_manifest_is_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let root = journal_root(&temp);
    let tx_dir = root.join("tx-prepared");
    fs::create_dir_all(&tx_dir).unwrap();
    fs::write(tx_dir.join("chapter.md"), "staged body").unwrap();
    // journal 根目录允许有非目录项（例如 pending.marker），扫描必须跳过。
    fs::write(root.join("pending.marker"), "").unwrap();

    let acks = api(&temp)
        .list_pending_chapter_draft_acks(PROJECT_ID.to_string())
        .expect("residual material without manifest must not fail the scan");
    assert!(
        acks.is_empty(),
        "no durable decision, no pending ack: {acks:?}"
    );
    assert!(
        tx_dir.exists(),
        "residual material must be preserved for recovery"
    );
}

/// 冲突目录（`CONFLICT_DIR`）即使含坏 manifest 也跳过，与
/// `recover_pending_chapter_commits` 的目录过滤一致。
#[test]
fn conflict_dir_is_skipped_even_with_corrupt_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let conflict_tx_dir = journal_root(&temp).join(CONFLICT_DIR).join("tx-conflict");
    fs::create_dir_all(&conflict_tx_dir).unwrap();
    fs::write(conflict_tx_dir.join("manifest.json"), "{ not json").unwrap();

    let acks = api(&temp)
        .list_pending_chapter_draft_acks(PROJECT_ID.to_string())
        .expect("conflict material is not a pending commit transaction");
    assert!(
        acks.is_empty(),
        "conflict dir must not contribute pending acks: {acks:?}"
    );
    assert!(conflict_tx_dir.exists());
}
