//! 写作统计的一次性日历口径重建。
//!
//! ## 为什么需要
//!
//! 每日统计（`app-meta/stats/daily/*.stats.json`）此前按 **UTC 日期**分桶：
//! `aggregate_single_event()` 拿 `timestamp_to_date()`（UTC）当「哪一天」。
//! 而「今日纯输入」这类产品语义要求按用户当地午夜切日。东八区本地
//! 10 月 6 日 00:30 输入的字被聚合进 UTC 的 10 月 5 日，于是状态栏
//! 「今日纯输入」在本地凌晨 0 点提前清零，直到早上 8 点之后才开始正常累计。
//!
//! 现在 [`WritingInputEvent`] 持久化 `local_date`，
//! [`StatsStore::aggregate_events`] 也按它分桶（见
//! [`crate::writing_stats::calendar`]）。**旧 daily 文件是按老口径生成的**，
//! 直接读会一直错下去。
//!
//! ## 做法
//!
//! daily stats 本来就是**可从 raw events 重建的派生数据**，所以这里
//! 不做「新旧双读」——旧 UTC daily 文件直接删掉，按新日历口径重算：
//!
//! 1. 扫 `events.local/*.events.jsonl`，给缺 `local_date` 的老事件补上
//!    （用 Core 自己的本地时区换算，不让平台端参与），并**回写** raw 文件。
//! 2. 删掉整个 `daily/` 目录。
//! 3. 从全部 raw events 重新聚合，逐日写回新的 daily 文件。
//!
//! 幂等：写一个 `calendar.local.v1` marker 文件在 `app-meta/stats/` 下，
//! 已有 marker 直接返回，重复调用不会二次重建。
//!
//! ## 失败处理
//!
//! 步骤 1 失败（raw 文件损坏/不可写）直接返回 `Err`，**不删 daily 目录**——
//! 此时旧的 daily 数据还是唯一可用的一份，不能先砸掉。
//!
//! ## 调用位置
//!
//! [`crate::writing_stats::api::StatsApi::new`]——每日统计第一次被使用时。
//! 那时 `app_data_root` 已确定，且比 UI 早。

use std::path::Path;

use crate::error::Result;
use crate::writing_stats::store::StatsStore;

/// 本次重建的 marker 内容。改动重建逻辑时把版本号往上抬。
const MARKER_NAME: &str = "calendar.local.v1";
const MARKER_CONTENT: &str = "daily stats 已按本地日历日重建\n";

/// 重建入口。幂等：已重建过就直接返回。
pub fn migrate_stats_to_local_calendar(app_data_root: &Path) -> Result<()> {
    let stats_dir = app_data_root.join("app-meta/stats");
    let marker_path = stats_dir.join(MARKER_NAME);
    if marker_path.exists() {
        return Ok(());
    }

    let store = StatsStore::new(app_data_root);

    // 没有任何 daily 文件时说明这个安装还没写过统计，重建是空操作。
    // 但仍要写 marker，否则每次启动都要重扫一遍 events 目录。
    if !store.daily_dir_has_stats()? {
        write_marker(&stats_dir)?;
        return Ok(());
    }

    // 步骤 1：补 local_date 并回写 raw events。失败就不往下走，保留旧 daily。
    backfill_event_local_dates(&store)?;

    // 步骤 2 + 3：删旧 daily，按新口径重算。
    store.rebuild_daily_stats_from_events()?;

    write_marker(&stats_dir)?;
    Ok(())
}

fn write_marker(stats_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(stats_dir)?;
    std::fs::write(stats_dir.join(MARKER_NAME), MARKER_CONTENT)?;
    Ok(())
}

