//! 写作统计的一次性日历口径迁移。
//!
//! ## 为什么需要
//!
//! 每日统计（`app-meta/stats/daily/*.stats.json`）此前按 **UTC 日期**分桶。
//! 现在 [`WritingInputEvent`] 持久化 `local_date`，新架构不再使用 daily 文件，
//! 所有统计由 [`crate::writing_stats::projection`] 从 raw events 实时计算。
//!
//! ## 做法
//!
//! 迁移只需要补 `local_date`，不需要重建 daily：
//!
//! 1. 扫 `events.local/*.events.jsonl`，给缺 `local_date` 的老事件补上
//!    （用 Core 自己的本地时区换算，不让平台端参与），并**回写** raw 文件。
//! 2. 写 marker 文件标记迁移完成。
//!
//! 旧 `app-meta/stats/daily` 不再作为运行时输入，已有 raw events 继续能被
//! 新 projection 读取。
//!
//! 幂等：写一个 `calendar.local.v2` marker 文件在 `app-meta/stats/` 下，
//! 已有 marker 直接返回，重复调用不会二次迁移。

use std::path::Path;

use crate::error::Result;
use crate::writing_stats::store::StatsStore;

/// 本次迁移的 marker 内容。改动迁移逻辑时把版本号往上抬。
const MARKER_NAME: &str = "calendar.local.v2";
const MARKER_CONTENT: &str = "raw events local_date 已补齐\n";

/// 迁移入口。幂等：已迁移过就直接返回。
pub fn migrate_stats_to_local_calendar(app_data_root: &Path) -> Result<()> {
    let stats_dir = app_data_root.join("app-meta/stats");
    let marker_path = stats_dir.join(MARKER_NAME);
    if marker_path.exists() {
        return Ok(());
    }

    let store = StatsStore::new(app_data_root);

    // 空安装判断：raw events 为空时直接写 marker。
    if store.list_event_file_dates()?.is_empty() {
        write_marker(&stats_dir)?;
        return Ok(());
    }

    // 步骤 1：补 local_date 并回写 raw events。失败就不写 marker。
    backfill_event_local_dates(&store)?;

    write_marker(&stats_dir)?;
    Ok(())
}

fn write_marker(stats_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(stats_dir)?;
    std::fs::write(stats_dir.join(MARKER_NAME), MARKER_CONTENT)?;
    Ok(())
}

/// 给缺 `local_date` 的老事件补本地日历日并回写 raw 文件。
///
/// `load_events_for_date` 是严格读取：任一行解析失败就带文件名+行号返回 Err，
/// 此时一个字节都没改写。宽松跳过坏行会让回写时永久删掉解析不了的原始数据。
fn backfill_event_local_dates(store: &StatsStore) -> Result<()> {
    for date in store.list_event_file_dates()? {
        let events = store.load_events_for_date(&date)?;
        if events.iter().all(|e| !e.local_date.is_empty()) {
            continue;
        }

        // 先在内存里拼好完整内容，全部成功才落盘，避免「写了一半失败」留下截断文件。
        let mut lines = String::new();
        for mut event in events {
            if event.local_date.is_empty() {
                event.local_date = event.business_date();
            }
            lines.push_str(&serde_json::to_string(&event)?);
            lines.push('\n');
        }
        store.rewrite_events_for_date(&date, &lines)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writing_stats::{EventSource, Platform, WritingInputEvent};
    use tempfile::tempdir;

    /// 造一条带指定 device_id / 字数 / 时间戳的老事件（无 local_date）。
    fn legacy_event(dir: &Path, device_id: &str, chars: u32, timestamp_ms: i64) {
        let mut event = WritingInputEvent::new(
            device_id,
            Platform::Desktop,
            "desktop",
            "p1",
            "v1",
            "c1",
            EventSource::HumanTyped,
            chars,
            0,
            0,
            0,
            0,
            "s1",
        );
        event.timestamp_ms = timestamp_ms;
        event.local_date = String::new();
        let store = StatsStore::new(dir);
        store.record_event(event).unwrap();
    }

    #[test]
    fn test_migration_backfills_local_date() {
        let dir = tempdir().unwrap();
        let ts = 1_791_217_800_000; // UTC+8 本地 2026-10-06 00:30
        legacy_event(dir.path(), "dev-1", 30, ts);

        migrate_stats_to_local_calendar(dir.path()).unwrap();

        // 验证 raw events 的 local_date 已补齐
        let store = StatsStore::new(dir.path());
        let utc_date = store.timestamp_to_date(ts).unwrap();
        let events = store.load_events_for_date(&utc_date).unwrap();
        assert_eq!(events.len(), 1);
        assert!(
            !events[0].local_date.is_empty(),
            "local_date should be backfilled"
        );
    }

    #[test]
    fn test_migration_is_idempotent() {
        let dir = tempdir().unwrap();
        let ts = chrono::Utc::now().timestamp_millis();
        legacy_event(dir.path(), "dev-1", 12, ts);

        migrate_stats_to_local_calendar(dir.path()).unwrap();
        migrate_stats_to_local_calendar(dir.path()).unwrap();

        // marker 文件存在
        assert!(dir.path().join("app-meta/stats").join(MARKER_NAME).exists());
    }

    #[test]
    fn test_migration_on_empty_install_writes_marker() {
        let dir = tempdir().unwrap();
        migrate_stats_to_local_calendar(dir.path()).unwrap();
        assert!(dir.path().join("app-meta/stats").join(MARKER_NAME).exists());
    }

    #[test]
    fn test_migration_refuses_to_delete_corrupt_raw_event() {
        let dir = tempdir().unwrap();
        let ts = chrono::Utc::now().timestamp_millis();
        legacy_event(dir.path(), "dev-1", 30, ts);

        // 往 raw 文件追加一行无法反序列化的旧 schema 数据。
        let store = StatsStore::new(dir.path());
        let utc_date = store.timestamp_to_date(ts).unwrap();
        let raw_path = dir.path().join("app-meta/stats/events.local");
        let raw =
            std::fs::read_to_string(raw_path.join(format!("{}.events.jsonl", utc_date))).unwrap();
        std::fs::write(
            raw_path.join(format!("{}.events.jsonl", utc_date)),
            format!("{}{}", raw, "{\"totally\":\"unknown-schema\"}\n"),
        )
        .unwrap();

        let err = migrate_stats_to_local_calendar(dir.path()).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains(&utc_date) && msg.contains(":2"),
            "错误应带文件名和行号，实际：{}",
            msg
        );

        // 坏行必须还在 raw 文件里（一个字节都没丢）。
        let raw_after =
            std::fs::read_to_string(raw_path.join(format!("{}.events.jsonl", utc_date))).unwrap();
        assert!(raw_after.contains("totally"), "损坏的原始行不能被静默删除");
        // marker 不应被写入
        assert!(!dir.path().join("app-meta/stats").join(MARKER_NAME).exists());
    }
}
