use super::store::{drafts_root, fact_file_path, META_FILE, TEXT_FILE};
use super::*;

fn temp_workspace() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp workspace")
}

fn draft_location(root: &Path) -> DraftLocation {
    DraftLocation::new(root, "p1", "c1").expect("draft location")
}

fn fact(event_id: &str, revision: u64, session_id: u64) -> DraftFact {
    DraftFact {
        event_id: event_id.to_string(),
        revision,
        session_id,
        projection_json: serde_json::json!({
            "eventId": event_id,
            "revision": revision,
        })
        .to_string(),
    }
}

fn ids(facts: &[LoadedFact]) -> Vec<&str> {
    facts.iter().map(|fact| fact.event_id.as_str()).collect()
}

fn store_text(root: &Path, text: &str, revision: u64) {
    let sha256 = writer_core::writing_stats::proof::content_digest_of_text(text);
    note_text(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        text.to_string(),
        revision,
        &sha256,
        ("g1", 0, ""),
    );
}

#[test]
fn freeze_reads_back_this_session_prefix_after_reload() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 2, 7));
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 3, 7));
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e3", 5, 7));
    store_text(root, "第一行\n第二行", 5);
    persist_current(root, "p1", "c1", true).expect("persist draft");

    // 模拟进程重启：丢掉内存 registry，只靠磁盘恢复。
    forget_for_test(root, "p1", "c1");
    let prefix = freeze_batch_for(root, "p1", "c1", 7, "g1", 3).expect("freeze prefix");
    assert_eq!(ids(&prefix), vec!["e1", "e2"]);
    assert!(prefix.iter().all(|fact| fact.revision <= 3));

    let all = freeze_batch_for(root, "p1", "c1", 7, "g1", 5).expect("freeze all");
    assert_eq!(ids(&all), vec!["e1", "e2", "e3"]);

    // generation 不同的冻结不能把本会话事实当成别人的前缀。
    assert!(freeze_batch_for(root, "p1", "c1", 7, "g2", 5).is_err());
}

#[test]
fn freeze_without_snapshot_keeps_newer_facts() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 2, 7));
    persist_current(root, "p1", "c1", true).expect("persist draft");
    forget_for_test(root, "p1", "c1");

    // 快照缺失（0）时不能返回空批次，否则保存成功后调用方会删掉草稿。
    let error =
        freeze_batch_for(root, "p1", "c1", 7, "g1", 0).expect_err("snapshot must not drop facts");
    assert!(
        error.contains("beyond snapshot"),
        "error must explain the snapshot gap: {error}"
    );
    let location = draft_location(root);
    assert!(fact_file_path(&location.dir, 1, "e1").exists());
}

#[test]
fn mixed_session_facts_are_not_silently_dropped() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 1, "g1", fact("e1", 1, 1));
    note_fact(root, "p1", "c1", "v1", 2, "g1", fact("e2", 2, 2));
    persist_current(root, "p1", "c1", true).expect("persist draft");
    forget_for_test(root, "p1", "c1");

    let error = freeze_batch_for(root, "p1", "c1", 2, "g1", 10)
        .expect_err("mixed sessions must not freeze silently");
    assert_eq!(error, MIXED_SESSION_ERROR);
    // 另一会话的旧事实仍在磁盘上，没有被丢弃。
    let location = draft_location(root);
    assert!(fact_file_path(&location.dir, 1, "e1").exists());
    assert!(fact_file_path(&location.dir, 2, "e2").exists());
}

#[test]
fn mark_committed_removes_only_the_acknowledged_prefix() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7));
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7));
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e3", 3, 7));
    store_text(root, "abc", 3);
    persist_current(root, "p1", "c1", true).expect("persist draft");

    // 索引里不存在的 id 直接忽略，不能报错也不能删多余文件。
    assert!(!mark_committed(
        root,
        "p1",
        "c1",
        &["missing".to_string()],
        "g1",
        1,
        "base-1"
    ));
    let location = draft_location(root);
    assert!(fact_file_path(&location.dir, 1, "e1").exists());

    assert!(!mark_committed(
        root,
        "p1",
        "c1",
        &["e1".to_string()],
        "g1",
        1,
        "base-1"
    ));
    assert!(!fact_file_path(&location.dir, 1, "e1").exists());
    assert!(fact_file_path(&location.dir, 2, "e2").exists());
    assert!(fact_file_path(&location.dir, 3, "e3").exists());

    forget_for_test(root, "p1", "c1");
    let remaining = freeze_batch_for(root, "p1", "c1", 7, "g1", 3).expect("remaining facts");
    assert_eq!(ids(&remaining), vec!["e2", "e3"]);

    let plan = restore_plan(root, "p1", "c1").expect("restore plan");
    assert_eq!(plan.base_generation, "g1");
    assert_eq!(plan.base_revision, 1);
    assert_eq!(plan.base_sha256, "base-1");

    // 正文还在：即使事实全部 ack，草稿也不能整个删除。
    assert!(!mark_committed(
        root,
        "p1",
        "c1",
        &["e2".to_string(), "e3".to_string()],
        "g1",
        3,
        "base-3"
    ));
    assert!(location.dir.join(TEXT_FILE).exists());
}

