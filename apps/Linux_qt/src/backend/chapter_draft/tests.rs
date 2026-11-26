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
        0,
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
        "tx-missing",
        &["missing".to_string()],
        "g1",
        1,
        "base-1"
    )
    .expect("ack cleanup"));
    let location = draft_location(root);
    assert!(fact_file_path(&location.dir, 1, "e1").exists());

    assert!(!mark_committed(
        root,
        "p1",
        "c1",
        "tx-1",
        &["e1".to_string()],
        "g1",
        1,
        "base-1"
    )
    .expect("ack cleanup"));
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
        "tx-2",
        &["e2".to_string(), "e3".to_string()],
        "g1",
        3,
        "base-3"
    )
    .expect("ack cleanup"));
    assert!(location.dir.join(TEXT_FILE).exists());
}

/// Issue #1062 评论 7191076411 第 1 项：多笔 ack 只允许同一对象 generation
/// 的递增 revision；旧事务重放幂等，不得把已推进的 base 降回去。
#[test]
fn replayed_or_older_ack_never_regresses_draft_base() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7));
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7));
    persist_current(root, "p1", "c1", true).expect("persist draft");

    assert!(!mark_committed(
        root,
        "p1",
        "c1",
        "tx-1",
        &["e1".to_string()],
        "g1",
        1,
        "base-1"
    )
    .expect("first ack"));
    assert!(
        !mark_committed(root, "p1", "c1", "tx-2", &[], "g1", 2, "base-2")
            .expect("second ack advances base")
    );
    let plan = restore_plan(root, "p1", "c1").expect("restore plan");
    assert_eq!(plan.base_revision, 2);
    assert_eq!(plan.base_sha256, "base-2");

    // 同一事务重放（带更小的旧版本）不得回退 base。
    assert!(
        !mark_committed(root, "p1", "c1", "tx-2", &[], "g1", 1, "base-1")
            .expect("replayed ack is idempotent")
    );
    let plan = restore_plan(root, "p1", "c1").expect("restore plan");
    assert_eq!(plan.base_revision, 2, "replayed ack must not regress base");
    assert_eq!(plan.base_sha256, "base-2");

    // 另一个未见过的事务带更小 revision 也不能回退。
    assert!(
        !mark_committed(root, "p1", "c1", "tx-older", &[], "g1", 1, "base-stale")
            .expect("older ack")
    );
    let plan = restore_plan(root, "p1", "c1").expect("restore plan");
    assert_eq!(
        plan.base_revision, 2,
        "older revision must not regress base"
    );

    // 同 generation 的更大 revision 正常前进。
    assert!(
        !mark_committed(root, "p1", "c1", "tx-3", &[], "g1", 3, "base-3")
            .expect("newer ack advances base")
    );
    let plan = restore_plan(root, "p1", "c1").expect("restore plan");
    assert_eq!(plan.base_revision, 3);
    assert_eq!(plan.base_sha256, "base-3");
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
        "tx-1",
        &["e1".to_string()],
        "g1",
        1,
        "base-1"
    )
    .expect("ack cleanup"));
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
        0,
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
    // 严格入口必须把“读不出来”与“没有草稿”区分开。
    assert!(restore_plan_strict(root, "p1", "c1").is_err());

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
        1,
        "content-v1:deadbeef",
        ("g1", 0, ""),
        0,
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

    assert!(
        !mark_committed(root, "p1", "c1", "tx-1", &["e1".into()], "g1", 1, "sha1")
            .expect("ack cleanup")
    );
    // ack 后剩余事件仍可冻结；重复 ack 同一批不会报错也不会误删后一条。
    let remaining = freeze_batch_for(root, "p1", "c1", 7, "g1", 2).expect("remaining fact");
    assert_eq!(ids(&remaining), vec!["e2"]);
    assert!(
        !mark_committed(root, "p1", "c1", "tx-1", &["e1".into()], "g1", 1, "sha1")
            .expect("ack cleanup")
    );
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
        0,
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