/// 给缺 `local_date` 的老事件补本地日历日并回写 raw 文件。
fn backfill_event_local_dates(store: &StatsStore) -> Result<()> {
    for date in store.list_event_file_dates()? {
        let events = store.load_events_for_date(&date)?;
        if events.iter().all(|e| !e.local_date.is_empty()) {
            continue;
        }

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
    use crate::writing_stats::store::{DailyStats, DailyStatsFile};
    use crate::writing_stats::{EventSource, Platform, WritingInputEvent};
    use tempfile::tempdir;

    fn write_daily(dir: &Path, date: &str, chars: u64) {
        let stats = DailyStats {
            date: date.to_string(),
            device_id: "dev-1".to_string(),
            platform: "desktop".to_string(),
            total_human_typed_chars: chars,
            ..Default::default()
        };
        let file = DailyStatsFile {
            date: date.to_string(),
            devices: vec![stats],
        };
        let store = StatsStore::new(dir);
        store.save_daily_stats_file(&file).unwrap();
    }

    #[test]
    fn test_migration_rebuilds_daily_with_local_calendar() {
        let dir = tempdir().unwrap();
        // 一条 UTC 10/5 16:30（= UTC+8 本地 10/6 00:30）的老事件，没有 local_date。
        let mut event = WritingInputEvent::new(
            "dev-1",
            Platform::Desktop,
            "desktop",
            "p1",
            "v1",
            "c1",
            EventSource::HumanTyped,
            30,
            0,
            0,
            0,
            0,
            "s1",
        );
        event.timestamp_ms = 1_791_217_800_000;
        event.local_date = String::new();
        let store = StatsStore::new(dir.path());
        store.record_event(event).unwrap();
        store.flush_events().unwrap();

        // 老口径：按 UTC 日期写成 10/5。
        write_daily(dir.path(), "2026-10-05", 30);

        migrate_stats_to_local_calendar(dir.path()).unwrap();

        // 重建后应落在本地日（本机时区决定具体哪天），且总字数不变。
        let after = StatsStore::new(dir.path());
        let daily_dates = after.list_daily_file_dates().unwrap();
        let mut total = 0;
        for date in &daily_dates {
            for stats in after.load_all_daily_stats_for_date(date).unwrap() {
                total += stats.total_human_typed_chars;
            }
        }
        assert_eq!(total, 30, "重建后总字数不应丢失或重复");
        assert!(!daily_dates.is_empty(), "重建后必须留下 daily 文件");

        // 老 UTC 口径写的 2026-10-05 不该还留在 daily 目录里当第二份。
        let stale = after.load_all_daily_stats_for_date("2026-10-05").unwrap();
        let local_today = crate::writing_stats::calendar::local_today_date();
        if local_today != "2026-10-05" {
            assert!(stale.is_empty(), "旧 UTC daily 文件应被删除，不做新旧双读");
        }
    }

    #[test]
    fn test_migration_is_idempotent() {
        let dir = tempdir().unwrap();
        let mut event = WritingInputEvent::new(
            "dev-1",
            Platform::Desktop,
            "desktop",
            "p1",
            "v1",
            "c1",
            EventSource::HumanTyped,
            12,
            0,
            0,
            0,
            0,
            "s1",
        );
        event.timestamp_ms = chrono::Utc::now().timestamp_millis();
        event.local_date = String::new();
        let store = StatsStore::new(dir.path());
        store.record_event(event).unwrap();
        store.flush_events().unwrap();
        write_daily(dir.path(), "2026-10-05", 12);

        migrate_stats_to_local_calendar(dir.path()).unwrap();
        migrate_stats_to_local_calendar(dir.path()).unwrap();

        let after = StatsStore::new(dir.path());
        let mut total = 0;
        for date in after.list_daily_file_dates().unwrap() {
            for stats in after.load_all_daily_stats_for_date(&date).unwrap() {
                total += stats.total_human_typed_chars;
            }
        }
        assert_eq!(total, 12, "重复执行不应重复计数");
    }

    #[test]
    fn test_migration_on_empty_install_writes_marker() {
        let dir = tempdir().unwrap();
        migrate_stats_to_local_calendar(dir.path()).unwrap();
        assert!(dir.path().join("app-meta/stats").join(MARKER_NAME).exists());
    }
}