#[test]
fn mark_committed_removes_fact_only_draft_directory() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7));
    persist_current(root, "p1", "c1", true).expect("persist draft");
    let location = draft_location(root);
    assert!(location.dir.exists());

    assert!(mark_committed(
        root,
        "p1",
        "c1",
        &["e1".to_string()],
        "g1",
        1,
        "base-1"
    ));
    assert!(!location.dir.exists());
    assert!(snapshot(root, "p1", "c1").is_none());
}

#[test]
fn restore_plan_returns_draft_base_and_batch_json() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 2, 7));
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 3, 7));
    let sha256 = writer_core::writing_stats::proof::content_digest_of_text("草稿正文");
    note_text(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        "草稿正文".to_string(),
        3,
        &sha256,
        ("generation-base", 9, "sha-base"),
    );
    persist_current(root, "p1", "c1", true).expect("persist draft");
    forget_for_test(root, "p1", "c1");

    let plan = restore_plan(root, "p1", "c1").expect("restore plan");
    assert_eq!(plan.volume_id, "v1");
    assert_eq!(plan.base_generation, "generation-base");
    assert_eq!(plan.base_revision, 9);
    assert_eq!(plan.base_sha256, "sha-base");
    assert_eq!(plan.text.as_deref(), Some("草稿正文"));
    assert_eq!(plan.text_revision, 3);
    assert_eq!(plan.text_sha256, sha256);
    assert_eq!(plan.fact_count, 2);
    assert!(!plan.mixed_sessions);
    assert_eq!(plan.session_summary(), "session=7 generation=g1");
    let values: Vec<serde_json::Value> = serde_json::from_str(&plan.batch_json).expect("batch");
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["eventId"], "e1");
    assert_eq!(values[1]["eventId"], "e2");
}

#[test]
fn missing_or_corrupt_fact_file_fails_freeze_and_restore() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7));
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7));
    persist_current(root, "p1", "c1", true).expect("persist draft");
    forget_for_test(root, "p1", "c1");
    let location = draft_location(root);
    let path = fact_file_path(&location.dir, 2, "e2");

    std::fs::remove_file(&path).expect("remove fact file");
    let error = freeze_batch_for(root, "p1", "c1", 7, "g1", 2).expect_err("missing fact must fail");
    assert!(
        error.contains("e2"),
        "error must name the event and path: {error}"
    );
    assert!(restore_plan(root, "p1", "c1").is_none());

    std::fs::write(&path, "{ this is not json").expect("corrupt fact file");
    forget_for_test(root, "p1", "c1");
    let error = freeze_batch_for(root, "p1", "c1", 7, "g1", 2).expect_err("corrupt fact must fail");
    assert!(
        error.contains("corrupted"),
        "error must name corruption: {error}"
    );
}

#[test]
fn restore_plan_rejects_corrupt_text_and_keeps_files() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7));
    note_text(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        "actual text".to_string(),
        2,
        "content-v1:deadbeef",
        ("g1", 0, ""),
    );
    persist_current(root, "p1", "c1", true).expect("persist draft");
    forget_for_test(root, "p1", "c1");

    assert!(restore_plan(root, "p1", "c1").is_none());
    let location = draft_location(root);
    assert!(location.dir.join(TEXT_FILE).exists());
    assert!(location.dir.join(META_FILE).exists());
    assert!(fact_file_path(&location.dir, 1, "e1").exists());
}

#[test]
fn freeze_and_restore_recover_facts_when_meta_index_lags() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7));
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7));
    persist_current(root, "p1", "c1", true).expect("persist draft");
    let location = draft_location(root);

    // 模拟 meta 索引只写到 2 秒前：清空索引但事实文件仍在。
    let meta_path = location.dir.join(META_FILE);
    let meta = std::fs::read_to_string(&meta_path).expect("read meta");
    let mut value: serde_json::Value = serde_json::from_str(&meta).expect("meta json");
    value["facts"] = serde_json::json!([]);
    std::fs::write(&meta_path, value.to_string()).expect("rewrite meta");
    forget_for_test(root, "p1", "c1");

    let batch = freeze_batch_for(root, "p1", "c1", 7, "g1", 2).expect("recover from facts dir");
    assert_eq!(ids(&batch), vec!["e1", "e2"]);

    forget_for_test(root, "p1", "c1");
    let plan = restore_plan(root, "p1", "c1").expect("restore plan from facts dir");
    assert_eq!(plan.fact_count, 2);
}