#[test]
fn pending_only_draft_is_listed_after_restart() {
    let workspace = temp_workspace();
    let root = workspace.path();
    // 首次编辑：只有 write-ahead spool 与 meta 事务头，facts/text 还没写出。
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
    forget_for_test(root, "p1", "c1");

    let listed = list_project_drafts(root, "p1");
    let draft = listed
        .iter()
        .find(|draft| draft.chapter_id == "c1")
        .expect("pending-only draft must be listed after restart");
    assert_eq!(draft.fact_count, 0);
    assert!(!draft.text_present);
    assert_eq!(draft.pending_input_count, 1);
    assert!(draft.pending_input_error.is_none());
    // 事务头必须持久化原始 base，恢复不依赖内存状态。
    assert_eq!(draft.base_generation, "g1");
    assert_eq!(draft.base_revision, 1);
    assert_eq!(draft.base_sha256, "base-sha");
}

#[test]
fn corrupt_pending_only_draft_is_counted_and_reported() {
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
    std::fs::write(&pending[0].path, "{ broken").expect("corrupt spool");
    forget_for_test(root, "p1", "c1");

    let listed = list_project_drafts(root, "p1");
    let draft = listed
        .iter()
        .find(|draft| draft.chapter_id == "c1")
        .expect("corrupt pending-only draft must not disappear");
    assert!(draft.pending_input_count >= 1);
    assert!(
        draft
            .pending_input_error
            .as_deref()
            .is_some_and(|error| error.contains("corrupted")),
        "summary must surface the spool error: {:?}",
        draft.pending_input_error
    );
}

#[test]
fn corrupt_fact_with_lagging_meta_is_reported_and_kept() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7)).expect("fact");
    persist_current(root, "p1", "c1", true).expect("persist draft");
    let location = draft_location(root);
    let fact_path = fact_file_path(&location.dir, 1, "e1");
    let original = std::fs::read_to_string(&fact_path).expect("read fact");

    // 模拟 meta 索引滞后（facts=[]）且事实文件损坏，无 text/pending。
    let meta_path = location.dir.join(META_FILE);
    let mut meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).expect("read meta"))
            .expect("meta json");
    meta["facts"] = serde_json::json!([]);
    std::fs::write(&meta_path, meta.to_string()).expect("rewrite meta");
    std::fs::write(&fact_path, "{ broken").expect("corrupt fact");
    forget_for_test(root, "p1", "c1");

    let error = restore_plan_strict(root, "p1", "c1")
        .expect_err("corrupt facts must be reported, not degraded to empty");
    assert!(
        error.contains("unreadable") || error.contains("corrupted"),
        "unexpected error: {error}"
    );
    assert!(fact_path.exists(), "corrupt fact must be kept for evidence");
    assert!(location.dir.exists(), "draft directory must not be removed");
    assert!(
        list_project_drafts(root, "p1")
            .iter()
            .any(|draft| draft.chapter_id == "c1"),
        "recovery list must still discover the chapter"
    );

    // 文件修好后，滞后的 meta 索引仍应自动扫描补齐。
    std::fs::write(&fact_path, original).expect("restore fact");
    forget_for_test(root, "p1", "c1");
    let plan = restore_plan_strict(root, "p1", "c1")
        .expect("readable lagging meta")
        .expect("restore plan");
    assert_eq!(plan.fact_count, 1);
    assert_eq!(plan.event_end_revision, 1);
}

#[test]
fn ack_cleanup_defers_when_fact_index_is_incomplete() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7)).expect("e1");
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7)).expect("e2");
    persist_current(root, "p1", "c1", true).expect("persist draft");
    let location = draft_location(root);
    // 索引里 e2 还在、文件被删：merge_index 必须报错，ack 决议不能推进。
    std::fs::remove_file(fact_file_path(&location.dir, 2, "e2")).expect("remove e2");

    let error = mark_committed(
        root,
        "p1",
        "c1",
        "tx-1",
        &["e1".to_string()],
        "g1",
        1,
        "sha1",
    )
    .expect_err("ack cleanup must defer on incomplete facts");
    assert!(error.contains("deferred"), "unexpected error: {error}");
    assert!(location.dir.exists(), "draft dir must be kept");
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(location.dir.join(META_FILE)).expect("meta"))
            .expect("meta json");
    assert_eq!(
        meta["facts"].as_array().map_or(0, Vec::len),
        2,
        "ack decision must not be written from an incomplete index"
    );
}

