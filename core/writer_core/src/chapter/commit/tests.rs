use std::fs;
use std::path::Path;

use chrono::Utc;

use super::super::Chapter;
use super::super::ChapterProofProjection;
use super::journal::{
    journal_root, mark_chapter_commit_draft_ack, mark_pending, md5_hex, pending_chapter_draft_acks,
    read_current, recover_pending_chapter_commits, sha256_hex, write_manifest,
    ChapterCommitManifest, ChapterCommitPhase,
};
use super::{
    commit_chapter_with_guard, ChapterCommitOutcome, ChapterCommitRequest, ChapterCommitSource,
    ChapterKey, ChapterMutationKind, ChapterVersion, BODY_FILE, META_FILE,
};
use crate::writing_stats::{EventSource, Platform, WritingInputEvent};

fn proof_projection(event_id: &str) -> ChapterProofProjection {
    let event = WritingInputEvent::with_identity(
        event_id.to_string(),
        1_000,
        "dev-1",
        Platform::Desktop,
        "desktop",
        "p1",
        "v1",
        "c1",
        EventSource::HumanTyped,
        1,
        0,
        0,
        0,
        0,
        "session-1",
    );
    ChapterProofProjection {
        event,
        baseline_text: Some(String::new()),
    }
}

fn create_test_chapter(root: &Path) -> ChapterKey {
    let volume = "v1";
    let chapter = "c1";
    let chapter_dir = root
        .join("volumes")
        .join(volume)
        .join("chapters")
        .join(chapter);
    fs::create_dir_all(&chapter_dir).unwrap();
    let meta = Chapter {
        id: chapter.to_string(),
        title: "测试".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        order: 0,
        word_count: 0,
        hash: String::new(),
        note: None,
        object_generation: "gen-1".to_string(),
        revision: 0,
        content_sha256: String::new(),
    };
    fs::write(
        chapter_dir.join(META_FILE),
        serde_json::to_string_pretty(&meta).unwrap(),
    )
    .unwrap();
    fs::write(chapter_dir.join(BODY_FILE), "").unwrap();
    ChapterKey::new("p1", volume, chapter)
}

fn commit(root: &Path, key: &ChapterKey, content: &str) -> ChapterCommitOutcome {
    let mut request = ChapterCommitRequest::new(key.clone(), content);
    request.allow_empty_overwrite = true;
    commit_chapter_with_guard(root, None, request).unwrap()
}

#[test]
fn commit_increments_revision_and_rejects_stale_base() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let key = create_test_chapter(root);

    let first = commit(root, &key, "第一版");
    let first = match first {
        ChapterCommitOutcome::Committed(receipt) => receipt,
        ChapterCommitOutcome::Conflict(conflict) => panic!("unexpected conflict: {conflict:?}"),
    };
    assert_eq!(first.before.revision, 0);
    assert_eq!(first.after.revision, 1);
    assert!(first.content_changed);

    let mut stale = ChapterCommitRequest::new(key.clone(), "过期版本");
    stale.expected = Some(first.before.clone());
    let outcome = commit_chapter_with_guard(root, None, stale).unwrap();
    let conflict = match outcome {
        ChapterCommitOutcome::Committed(_) => panic!("expected conflict"),
        ChapterCommitOutcome::Conflict(conflict) => conflict,
    };
    assert_eq!(conflict.actual.revision, 1);

    let (_meta, content, _version) = read_current(root, &key).unwrap();
    assert_eq!(content, "第一版");
}

#[test]
fn commit_is_noop_when_content_and_additional_files_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let key = create_test_chapter(root);
    let _ = commit(root, &key, "同一版");
    let receipt = match commit(root, &key, "同一版") {
        ChapterCommitOutcome::Committed(receipt) => receipt,
        ChapterCommitOutcome::Conflict(conflict) => panic!("unexpected conflict: {conflict:?}"),
    };
    assert!(!receipt.content_changed);
    assert!(receipt.written_files.is_empty());
    assert_eq!(receipt.after.revision, 1);
}