#[test]
fn legacy_single_file_draft_migrates_on_read() {
    let workspace = temp_workspace();
    let root = workspace.path();
    let text = "legacy draft text";
    let sha256 = writer_core::writing_stats::proof::content_digest_of_text(text);
    let legacy = serde_json::json!({
        "schemaVersion": 1,
        "projectId": "p1",
        "volumeId": "v1",
        "chapterId": "c1",
        "baseGeneration": "legacy-generation",
        "baseRevision": 4,
        "baseSha256": "legacy-sha",
        "textRevision": 5,
        "textSha256": sha256,
        "text": text,
        "facts": [{
            "eventId": "legacy-event",
            "revision": 5,
            "projection": { "eventId": "legacy-event" }
        }],
        "updatedAtMs": 1,
    });
    let drafts_root = drafts_root(root, "p1");
    std::fs::create_dir_all(&drafts_root).expect("create drafts root");
    let legacy_path = drafts_root.join("c1.json");
    std::fs::write(&legacy_path, legacy.to_string()).expect("write legacy draft");

    let listed = list_project_drafts(root, "p1");
    assert!(listed
        .iter()
        .any(|draft| draft.chapter_id == "c1" && draft.fact_count == 1 && draft.text_present));

    let plan = restore_plan(root, "p1", "c1").expect("migrated restore plan");
    assert_eq!(plan.base_generation, "legacy-generation");
    assert_eq!(plan.base_revision, 4);
    assert_eq!(plan.base_sha256, "legacy-sha");
    assert_eq!(plan.text.as_deref(), Some(text));
    assert_eq!(plan.fact_count, 1);
    let values: Vec<serde_json::Value> = serde_json::from_str(&plan.batch_json).expect("batch");
    assert_eq!(values[0]["eventId"], "legacy-event");
    assert!(!legacy_path.exists());
    assert!(drafts_root.join("c1").join(META_FILE).exists());
}

#[test]
fn draft_metrics_counts_index_and_text_bytes_only() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7));
    store_text(root, "hello", 1);
    persist_current(root, "p1", "c1", true).expect("persist draft");
    let (facts, bytes) = draft_metrics();
    assert!(facts >= 1);
    assert!(bytes >= 5);
}

#[test]
fn freeze_rejects_snapshot_ahead_of_last_event() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 2, 7));
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 3, 7));
    persist_current(root, "p1", "c1", true).expect("persist draft");

    // 快照 revision 4 之前发生过编辑，但事件链只到 3：不能把“找不到”
    // 当成“本来没有”而带着不完整证明提交正文。
    let error = freeze_batch_for(root, "p1", "c1", 7, "g1", 4).expect_err("gap must fail");
    assert!(error.contains("not current"), "unexpected error: {error}");
    assert_eq!(
        ids(&freeze_batch_for(root, "p1", "c1", 7, "g1", 3).expect("exact snapshot")),
        vec!["e1", "e2"]
    );
}

#[test]
fn failed_fact_write_blocks_snapshot_until_retry_succeeds() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7)).expect("first fact");
    persist_current(root, "p1", "c1", true).expect("persist draft");
    let location = draft_location(root);
    // 目标路径被目录占住，原子的写盘 rename 会失败。
    let blocked = fact_file_path(&location.dir, 2, "e2");
    std::fs::create_dir_all(&blocked).expect("occupy fact path");

    let error = note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7))
        .expect_err("fact write must fail");
    assert!(error.contains("write draft fact failed"), "{error}");
    let blocked_error =
        freeze_batch_for(root, "p1", "c1", 7, "g1", 2).expect_err("failed fact blocks snapshot");
    assert!(
        blocked_error.contains("retry draft fact failed")
            || blocked_error.contains("persistence failed"),
        "unexpected error: {blocked_error}"
    );

    // 故障解除后冻结前会重试失败事件，并把批次补齐。
    std::fs::remove_dir_all(&blocked).expect("clear blocked path");
    let batch = freeze_batch_for(root, "p1", "c1", 7, "g1", 2).expect("retry recovers");
    assert_eq!(ids(&batch), vec!["e1", "e2"]);
}

