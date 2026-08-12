//! 写作统计聚合模块。
//!
//! 将原始输入事件聚合为每日统计数据，支持实时和批量两种模式。
//!
//! 核心不变量：
//! - 单进程写入假设：aggregate_single_event 的 read-modify-write 非原子，
//!   并发写入可能丢失数据。多进程场景需外部串行化。
//! - 速度曲线分桶为半开区间 `[bucket_start, bucket_end)`。
//! - 事件按时间戳确定所属日期，按来源（人工/粘贴/删除/AI）分别累加字符数。

use crate::error::Result;
use crate::writing_stats::store::{DailyStats, SpeedBucket, StatsStore};
use crate::writing_stats::WritingInputEvent;
use std::path::Path;

pub struct StatsAggregator {
    store: StatsStore,
}

impl StatsAggregator {
    pub fn new(app_data_root: &Path) -> Self {
        Self {
            store: StatsStore::new(app_data_root),
        }
    }

    pub fn store(&self) -> &StatsStore {
        &self.store
    }

    pub fn aggregate_and_save(&self, start_date: &str, end_date: &str) -> Result<Vec<DailyStats>> {
        let events = self.store.load_events_range(start_date, end_date)?;
        let daily_stats = self.store.aggregate_events(&events)?;

        for stats in &daily_stats {
            self.store.save_or_merge_daily_stats(stats)?;
        }

        Ok(daily_stats)
    }

    /// 实时聚合单个事件并保存。
    ///
    /// 读取当日统计 → 合并事件 → 写回。此 read-modify-write 非原子：
    /// 并发写入可能导致数据丢失。当前设计假设单进程写入，多进程场景需外部串行化。
    pub fn aggregate_single_event(&self, event: &WritingInputEvent) -> Result<()> {
        let date = self.store.timestamp_to_date(event.timestamp_ms)?;

        let mut stats = DailyStats {
            date: date.clone(),
            device_id: event.device_id.clone(),
            platform: event.platform.to_string(),
            ..Default::default()
        };

        stats.apply_event(event);

        self.store.save_or_merge_daily_stats(&stats)?;
        Ok(())
    }