#[test]
fn recovery_applies_decided_body_but_never_overwrites_newer_content() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let key = create_test_chapter(root);
    let first = match commit(root, &key, "A") {
        ChapterCommitOutcome::Committed(receipt) => receipt,
        ChapterCommitOutcome::Conflict(conflict) => panic!("unexpected conflict: {conflict:?}"),
    };

    // 手工制造一个已决议但未发布的 journal。
    let tx_id = "tx-recover";
    let tx_dir = journal_root(root).join(tx_id);
    fs::create_dir_all(&tx_dir).unwrap();
    mark_pending(root).unwrap();
    let after_content = "B";
    crate::storage::atomic_write_bytes(&tx_dir.join(BODY_FILE), after_content.as_bytes()).unwrap();
    let mut manifest = ChapterCommitManifest {
        schema_version: 1,
        transaction_id: tx_id.to_string(),
        project_root: root.to_string_lossy().to_string(),
        key: key.clone(),
        mutation: ChapterMutationKind::Replace,
        source: ChapterCommitSource::Editor,
        created_at_ms: Utc::now().timestamp_millis(),
        occurred_at_ms: Utc::now().timestamp_millis(),
        commit_sequence: 1,
        event_ids: Vec::new(),
        proof_events: Vec::new(),
        sync_proof_facts: Vec::new(),
        expected: None,
        before: first.after.clone(),
        after: ChapterVersion {
            object_generation: first.after.object_generation.clone(),
            revision: first.after.revision + 1,
            content_sha256: sha256_hex(after_content),
        },
        body_changed: true,
        content_len: after_content.len(),
        content_md5: md5_hex(after_content),
        word_count: 1,
        updated_at: Utc::now().to_rfc3339(),
        additional_files: Vec::new(),
        phase: ChapterCommitPhase::Decided,
        pending_projections: Vec::new(),
        draft_ack_projections: Vec::new(),
        created_meta: None,
        meta_title: None,
        meta_order: None,
        created: false,
        deleted: false,
        app_data_root: None,
        trash_path: None,
        superseded_reason: None,
    };
    write_manifest(&tx_dir, &manifest).unwrap();
    recover_pending_chapter_commits(root).unwrap();
    let (_meta, content, version) = read_current(root, &key).unwrap();
    assert_eq!(content, "B");
    assert_eq!(version.revision, 2);

    // 再模拟一个滞留的旧决议：当前已是 B，旧决议写 A 必须被拒绝。
    let stale_dir = journal_root(root).join("tx-stale");
    fs::create_dir_all(&stale_dir).unwrap();
    crate::storage::atomic_write_bytes(&stale_dir.join(BODY_FILE), b"A").unwrap();
    manifest.transaction_id = "tx-stale".to_string();
    manifest.before = first.after.clone();
    manifest.after = first.after.clone();
    manifest.after.content_sha256 = sha256_hex("A");
    manifest.phase = ChapterCommitPhase::Decided;
    write_manifest(&stale_dir, &manifest).unwrap();
    let recovered = recover_pending_chapter_commits(root).unwrap();
    assert!(recovered.iter().any(|item| item.superseded));
    let (_meta, content, _version) = read_current(root, &key).unwrap();
    assert_eq!(content, "B");
}

/// Issue #1062：平台草稿的原始证明事件必须留在同一 durable 决议里，
/// 由草稿精确 ack 后才能清理；Core 不能把它们当作自己的统计投影。
#[test]
fn draft_proof_events_stay_pending_until_platform_ack() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let key = create_test_chapter(root);

    let mut request = ChapterCommitRequest::new(key.clone(), "abc");
    request.allow_empty_overwrite = true;
    request.proof_events = vec![proof_projection("evt-1"), proof_projection("evt-2")];
    let receipt = match commit_chapter_with_guard(root, None, request).unwrap() {
        ChapterCommitOutcome::Committed(receipt) => receipt,
        ChapterCommitOutcome::Conflict(conflict) => panic!("unexpected conflict: {conflict:?}"),
    };
    assert!(
        receipt.pending_projections.is_empty(),
        "draft facts must not be scheduled as Core statistics projections"
    );

    let acks = pending_chapter_draft_acks(root).unwrap();
    assert_eq!(acks.len(), 1);
    assert_eq!(acks[0].transaction_id, receipt.transaction_id);
    assert_eq!(acks[0].event_ids, vec!["evt-1", "evt-2"]);
    assert_eq!(acks[0].after.revision, receipt.after.revision);
    assert!(journal_root(root).join(&receipt.transaction_id).exists());

    // 进程重启扫描：仍保留该事务，等待平台草稿确认。
    let recovered = recover_pending_chapter_commits(root).unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].draft_ack_projections.len(), 2);

    assert!(!mark_chapter_commit_draft_ack(root, &receipt.transaction_id, "evt-1").unwrap());
    assert!(journal_root(root).join(&receipt.transaction_id).exists());
    assert!(mark_chapter_commit_draft_ack(root, &receipt.transaction_id, "evt-2").unwrap());
    assert!(!journal_root(root).join(&receipt.transaction_id).exists());
    assert!(pending_chapter_draft_acks(root).unwrap().is_empty());
}