#[test]
fn mark_committed_is_recoverable_and_idempotent() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7)).expect("e1");
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7)).expect("e2");
    persist_current(root, "p1", "c1", true).expect("persist draft");
    let location = draft_location(root);

    assert!(!mark_committed(
        root,
        "p1",
        "c1",
        &["e1".into()],
        "g1",
        1,
        "sha1"
    ));
    // ack 后剩余事件仍可冻结；重复 ack 同一批不会报错也不会误删后一条。
    let remaining = freeze_batch_for(root, "p1", "c1", 7, "g1", 2).expect("remaining fact");
    assert_eq!(ids(&remaining), vec!["e2"]);
    assert!(!mark_committed(
        root,
        "p1",
        "c1",
        &["e1".into()],
        "g1",
        1,
        "sha1"
    ));
    let remaining = freeze_batch_for(root, "p1", "c1", 7, "g1", 2).expect("remaining fact again");
    assert_eq!(ids(&remaining), vec!["e2"]);
    assert!(fact_file_path(&location.dir, 2, "e2").exists());
    // 已确认的 e1 文件已删除，但 meta 的 acked 记录让缺失文件不再报错。
    let plan = restore_plan(root, "p1", "c1").expect("restore plan after ack");
    assert_eq!(plan.event_end_revision, 2);
    assert_eq!(plan.facts.len(), 1);
}

#[test]
fn pending_fact_inputs_round_trip() {
    let workspace = temp_workspace();
    let root = workspace.path();
    spool_failed_input(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        2,
        "e1",
        r#"{"eventId":"e1","newRevision":2}"#,
        ("g1", 1, "base-sha"),
    )
    .expect("spool input");
    let pending = pending_inputs(root, "p1", "c1").expect("read spool");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].event_id, "e1");
    assert_eq!(pending[0].revision, 2);
    assert_eq!(pending[0].base_revision, 1);
    assert!(pending[0].input_json.contains("newRevision"));
    // 未重试成功前，该 revision 的正文快照不可提交。
    let error = freeze_batch_for(root, "p1", "c1", 7, "g1", 2).expect_err("pending input blocks");
    assert!(error.contains("persistence failed"), "{error}");
    clear_pending_input(root, "p1", "c1", "e1");
    assert!(pending_inputs(root, "p1", "c1")
        .expect("read spool")
        .is_empty());
    assert!(freeze_batch_for(root, "p1", "c1", 7, "g1", 2).is_ok());
}

#[test]
fn corrupt_pending_input_blocks_saves_across_restart() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_text(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        "正文".to_string(),
        2,
        &writer_core::writing_stats::proof::content_digest_of_text("正文"),
        ("g1", 1, "base-sha"),
    );
    spool_failed_input(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        2,
        "e1",
        r#"{"eventId":"e1","newRevision":2}"#,
        ("g1", 1, "base-sha"),
    )
    .expect("spool input");
    let pending = pending_inputs(root, "p1", "c1").expect("read spool");
    let path = pending[0].path.clone();
    std::fs::write(&path, "{ not valid json").expect("corrupt spool");
    // 模拟重启：内存失败集合清空，只剩磁盘事实。
    forget_for_test(root, "p1", "c1");
    let error = pending_inputs(root, "p1", "c1").expect_err("corrupt spool must be reported");
    assert!(error.contains("corrupted"), "{error}");
    let error = freeze_batch_for(root, "p1", "c1", 7, "g1", 2)
        .expect_err("corrupt spool must block the snapshot");
    assert!(error.contains("corrupted"), "{error}");
    std::fs::remove_file(&path).expect("remove spool");
    forget_for_test(root, "p1", "c1");
    assert!(freeze_batch_for(root, "p1", "c1", 7, "g1", 2).is_ok());
}

#[test]
fn first_fact_writes_meta_header_and_stays_discoverable() {
    let workspace = temp_workspace();
    let root = workspace.path();
    // 首次事件：此前没有 note_text / meta.json。
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 2, 7)).expect("first fact");
    let location = draft_location(root);
    assert!(
        location.dir.join(META_FILE).exists(),
        "首次事实前必须写出 meta 事务头"
    );
    // 模拟进程崩溃后重启：恢复列表与恢复计划都必须发现这批事件。
    forget_for_test(root, "p1", "c1");
    let listed = list_project_drafts(root, "p1");
    assert!(
        listed
            .iter()
            .any(|draft| draft.chapter_id == "c1" && draft.fact_count == 1),
        "meta 事务头必须让恢复列表发现草稿"
    );
    let plan = restore_plan(root, "p1", "c1").expect("restore plan after restart");
    assert_eq!(plan.event_end_revision, 2);
}