#[test]
fn settlement_meta_failure_keeps_text_and_ack_responsibility() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = temp_workspace();
    let root = workspace.path();
    let text = "已提交正文";
    let digest = writer_core::writing_stats::proof::content_digest_of_text(text);
    note_text(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        text.to_string(),
        1,
        &digest,
        ("g1", 1, "base-1"),
        0,
    );
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7)).expect("e1");
    // 保留一条未确认事实，避免“全部 ack + 正文收口”走目录删除的短路分支，
    // 确保本测试真正覆盖 meta 写盘失败。
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7)).expect("e2");
    persist_current(root, "p1", "c1", true).expect("persist draft");
    let location = draft_location(root);
    let previous_mode = std::fs::metadata(&location.dir)
        .expect("draft dir metadata")
        .permissions()
        .mode();
    std::fs::set_permissions(&location.dir, std::fs::Permissions::from_mode(0o500))
        .expect("make draft dir read-only");

    let error = mark_committed_with_settlement(
        root,
        "p1",
        "c1",
        "tx-1",
        &["e1".to_string()],
        "g1",
        2,
        "base-2",
        Some(&TextSettlement {
            session_id: 7,
            snapshot_revision: 1,
            snapshot_digest: digest.clone(),
        }),
    )
    .expect_err("meta write failure must keep the ack decision pending");
    assert!(error.contains("persist draft ack meta failed"), "{error}");
    std::fs::set_permissions(
        &location.dir,
        std::fs::Permissions::from_mode(previous_mode),
    )
    .expect("restore draft dir permissions");

    // meta 未 durable：text.txt 与事实都还在，重放仍能作出同一决议。
    assert!(location.dir.join(TEXT_FILE).exists());
    let plan = restore_plan(root, "p1", "c1").expect("plan after failed settle");
    assert_eq!(plan.text.as_deref(), Some(text));
    assert_eq!(plan.fact_count, 2);

    assert!(
        !mark_committed_with_settlement(
            root,
            "p1",
            "c1",
            "tx-1",
            &["e1".to_string()],
            "g1",
            2,
            "base-2",
            Some(&TextSettlement {
                session_id: 7,
                snapshot_revision: 1,
                snapshot_digest: digest,
            }),
        )
        .expect("retry after meta becomes writable"),
        "还有未确认事实：目录不能删除"
    );
    assert!(!location.dir.join(TEXT_FILE).exists(), "已确认正文已收口");
    let plan = restore_plan(root, "p1", "c1").expect("plan after settled ack");
    assert_eq!(plan.text, None);
    assert_eq!(plan.fact_count, 1);
    assert_eq!(plan.facts[0].event_id, "e2");
}

#[test]
fn pending_ack_replay_advances_base_and_removes_confirmed_prefix() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7)).expect("e1");
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7)).expect("e2");
    persist_current(root, "p1", "c1", true).expect("persist draft");
    record_pending_ack(
        root,
        "p1",
        "c1",
        &PendingAck {
            transaction_id: "tx-1".to_string(),
            event_ids: vec!["e1".to_string()],
            base_generation: "g1".to_string(),
            base_revision: 1,
            base_sha256: "sha1".to_string(),
            editor_session_id: 0,
            snapshot_revision: 0,
            snapshot_digest: String::new(),
            reconciled: false,
        },
    )
    .expect("record ack");
    forget_for_test(root, "p1", "c1");

    assert_eq!(replay_pending_acks(root, "p1", "c1").expect("replay"), 1);
    // 本地 mark_committed 成功只把 ack 标为 reconciled；文件保留到 Core
    // journal 的草稿确认回写成功（见 core_ack）。
    assert_eq!(pending_ack_count(root, "p1", "c1"), 1);
    assert_eq!(
        reconciled_acks(root, "p1", "c1")
            .expect("reconciled acks")
            .len(),
        1
    );
    let location = draft_location(root);
    assert!(!fact_file_path(&location.dir, 1, "e1").exists());
    let plan = restore_plan_strict(root, "p1", "c1")
        .expect("restore plan")
        .expect("draft still has e2");
    assert_eq!(plan.base_revision, 1);
    assert_eq!(plan.fact_count, 1);
    // 幂等：再重放一次只重复本地结算，不产生第二笔提交。
    assert_eq!(
        replay_pending_acks(root, "p1", "c1").expect("replay again"),
        1
    );
    assert_eq!(pending_ack_count(root, "p1", "c1"), 1);
}

