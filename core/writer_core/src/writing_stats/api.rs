//! # 写作统计 API 模块
//!
//! 所有查询方法走同一个「load events -> project_events -> 取需要的部分」入口。
//! 不再封装 `StatsAggregator`，直接持有 `StatsStore`。
//!
//! ## 返回格式
//!
//! 所有查询方法返回 `serde_json::Value`，便于直接序列化为 JSON 响应。

use crate::error::Result;
use crate::writing_stats::projection::{project_events, CurrentWritingSpeed};
use crate::writing_stats::store::StatsStore;
use crate::writing_stats::{DateRange, WritingInputEvent};
use serde_json::Value;
use std::path::Path;

/// 写作统计 API 入口 — 直接持有 `StatsStore`，所有查询走 projection。
pub struct StatsApi {
    store: StatsStore,
}

impl StatsApi {
    pub fn new(app_data_root: &Path) -> Self {
        // 迁移：补齐旧事件的 local_date（不再重建 daily）
        if let Err(e) =
            crate::writing_stats::migration::migrate_stats_to_local_calendar(app_data_root)
        {
            eprintln!("[writing_stats] calendar migration skipped: {}", e);
        }
        Self {
            store: StatsStore::new(app_data_root),
        }
    }

    pub fn store(&self) -> &StatsStore {
        &self.store
    }

    /// 「今天」的写作汇总 —— 今天是哪一天由 **Core 的本地日历口径**决定。
    pub fn get_today_stats_summary(&self) -> Result<Value> {
        let today = crate::writing_stats::calendar::local_today_date();
        let range = DateRange {
            start_date: today.clone(),
            end_date: today,
        };
        self.get_stats_summary(&range)
    }

    pub fn get_stats_summary(&self, range: &DateRange) -> Result<Value> {
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let proj = project_events(&events, 60, None);

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "totalHumanTypedChars": proj.summary.total_human_typed_chars,
            "totalPastedChars": proj.summary.total_pasted_chars,
            "totalDeletedChars": proj.summary.total_deleted_chars,
            "totalAiInsertedChars": proj.summary.total_ai_inserted_chars,
            "totalNetDeltaChars": proj.summary.total_net_delta_chars,
            "totalActiveSeconds": proj.summary.total_active_seconds,
            "totalSessions": proj.summary.total_sessions,
            "daysCount": 0, // 不再按 daily 文件计数
        }))
    }

    pub fn get_stats_by_project(&self, range: &DateRange) -> Result<Value> {
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let proj = project_events(&events, 60, None);

        let projects: Vec<Value> = proj
            .per_project
            .values()
            .map(|p| {
                serde_json::json!({
                    "projectId": p.project_id,
                    "humanTypedChars": p.human_typed_chars,
                    "pastedChars": p.pasted_chars,
                    "deletedChars": p.deleted_chars,
                    "aiInsertedChars": p.ai_inserted_chars,
                    "netDeltaChars": p.net_delta_chars,
                    "activeSeconds": p.active_seconds,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "projects": projects,
        }))
    }

    pub fn get_stats_by_chapter(&self, range: &DateRange) -> Result<Value> {
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let proj = project_events(&events, 60, None);

        let chapters: Vec<Value> = proj
            .per_chapter
            .values()
            .map(|c| {
                serde_json::json!({
                    "chapterId": c.chapter_id,
                    "humanTypedChars": c.human_typed_chars,
                    "pastedChars": c.pasted_chars,
                    "deletedChars": c.deleted_chars,
                    "aiInsertedChars": c.ai_inserted_chars,
                    "netDeltaChars": c.net_delta_chars,
                    "activeSeconds": c.active_seconds,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "chapters": chapters,
        }))
    }

    pub fn get_stats_by_device(&self, range: &DateRange) -> Result<Value> {
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let proj = project_events(&events, 60, None);

        let devices: Vec<Value> = proj
            .per_device
            .values()
            .map(|d| {
                serde_json::json!({
                    "deviceId": d.device_id,
                    "platform": d.platform,
                    "deviceClass": d.device_class,
                    "humanTypedChars": d.human_typed_chars,
                    "pastedChars": d.pasted_chars,
                    "deletedChars": d.deleted_chars,
                    "aiInsertedChars": d.ai_inserted_chars,
                    "netDeltaChars": d.net_delta_chars,
                    "activeSeconds": d.active_seconds,
                    "sessionsCount": d.sessions_count,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "devices": devices,
        }))
    }

    pub fn get_stats_by_device_class(&self, range: &DateRange) -> Result<Value> {
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let proj = project_events(&events, 60, None);

        let classes: Vec<Value> = proj
            .per_device_class
            .values()
            .map(|c| {
                serde_json::json!({
                    "deviceClass": c.device_class,
                    "deviceCount": c.device_count,
                    "totalHumanTypedChars": c.total_human_typed_chars,
                    "totalNetDeltaChars": c.total_net_delta_chars,
                    "activeSeconds": c.active_seconds,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "deviceClasses": classes,
        }))
    }

    pub fn get_speed_curve(&self, range: &DateRange, bucket_minutes: u32) -> Result<Value> {
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let proj = project_events(&events, bucket_minutes, None);

        let bucket_json: Vec<Value> = proj
            .speed_curve
            .iter()
            .map(|b| {
                serde_json::json!({
                    "startMs": b.start_ms,
                    "endMs": b.end_ms,
                    "charsTyped": b.chars_typed,
                    "charsPerMinute": b.chars_per_minute,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "bucketMinutes": bucket_minutes,
            "buckets": bucket_json,
        }))
    }

    /// 记录写作事件 — 只持久化事件，不再调 aggregate_single_event。
    pub fn record_event(&self, event: WritingInputEvent) -> Result<()> {
        self.store.record_event(event)?;
        Ok(())
    }

    /// 「当前写作速度」：以调用时刻为终点的实时纯输入速度。
    pub fn get_current_speed(&self, window_seconds: u32) -> Result<CurrentWritingSpeed> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let window_seconds = window_seconds.max(1);
        let start_ms = now_ms - i64::from(window_seconds) * 1_000;

        let events = self.store.load_events_in_window(start_ms, now_ms)?;
        let proj = project_events(&events, 60, Some(window_seconds));
        Ok(proj.current_speed.unwrap_or(CurrentWritingSpeed {
            window_seconds,
            sampled_at_ms: now_ms,
            chars_typed: 0,
            chars_per_minute: 0.0,
        }))
    }

    /// 本机当前的业务日历日（本地时区，非 UTC）。
    pub fn today_date() -> String {
        crate::writing_stats::calendar::local_today_date()
    }
}
