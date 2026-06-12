//! # 写作统计聚合模块
//!
//! 本模块负责将原始的写作输入事件聚合为有意义的统计数据，是统计系统的核心处理层。
//!
//! ## 主要功能
//!
//! - **事件聚合**: 将多个输入事件汇总为每日统计数据
//! - **实时聚合**: 支持单个事件的实时统计更新
//! - **批量聚合**: 支持指定时间范围内的批量数据聚合
//! - **速度曲线**: 计算指定时间粒度的写作速度变化曲线
//! - **多维度统计**: 按项目、卷、章节进行细分统计
//!
//! ## 核心结构
//!
//! - `StatsAggregator`: 统计聚合器，协调事件聚合和数据存储
//!
//! ## 聚合逻辑
//!
//! 1. 根据事件时间戳确定所属日期
//! 2. 按事件来源分别累加字符数（人工输入/粘贴/删除/AI插入）
//! 3. 更新项目、卷、章节的细分统计
//! 4. 将聚合结果保存或合并到每日统计文件
//!
//! ## 依赖关系
//!
//! - `crate::writing_stats::store`: 数据存储层
//! - `crate::writing_stats::WritingInputEvent`: 输入事件定义
//!
//! ## 使用场景
//!
//! - 事件记录后的实时统计更新
//! - 历史数据的重新聚合计算
//! - 生成写作速度分析报告
//! - 统计数据的批量处理和迁移

use crate::error::Result;
use crate::writing_stats::store::{DailyStats, SpeedBucket, StatsStore};
use crate::writing_stats::WritingInputEvent;
use std::path::Path;

pub struct StatsAggregator {
    store: StatsStore,
}

impl StatsAggregator {
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            store: StatsStore::new(workspace_path),
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

    pub fn aggregate_single_event(&self, event: &WritingInputEvent) -> Result<()> {
        let date = self.store.timestamp_to_date(event.timestamp_ms)?;

        let mut stats = DailyStats {
            date: date.clone(),
            device_id: event.device_id.clone(),
            platform: event.platform.to_string(),
            ..Default::default()
        };

        match event.source {
            crate::writing_stats::EventSource::HumanTyped => {
                stats.total_human_typed_chars += event.inserted_chars as u64;
            }
            crate::writing_stats::EventSource::Pasted => {
                stats.total_pasted_chars += event.pasted_chars as u64;
            }
            crate::writing_stats::EventSource::Deleted => {
                stats.total_deleted_chars += event.deleted_chars as u64;
            }
            crate::writing_stats::EventSource::AiInserted => {
                stats.total_ai_inserted_chars += event.ai_inserted_chars as u64;
            }
            _ => {}
        }
        stats.total_net_delta_chars += event.net_delta_chars as i64;

        let proj = stats
            .per_project
            .entry(event.project_id.clone())
            .or_default();
        match event.source {
            crate::writing_stats::EventSource::HumanTyped => {
                proj.human_typed_chars += event.inserted_chars as u64
            }
            crate::writing_stats::EventSource::Pasted => {
                proj.pasted_chars += event.pasted_chars as u64
            }
            crate::writing_stats::EventSource::Deleted => {
                proj.deleted_chars += event.deleted_chars as u64
            }
            crate::writing_stats::EventSource::AiInserted => {
                proj.ai_inserted_chars += event.ai_inserted_chars as u64
            }
            _ => {}
        }
        proj.net_delta_chars += event.net_delta_chars as i64;

        let vol = stats.per_volume.entry(event.volume_id.clone()).or_default();
        match event.source {
            crate::writing_stats::EventSource::HumanTyped => {
                vol.human_typed_chars += event.inserted_chars as u64
            }
            crate::writing_stats::EventSource::Pasted => {
                vol.pasted_chars += event.pasted_chars as u64
            }
            crate::writing_stats::EventSource::Deleted => {
                vol.deleted_chars += event.deleted_chars as u64
            }
            crate::writing_stats::EventSource::AiInserted => {
                vol.ai_inserted_chars += event.ai_inserted_chars as u64
            }
            _ => {}
        }
        vol.net_delta_chars += event.net_delta_chars as i64;

        let chap = stats
            .per_chapter
            .entry(event.chapter_id.clone())
            .or_default();
        match event.source {
            crate::writing_stats::EventSource::HumanTyped => {
                chap.human_typed_chars += event.inserted_chars as u64
            }
            crate::writing_stats::EventSource::Pasted => {
                chap.pasted_chars += event.pasted_chars as u64
            }
            crate::writing_stats::EventSource::Deleted => {
                chap.deleted_chars += event.deleted_chars as u64
            }
            crate::writing_stats::EventSource::AiInserted => {
                chap.ai_inserted_chars += event.ai_inserted_chars as u64
            }
            _ => {}
        }
        chap.net_delta_chars += event.net_delta_chars as i64;

        self.store.save_or_merge_daily_stats(&stats)?;
        Ok(())
    }

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

        let bucket_ms = bucket_minutes as i64 * 60 * 1000;
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
                chars_in_bucket as f64 / minutes
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
            Platform::Linux,
            "proj1",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            10,
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
            Platform::Linux,
            "proj1",
            "vol1",
            "chap1",
            EventSource::Pasted,
            0,
            0,
            20,
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
            Platform::Linux,
            "proj1",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            "session-1",
        );
        agg.aggregate_single_event(&event1).unwrap();

        let event2 = WritingInputEvent::new(
            "device-1",
            Platform::Linux,
            "proj1",
            "vol1",
            "chap1",
            EventSource::Deleted,
            0,
            3,
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
            Platform::Linux,
            "proj1",
            "vol1",
            "chap1",
            EventSource::AiInserted,
            0,
            0,
            0,
            50,
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
            Platform::Linux,
            "proj-abc",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            15,
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
            Platform::Linux,
            "proj1",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            "session-1",
        );
        agg.aggregate_single_event(&event1).unwrap();

        let event2 = WritingInputEvent::new(
            "device-android",
            Platform::Android,
            "proj1",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            20,
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
            platform: Platform::Linux,
            project_id: "proj1".to_string(),
            volume_id: "vol1".to_string(),
            chapter_id: "chap1".to_string(),
            source: EventSource::HumanTyped,
            inserted_chars: 30,
            deleted_chars: 0,
            pasted_chars: 0,
            ai_inserted_chars: 0,
            net_delta_chars: 30,
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