#[test]
fn pending_ack_replay_failure_keeps_files_and_blocks_freeze() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7)).expect("e1");
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7)).expect("e2");
    persist_current(root, "p1", "c1", true).expect("persist draft");
    record_pending_ack(
        root,
        "p1",
        "c1",
        &PendingAck {
            transaction_id: "tx-2".to_string(),
            event_ids: vec!["e1".to_string()],
            base_generation: "g1".to_string(),
            base_revision: 1,
            base_sha256: "sha1".to_string(),
            editor_session_id: 0,
            snapshot_revision: 0,
            snapshot_digest: String::new(),
            reconciled: false,
        },
    )
    .expect("record ack");
    let location = draft_location(root);
    std::fs::remove_file(fact_file_path(&location.dir, 2, "e2")).expect("remove e2");
    forget_for_test(root, "p1", "c1");

    assert!(replay_pending_acks(root, "p1", "c1").is_err());
    assert_eq!(pending_ack_count(root, "p1", "c1"), 1);
    assert!(freeze_batch_for(root, "p1", "c1", 7, "g1", 2).is_err());

    // 文件恢复后，freeze 先补做 ack，再只冻结仍保留的 e2。
    std::fs::write(
        fact_file_path(&location.dir, 2, "e2"),
        serde_json::json!({
            "eventId": "e2",
            "revision": 2,
            "sessionId": 7,
            "projection": {"eventId": "e2", "revision": 2},
        })
        .to_string(),
    )
    .expect("restore e2");
    forget_for_test(root, "p1", "c1");
    assert_eq!(pending_ack_count(root, "p1", "c1"), 1);
    let batch = freeze_batch_for(root, "p1", "c1", 7, "g1", 2).expect("freeze after ack replay");
    assert_eq!(ids(&batch), vec!["e2"]);
    // 本地 ack 已完成但 Core 回写还没发生：文件保留，等下一次对账。
    assert_eq!(pending_ack_count(root, "p1", "c1"), 1);
    assert_eq!(
        reconciled_acks(root, "p1", "c1")
            .expect("reconciled acks")
            .len(),
        1
    );
}

#[test]
fn corrupt_meta_draft_is_listed_with_error_and_kept() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7)).expect("fact");
    let location = draft_location(root);
    std::fs::write(location.dir.join(META_FILE), "{ broken meta").expect("corrupt meta");
    let fact_path = fact_file_path(&location.dir, 1, "e1");
    forget_for_test(root, "p1", "c1");

    let listed = list_project_drafts(root, "p1");
    let draft = listed
        .iter()
        .find(|draft| draft.chapter_id == "c1")
        .expect("corrupt meta draft must not disappear");
    assert!(
        draft
            .draft_error
            .as_deref()
            .is_some_and(|error| error.contains("invalid")),
        "summary must surface meta error: {:?}",
        draft.draft_error
    );
    assert_eq!(draft.fact_count, 1);
    assert!(restore_plan_strict(root, "p1", "c1").is_err());
    assert!(location.dir.exists());
    assert!(fact_path.exists());
}

#[test]
fn missing_meta_with_disk_content_is_listed_as_pending_recovery() {
    let workspace = temp_workspace();
    let root = workspace.path();
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e1", 1, 7)).expect("fact");
    let location = draft_location(root);
    std::fs::remove_file(location.dir.join(META_FILE)).expect("remove meta");
    forget_for_test(root, "p1", "c1");

    let listed = list_project_drafts(root, "p1");
    let draft = listed
        .iter()
        .find(|draft| draft.chapter_id == "c1")
        .expect("meta-less draft with facts must be listed");
    assert!(
        draft
            .draft_error
            .as_deref()
            .is_some_and(|error| error.contains("missing")),
        "summary must report missing meta: {:?}",
        draft.draft_error
    );
    assert!(location.dir.exists());
}

#[test]
fn note_text_base_is_initialized_once() {
    let workspace = temp_workspace();
    let root = workspace.path();
    let sha_v1 = writer_core::writing_stats::proof::content_digest_of_text("V1");
    // 首次 note_text 用 QML 上报的 committed base 初始化草稿基线。
    note_text(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        "V1".to_string(),
        1,
        &sha_v1,
        ("g1", 1, "sha-v1"),
        0,
    );
    // 回执还没被 GUI 收到时继续输入：旧 base 不能把已初始化的基线回退。
    let sha_v0 = writer_core::writing_stats::proof::content_digest_of_text("V0");
    note_text(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g0",
        "V0".to_string(),
        0,
        &sha_v0,
        ("g0", 0, ""),
        0,
    );

    let state = snapshot(root, "p1", "c1").expect("draft state");
    assert_eq!(state.base_generation, "g1");
    assert_eq!(state.base_revision, 1);
    assert_eq!(state.base_sha256, "sha-v1");
}

