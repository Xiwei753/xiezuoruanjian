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

    // 空安装判断 =「daily 没有统计文件」**且**「raw events 也为空」。
    //
    // 只看 daily 会漏一种真实情况：raw events 已存在但 daily 恰好缺失
    // （例如上次迁移失败、或用户手工删过 daily 目录）。此时若直接写 marker，
    // daily 永远不会被重建，统计就此一直缺失。
    if !store.daily_dir_has_stats()? && store.list_event_file_dates()?.is_empty() {
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
///
/// **必须**用 `load_events_for_date_strict`：宽松 loader 会静默跳过解析不了的
/// 行，回写时那些原始数据就被永久删掉了。任一行坏掉就带文件名+行号返回 Err，
/// 此时一个字节都没改写，旧 `daily/` 也保留（步骤 2 还没跑）。
fn backfill_event_local_dates(store: &StatsStore) -> Result<()> {
    for date in store.list_event_file_dates()? {
        let events = store.load_events_for_date_strict(&date)?;
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
        store.flush_events().unwrap();
    }

    /// 汇总某天所有设备的纯输入字数。
    fn total_chars_on(dir: &Path, date: &str) -> u64 {
        StatsStore::new(dir)
            .load_all_daily_stats_for_date(date)
            .unwrap()
            .iter()
            .map(|s| s.total_human_typed_chars)
            .sum()
    }

    // `aggregate_events` 按 (business_date, device_id) 分组，同一天返回多条。
    // 重建时若自己拼 `DailyStatsFile { devices: vec![stats] }` 覆盖写，
    // 后一个设备会把前一个设备的数据整条顶掉——同一天多设备只剩最后一个。
    #[test]
    fn test_migration_keeps_all_devices_of_same_day() {
        let dir = tempdir().unwrap();
        let ts = 1_791_217_800_000; // UTC+8 本地 2026-10-06 00:30
        legacy_event(dir.path(), "dev-A", 10, ts);
        legacy_event(dir.path(), "dev-B", 20, ts);
        let local_day = crate::writing_stats::calendar::local_date_at(ts).unwrap();

        // 老口径写一个只含单设备的 daily，触发重建路径。
        write_daily(dir.path(), "2026-10-05", 30);

        migrate_stats_to_local_calendar(dir.path()).unwrap();

        let devices = StatsStore::new(dir.path())
            .load_all_daily_stats_for_date(&local_day)
            .unwrap();
        assert_eq!(devices.len(), 2, "同一天的两个设备都必须在重建结果里");
        assert_eq!(
            total_chars_on(dir.path(), &local_day),
            30,
            "汇总必须是 10 + 20，而不是其中之一"
        );
    }

    // 迁移回写 raw events 时若用容错 loader，解析不了的原始行会被永久删除。
    // 严格 loader 必须在坏行上报错并中止，让旧 daily 保留。
    #[test]
    fn test_migration_refuses_to_delete_corrupt_raw_event() {
        let dir = tempdir().unwrap();
        let ts = chrono::Utc::now().timestamp_millis();
        legacy_event(dir.path(), "dev-1", 30, ts);
        write_daily(dir.path(), "2026-10-05", 30);

        // 往 raw 文件追加一行无法反序列化的旧 schema 数据。
        let store = StatsStore::new(dir.path());
        let utc_date = store.timestamp_to_date(ts).unwrap();
        let raw_path = dir.path().join("app-meta/stats/events.local");
        let raw =
            std::fs::read_to_string(&raw_path.join(format!("{}.events.jsonl", utc_date))).unwrap();
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
            std::fs::read_to_string(&raw_path.join(format!("{}.events.jsonl", utc_date))).unwrap();
        assert!(raw_after.contains("totally"), "损坏的原始行不能被静默删除");
        // 旧 daily 保留：步骤 2（删 daily）根本没跑到。
        assert_eq!(total_chars_on(dir.path(), "2026-10-05"), 30);
        assert!(!dir.path().join("app-meta/stats").join(MARKER_NAME).exists());
    }

    // raw events 存在但 daily 恰好缺失时，不能直接写 marker 跳过重建，
    // 否则 daily 永远缺失、统计一直空白。
    #[test]
    fn test_migration_rebuilds_when_daily_missing_but_events_present() {
        let dir = tempdir().unwrap();
        let ts = chrono::Utc::now().timestamp_millis();
        legacy_event(dir.path(), "dev-1", 25, ts);
        // 故意不写 daily：模拟「有 raw、daily 缺失」。
        assert!(!StatsStore::new(dir.path()).daily_dir_has_stats().unwrap());

        migrate_stats_to_local_calendar(dir.path()).unwrap();

        let today = crate::writing_stats::calendar::local_today_date();
        assert_eq!(
            total_chars_on(dir.path(), &today),
            25,
            "有 raw events 却缺 daily 时必须重建"
        );
    }
}