    /// 计算写作速度曲线。
    ///
    /// 将时间范围按 `bucket_minutes` 分桶，统计每个桶内的输入字符数和字符/分钟。
    /// 桶区间为半开区间 `[bucket_start, bucket_end)`。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn get_speed_curve(
        &self,
        start_date: &str,
        end_date: &str,
        bucket_minutes: u32,
    ) -> Result<Vec<SpeedBucket>> {
        let events = self.store.load_events_range(start_date, end_date)?;
        if events.is_empty() {
            return Ok(Vec::new());
        }

        let mut sorted = events;
        sorted.sort_by_key(|e| e.timestamp_ms);

        let bucket_ms = i64::from(bucket_minutes) * 60 * 1000;
        let Some(first) = sorted.first() else {
            return Ok(Vec::new());
        };
        let Some(last) = sorted.last() else {
            return Ok(Vec::new());
        };
        let first_ms = first.timestamp_ms;
        let last_ms = last.timestamp_ms;

        let mut buckets = Vec::new();
        let mut bucket_start = first_ms;

        while bucket_start <= last_ms {
            let bucket_end = bucket_start + bucket_ms;
            let mut chars_in_bucket: u32 = 0;

            for event in &sorted {
                if event.timestamp_ms >= bucket_start && event.timestamp_ms < bucket_end {
                    chars_in_bucket += event.inserted_chars;
                }
            }

            let minutes = bucket_ms as f64 / 60_000.0;
            let chars_per_minute = if minutes > 0.0 {
                f64::from(chars_in_bucket) / minutes
            } else {
                0.0
            };

            buckets.push(SpeedBucket {
                start_ms: bucket_start,
                end_ms: bucket_end,
                chars_typed: chars_in_bucket,
                chars_per_minute,
            });

            bucket_start = bucket_end;
        }

        Ok(buckets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writing_stats::{EventSource, Platform, WritingInputEvent};
    use tempfile::tempdir;

    #[test]
    fn test_aggregate_single_human_typed() {
        let temp_dir = tempdir().unwrap();
        let agg = StatsAggregator::new(temp_dir.path());

        let event = WritingInputEvent::new(
            "device-1",
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            0,
            "session-1",
        );

        agg.aggregate_single_event(&event).unwrap();

        let date = agg.store().timestamp_to_date(event.timestamp_ms).unwrap();
        let stats = agg.store().load_all_daily_stats_for_date(&date).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_human_typed_chars, 10);
        assert_eq!(stats[0].total_pasted_chars, 0);
        assert_eq!(stats[0].total_deleted_chars, 0);
        assert_eq!(stats[0].total_ai_inserted_chars, 0);
    }

    #[test]
    fn test_aggregate_does_not_count_paste_as_human_typed() {
        let temp_dir = tempdir().unwrap();
        let agg = StatsAggregator::new(temp_dir.path());

        let event = WritingInputEvent::new(
            "device-1",
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            EventSource::Pasted,
            0,
            0,
            20,
            0,
            0,
            "session-1",
        );

        agg.aggregate_single_event(&event).unwrap();

        let date = agg.store().timestamp_to_date(event.timestamp_ms).unwrap();
        let stats = agg.store().load_all_daily_stats_for_date(&date).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_human_typed_chars, 0);
        assert_eq!(stats[0].total_pasted_chars, 20);
    }

    #[test]
    fn test_aggregate_delete_does_not_cancel_human_typed() {
        let temp_dir = tempdir().unwrap();
        let agg = StatsAggregator::new(temp_dir.path());

        let event1 = WritingInputEvent::new(
            "device-1",
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            0,
            "session-1",
        );
        agg.aggregate_single_event(&event1).unwrap();

        let event2 = WritingInputEvent::new(
            "device-1",
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            EventSource::Deleted,
            0,
            3,
            0,
            0,
            0,
            "session-1",
        );
        agg.aggregate_single_event(&event2).unwrap();

        let date = agg.store().timestamp_to_date(event1.timestamp_ms).unwrap();
        let stats = agg.store().load_all_daily_stats_for_date(&date).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_human_typed_chars, 10);
        assert_eq!(stats[0].total_deleted_chars, 3);
        assert_eq!(stats[0].total_net_delta_chars, 7);
    }

    #[test]
    fn test_aggregate_ai_insert_not_counted_as_human() {
        let temp_dir = tempdir().unwrap();
        let agg = StatsAggregator::new(temp_dir.path());

        let event = WritingInputEvent::new(
            "device-1",
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            EventSource::AiInserted,
            0,
            0,
            0,
            50,
            0,
            "session-1",
        );

        agg.aggregate_single_event(&event).unwrap();

        let date = agg.store().timestamp_to_date(event.timestamp_ms).unwrap();
        let stats = agg.store().load_all_daily_stats_for_date(&date).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_human_typed_chars, 0);
        assert_eq!(stats[0].total_ai_inserted_chars, 50);
    }

    #[test]
    fn test_per_project_tracking() {
        let temp_dir = tempdir().unwrap();
        let agg = StatsAggregator::new(temp_dir.path());

        let event = WritingInputEvent::new(
            "device-1",
            Platform::Desktop,
            "desktop",
            "proj-abc",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            15,
            0,
            0,
            0,
            0,
            "session-1",
        );

        agg.aggregate_single_event(&event).unwrap();

        let date = agg.store().timestamp_to_date(event.timestamp_ms).unwrap();
        let stats = agg.store().load_all_daily_stats_for_date(&date).unwrap();
        assert_eq!(stats.len(), 1);
        let proj = stats[0].per_project.get("proj-abc").unwrap();
        assert_eq!(proj.human_typed_chars, 15);
    }

    #[test]
    fn test_multiple_devices_separate() {
        let temp_dir = tempdir().unwrap();
        let agg = StatsAggregator::new(temp_dir.path());

        let event1 = WritingInputEvent::new(
            "device-linux",
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            0,
            "session-1",
        );
        agg.aggregate_single_event(&event1).unwrap();

        let event2 = WritingInputEvent::new(
            "device-android",
            Platform::Android,
            "phone",
            "proj1",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            20,
            0,
            0,
            0,
            0,
            "session-2",
        );
        agg.aggregate_single_event(&event2).unwrap();

        let date = agg.store().timestamp_to_date(event1.timestamp_ms).unwrap();
        let all_stats = agg.store().load_all_daily_stats_for_date(&date).unwrap();
        assert_eq!(all_stats.len(), 2);

        let stats_linux = all_stats
            .iter()
            .find(|s| s.device_id == "device-linux")
            .unwrap();
        assert_eq!(stats_linux.total_human_typed_chars, 10);

        let stats_android = all_stats
            .iter()
            .find(|s| s.device_id == "device-android")
            .unwrap();
        assert_eq!(stats_android.total_human_typed_chars, 20);
    }

    #[test]
    fn test_speed_bucket_generation() {
        let temp_dir = tempdir().unwrap();
        let agg = StatsAggregator::new(temp_dir.path());

        let now_ms = chrono::Utc::now().timestamp_millis();

        let event = WritingInputEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            timestamp_ms: now_ms,
            device_id: "device-1".to_string(),
            platform: Platform::Desktop,
            device_class: "desktop".to_string(),
            project_id: "proj1".to_string(),
            volume_id: "vol1".to_string(),
            chapter_id: "chap1".to_string(),
            source: EventSource::HumanTyped,
            inserted_chars: 30,
            deleted_chars: 0,
            pasted_chars: 0,
            ai_inserted_chars: 0,
            net_delta_chars: 30,
            duration_seconds: 0,
            session_id: "session-1".to_string(),
        };

        agg.store().record_event(event).unwrap();
        agg.store().flush_events().unwrap();

        let date = agg.store().timestamp_to_date(now_ms).unwrap();
        let buckets = agg.get_speed_curve(&date, &date, 1).unwrap();
        assert!(!buckets.is_empty());
        assert!(buckets.iter().any(|b| b.chars_typed > 0));
    }
}