#[test]
fn text_is_persisted_only_after_fact_covers_it() {
    let workspace = temp_workspace();
    let root = workspace.path();
    let text = "第一行\n第二行";
    let sha256 = writer_core::writing_stats::proof::content_digest_of_text(text);
    note_text(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        text.to_string(),
        2,
        &sha256,
        // 章节已经保存过 30 次：持久 revision=30 与编辑器 revision 不是同一
        // 坐标；它不能把编辑器 revision 2 的未证明正文判成“已覆盖”。
        ("g1", 30, "base-sha"),
        0,
    );

    // 还没有覆盖编辑器 revision 2 的事实：正文候选只留在内存里。
    persist_current(root, "p1", "c1", true).expect("persist candidate meta");
    let location = draft_location(root);
    assert!(!location.dir.join(TEXT_FILE).exists());
    assert!(!text_ready_for_commit(root, "p1", "c1"));

    // 证明事实覆盖 revision 2 之后才允许发布整章正文。
    note_fact(root, "p1", "c1", "v1", 7, "g1", fact("e2", 2, 7)).expect("fact");
    assert!(text_ready_for_commit(root, "p1", "c1"));
    persist_current(root, "p1", "c1", true).expect("persist text");
    assert_eq!(
        std::fs::read_to_string(location.dir.join(TEXT_FILE)).expect("text file"),
        text
    );
}

#[test]
fn mark_committed_keeps_text_only_draft() {
    let workspace = temp_workspace();
    let root = workspace.path();
    let text = "已覆盖 base 的正文";
    let sha256 = writer_core::writing_stats::proof::content_digest_of_text(text);
    note_text(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        text.to_string(),
        1,
        &sha256,
        ("g1", 1, "base-1"),
        1,
    );
    persist_current(root, "p1", "c1", true).expect("persist text");
    let location = draft_location(root);
    assert!(location.dir.join(TEXT_FILE).exists());

    // 空证明批次的保存回执只推进 base：草稿里仍有正文时不能删除目录，
    // 否则正在继续输入的新版本会随旧保存回执一起消失。
    assert!(
        !mark_committed(root, "p1", "c1", "tx-2", &[], "g1", 2, "base-2")
            .expect("ack without facts")
    );
    assert!(location.dir.join(TEXT_FILE).exists());
    let plan = restore_plan(root, "p1", "c1").expect("draft kept");
    assert_eq!(plan.text.as_deref(), Some(text));
    assert_eq!(plan.base_revision, 2);
}

/// Issue #1062 评论 7193130307 第 3 项：快照 revision 没有精确事实覆盖时，
/// 不能仅凭“后面的事实 revision 更大”就认定正文已被证明。
#[test]
fn snapshot_without_covering_fact_is_not_published_even_if_watermark_is_higher() {
    let workspace = temp_workspace();
    let root = workspace.path();
    let text = "中间态正文";
    let sha256 = writer_core::writing_stats::proof::content_digest_of_text(text);
    note_text(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        text.to_string(),
        2,
        &sha256,
        ("g1", 30, "base-sha"),
        0,
    );
    let fact_digest = writer_core::writing_stats::proof::content_digest_of_text("事件终态");
    let projection = serde_json::json!({
        "event": {"proof": {"afterTextSha256": fact_digest}},
        "baselineText": "基线"
    });
    assert_eq!(
        projection_after_text_sha256(&projection.to_string()),
        fact_digest
    );
    note_fact(
        root,
        "p1",
        "c1",
        "v1",
        7,
        "g1",
        DraftFact {
            event_id: "e5".to_string(),
            revision: 5,
            session_id: 7,
            projection_json: projection.to_string(),
        },
    )
    .expect("fact");

    assert!(!text_ready_for_commit(root, "p1", "c1"));
    persist_current(root, "p1", "c1", true).expect("persist");
    let location = draft_location(root);
    assert!(
        !location.dir.join(TEXT_FILE).exists(),
        "没有覆盖该 revision 的事实摘要时不得发布正文"
    );
}
