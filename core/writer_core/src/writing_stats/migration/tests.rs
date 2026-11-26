//! migration.rs 的单元测试（从生产文件拆出，保持原有断言）。

use super::*;
use crate::writing_stats::calendar;
use crate::writing_stats::{EventSource, Platform};
use tempfile::TempDir;

fn legacy_event(device_id: &str, chars: u32, timestamp_ms: i64) -> WritingInputEvent {
    let mut event = WritingInputEvent::new(
        device_id,
        Platform::Desktop,
        "desktop",
        "proj_1",
        "vol_1",
        "ch_1",
        EventSource::HumanTyped,
        chars,
        0,
        0,
        0,
        0,
        "session_1",
    );
    event.timestamp_ms = timestamp_ms;
    event.local_date = String::new();
    event
}

fn write_legacy_file(dir: &Path, event: &WritingInputEvent) -> String {
    let date = calendar::utc_date_at(event.timestamp_ms).unwrap();
    let legacy_dir = dir.join(LEGACY_EVENTS_DIR);
    fs::create_dir_all(&legacy_dir).unwrap();
    let path = legacy_dir.join(format!("{date}.events.jsonl"));
    fs::write(
        &path,
        format!("{}\n", serde_json::to_string(event).unwrap()),
    )
    .unwrap();
    date
}

fn prepare(dir: &Path) {
    crate::settings::ensure_device_info(dir, "", "", Some("device_1")).unwrap();
}

#[test]
fn test_migrates_legacy_events_into_device_partition() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    prepare(dir);
    let ts = 1_791_217_800_000;
    let date = write_legacy_file(dir, &legacy_event("device_1", 42, ts));

    migrate_stats_event_store(dir).unwrap();

    let store = StatsStore::new(dir);
    let events = store.load_events_for_date(&date).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].inserted_chars, 42);
    assert!(!events[0].local_date.is_empty());
    // 新契约：迁入的历史业务日立即封口为 zstd 归档段
    // (`proof/archive/<date>.events.jsonl.zst`)，不再保留明文 events.jsonl。
    assert!(dir
        .join(crate::writing_stats::checkpoint::archive_relative_path(
            "device_1", &date
        ))
        .exists());
    assert!(dir.join("app-meta/stats").join(MARKER_NAME).exists());
    // 旧数据保留真实记录，不被删除。
    assert!(dir
        .join(LEGACY_EVENTS_DIR)
        .join(format!("{date}.events.jsonl"))
        .exists());
}

#[test]
fn test_migration_retry_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    prepare(dir);
    let date = write_legacy_file(dir, &legacy_event("device_1", 42, 1_791_217_800_000));

    migrate_stats_event_store(dir).unwrap();
    // 模拟写标记前中断：删除标记后重跑，事件按 event_id 去重不重复。
    fs::remove_file(dir.join("app-meta/stats").join(MARKER_NAME)).unwrap();
    migrate_stats_event_store(dir).unwrap();

    let store = StatsStore::new(dir);
    assert_eq!(store.load_events_for_date(&date).unwrap().len(), 1);
}

#[test]
fn test_empty_install_writes_marker_without_events() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    migrate_stats_event_store(dir).unwrap();

    assert!(dir.join("app-meta/stats").join(MARKER_NAME).exists());
    assert!(!dir.join("stats/events").exists());
}

#[test]
fn test_corrupt_legacy_file_blocks_migration_and_preserves_content() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    prepare(dir);
    let date = write_legacy_file(dir, &legacy_event("device_1", 42, 1_791_217_800_000));
    let legacy_path = dir
        .join(LEGACY_EVENTS_DIR)
        .join(format!("{date}.events.jsonl"));
    let original = fs::read_to_string(&legacy_path).unwrap();
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&legacy_path)
        .unwrap();
    use std::io::Write;
    writeln!(file, "{{\"bad\":}}").unwrap();
    drop(file);

    let err = migrate_stats_event_store(dir).unwrap_err();
    let message = err.to_string();
    assert!(message.contains(&date));
    assert!(message.contains(":2"));
    assert!(!dir.join("app-meta/stats").join(MARKER_NAME).exists());
    // 损坏的旧文件保持原样，未被清空或重写。
    let after = fs::read_to_string(&legacy_path).unwrap();
    assert_eq!(after, format!("{original}{{\"bad\":}}\n"));
}

#[test]
fn test_migrated_legacy_files_are_not_read_again() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    prepare(dir);
    let date = write_legacy_file(dir, &legacy_event("device_1", 42, 1_791_217_800_000));
    migrate_stats_event_store(dir).unwrap();

    // 迁移完成后给旧文件追加假事件，不再影响聚合结果。
    let legacy_path = dir
        .join(LEGACY_EVENTS_DIR)
        .join(format!("{date}.events.jsonl"));
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&legacy_path)
        .unwrap();
    use std::io::Write;
    writeln!(
        file,
        "{}",
        serde_json::to_string(&legacy_event("device_1", 99, 1_791_217_800_000)).unwrap()
    )
    .unwrap();
    drop(file);

    let store = StatsStore::new(dir);
    let events = store.load_all_events().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].inserted_chars, 42);
}
