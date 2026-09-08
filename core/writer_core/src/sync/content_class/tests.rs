use super::*;

#[test]
fn classify_document_paths() {
    assert_eq!(
        classify_content_path("volumes/v1/chapters/c1/chapter.md"),
        ContentClass::UserTextDocument
    );
    assert_eq!(
        classify_content_path("note.md"),
        ContentClass::UserTextDocument
    );
    assert_eq!(
        classify_content_path("volumes/v1/outline.md"),
        ContentClass::UserTextDocument
    );
}

#[test]
fn classify_metadata_paths() {
    assert_eq!(
        classify_content_path("project.json"),
        ContentClass::Metadata
    );
    assert_eq!(
        classify_content_path("volumes/v1/volume.json"),
        ContentClass::Metadata
    );
}

#[test]
fn classify_local_only_paths() {
    assert_eq!(
        classify_content_path("backups/x.md"),
        ContentClass::LocalOnly
    );
    assert_eq!(
        classify_content_path("app-meta/sync/state.local.json"),
        ContentClass::LocalOnly
    );
}

#[test]
fn three_way_no_conflict_when_equal() {
    assert_eq!(
        three_way_resolve("h1", "h2", "h2"),
        ThreeWayResult::NoConflict
    );
}

#[test]
fn three_way_both_changed() {
    assert_eq!(
        three_way_resolve("h1", "h2", "h3"),
        ThreeWayResult::BothChanged
    );
}

// ── 纯 LWW 决策测试（#644 评论 5474166587 问题3） ──

fn lww_rec(hash: &str, time: i64, device: &str, op: &str) -> LwwRecord {
    LwwRecord {
        content_hash: hash.to_string(),
        updated_at_ms: time,
        deleted_at_ms: None,
        device_id: device.to_string(),
        op: op.to_string(),
    }
}

#[test]
fn lww_remote_newer_wins() {
    let local = lww_rec("h1", 1000, "dev1", "upsert");
    let remote = lww_rec("h2", 2000, "dev2", "upsert");
    assert_eq!(resolve_lww(&local, &remote), LwwWinner::Remote);
}

#[test]
fn lww_local_newer_wins() {
    let local = lww_rec("h1", 2000, "dev1", "upsert");
    let remote = lww_rec("h2", 1000, "dev2", "upsert");
    assert_eq!(resolve_lww(&local, &remote), LwwWinner::Local);
}

#[test]
fn lww_tie_same_content_and_op() {
    let local = lww_rec("h1", 1000, "dev1", "upsert");
    let remote = lww_rec("h1", 1000, "dev2", "upsert");
    assert_eq!(resolve_lww(&local, &remote), LwwWinner::Tie);
}

#[test]
fn lww_tie_breaker_device_id_wins() {
    // 时间戳相同、内容不同 → device_id 字典序较大者获胜
    let local = lww_rec("h1", 1000, "dev1", "upsert");
    let remote = lww_rec("h2", 1000, "dev2", "upsert");
    assert_eq!(resolve_lww(&local, &remote), LwwWinner::Remote);
}

#[test]
fn lww_tie_breaker_local_device_id_wins() {
    let local = lww_rec("h1", 1000, "dev9", "upsert");
    let remote = lww_rec("h2", 1000, "dev1", "upsert");
    assert_eq!(resolve_lww(&local, &remote), LwwWinner::Local);
}

#[test]
fn lww_record_time_delete_prefers_deleted_at() {
    let rec = LwwRecord {
        content_hash: "h".to_string(),
        updated_at_ms: 1000,
        deleted_at_ms: Some(2000),
        device_id: "d".to_string(),
        op: "delete".to_string(),
    };
    assert_eq!(lww_record_time(&rec), 2000);
}
