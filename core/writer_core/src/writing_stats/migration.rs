//! # 写作统计事件存储迁移
//!
//! 把旧版单机事件库 `app-meta/stats/events.local/YYYY-MM-DD.events.jsonl`
//! 一次性迁入新的按来源设备分区的事件库 `stats/events/<origin_device_id>/`。
//!
//! 旧目录只作为迁移输入：迁移完成后不再写入、不再作为另一份活跃统计真相源，
//! 但文件保留在原地，供诊断与回退查看。迁移按 `event_id` 幂等写入目标分区，
//! 失败时不会写入完成标记，下次启动可以安全重试。

use crate::error::Result;
use crate::writing_stats::store::StatsStore;
use crate::writing_stats::WritingInputEvent;
use chrono::NaiveDate;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// 新版事件分区迁移完成标记。
pub const MARKER_NAME: &str = "events.device_partitions.v1";
pub const MARKER_CONTENT: &str = "raw events migrated into per-device partitions\n";

/// 旧版本地事件目录（相对 app_data_root）。
const LEGACY_EVENTS_DIR: &str = "app-meta/stats/events.local";

/// 把旧版本地事件库迁移到按来源设备分区的新事件库。
///
/// - 已完成迁移（标记存在）时直接返回；
/// - 旧库为空（全新安装）时只写完成标记；
/// - 否则逐文件严格读取旧事件、补齐缺失的业务日 `local_date`，按 `event_id`
///   幂等写入本机来源分区，全部成功后再写完成标记。
pub fn migrate_stats_event_store(app_data_root: &Path) -> Result<()> {
    let stats_dir = app_data_root.join("app-meta/stats");
    if stats_dir.join(MARKER_NAME).exists() {
        return Ok(());
    }

    let legacy_dir = app_data_root.join(LEGACY_EVENTS_DIR);
    let legacy_files = list_legacy_event_files(&legacy_dir)?;
    if legacy_files.is_empty() {
        // 全新安装（或旧库为空）：不生成任何事件，只记录迁移完成。
        return write_marker(&stats_dir);
    }

    let store = StatsStore::new(app_data_root);
    // 先解析本机来源分区；失败会在写标记前返回，下次启动可以重试。
    store.resolve_origin_device_id()?;

    let mut migrated = 0usize;
    for (date, path) in &legacy_files {
        let mut events = load_legacy_events(date, path)?;
        for event in &mut events {
            if event.local_date.is_empty() {
                event.local_date = event.business_date();
            }
        }
        migrated += store.record_events(events)?;
    }
    log::debug!("[writing_stats] migrated {migrated} legacy stats events");
    write_marker(&stats_dir)
}

/// 旧库是否还存在待迁移的合法事件文件（仅枚举，不触发迁移、不写盘）。
///
/// 供同步组发现使用：升级后若自动同步先于统计页发生，本机统计设备组
/// 必须仍能出现在计划里，旧历史才有机会被上传。
pub fn legacy_events_present(app_data_root: &Path) -> bool {
    list_legacy_event_files(&app_data_root.join(LEGACY_EVENTS_DIR))
        .map(|files| !files.is_empty())
        .unwrap_or(false)
}

fn write_marker(stats_dir: &Path) -> Result<()> {
    fs::create_dir_all(stats_dir)?;
    fs::write(stats_dir.join(MARKER_NAME), MARKER_CONTENT)?;
    Ok(())
}

/// 列出旧事件库中合法的日期文件，按日期升序。
fn list_legacy_event_files(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(date) = name.strip_suffix(".events.jsonl") else {
            continue;
        };
        if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
            continue;
        }
        files.push((date.to_string(), entry.path()));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

/// 严格读取一个旧事件文件（任何一行解析失败都报错）。
fn load_legacy_events(date: &str, path: &Path) -> Result<Vec<WritingInputEvent>> {
    let handle = File::open(path)?;
    let reader = BufReader::new(handle);
    let mut events = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event: WritingInputEvent = serde_json::from_str(trimmed).map_err(|e| {
            crate::Error::Other(format!(
                "Corrupt legacy event at {LEGACY_EVENTS_DIR}/{date}.events.jsonl:{}: {e}",
                idx + 1
            ))
        })?;
        events.push(event);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
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
        assert!(dir
            .join("stats/events/device_1")
            .join(format!("{date}.events.jsonl"))
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
}
