//! # 写作统计 API 模块
//!
//! 所有查询方法走同一个「load events -> project_events -> 取需要的部分」入口。
//! 不再封装 `StatsAggregator`，直接持有 `StatsStore`。
//!
//! ## 返回格式
//!
//! 所有查询方法返回 `serde_json::Value`，便于直接序列化为 JSON 响应。

use crate::error::Result;
use crate::writing_stats::projection::{
    project_events, project_lifetime_stats, CurrentWritingSpeed, ProjectLifetimeStatsAgg,
    StatsProjection, StatsSummary,
};
use crate::writing_stats::store::StatsStore;
use crate::writing_stats::{DateRange, WritingInputEvent};
use serde_json::Value;
use std::path::Path;

/// 把写作汇总构造成稳定 camelCase JSON —— 现有区间查询与面板快照共用同一口径。
fn summary_json(range: &DateRange, summary: &StatsSummary) -> Value {
    serde_json::json!({
        "range": {
            "startDate": range.start_date,
            "endDate": range.end_date,
        },
        "totalHumanTypedChars": summary.total_human_typed_chars,
        "totalPastedChars": summary.total_pasted_chars,
        "totalDeletedChars": summary.total_deleted_chars,
        "totalAiInsertedChars": summary.total_ai_inserted_chars,
        "totalNetDeltaChars": summary.total_net_delta_chars,
        "totalActiveSeconds": summary.total_active_seconds,
        "totalSessions": summary.total_sessions,
        "daysCount": summary.days_count,
    })
}

/// 过滤出业务日落在 `[start_date, end_date]` 内的事件副本。
fn events_within(
    events: &[WritingInputEvent],
    start_date: &str,
    end_date: &str,
) -> Vec<WritingInputEvent> {
    events
        .iter()
        .filter(|e| {
            let date = e.business_date();
            date.as_str() >= start_date && date.as_str() <= end_date
        })
        .cloned()
        .collect()
}

/// 作品排行记录（与既有区间 Getter 同形的 camelCase 字段）。
fn project_records_json(projection: &StatsProjection) -> Vec<Value> {
    projection
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
        .collect()
}

/// 章节排行记录（与既有区间 Getter 同形的 camelCase 字段）。
fn chapter_records_json(projection: &StatsProjection) -> Vec<Value> {
    projection
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
        .collect()
}

/// 设备记录（含 platform / deviceClass / sessionsCount）。
fn device_records_json(projection: &StatsProjection) -> Vec<Value> {
    projection
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
        .collect()
}

/// 速度曲线分桶（与既有区间 Getter 同形的 camelCase 字段）。
fn speed_buckets_json(projection: &StatsProjection) -> Vec<Value> {
    projection
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
        .collect()
}

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

        Ok(summary_json(range, &proj.summary))
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

    /// 单个作品的历史写作档案（#934）。
    ///
    /// 只读本地事件历史（`events.local` 不同步），不写任何缓存账本；
    /// 作品没有任何人工输入时返回全 0 且无首次写作日期。
    pub fn get_project_lifetime_stats(&self, project_id: &str) -> Result<ProjectLifetimeStatsAgg> {
        let events = self.store.load_events_for_project(project_id)?;
        Ok(project_lifetime_stats(&events, project_id))
    }

    /// 统计面板一次性快照（#935）：一次读盘，按 Core 本地业务日历切分
    /// 今日 / 本周（周一开始）/ 本月，本周范围的作品、章节、设备与速度曲线
    /// 都从同一份投影取。
    ///
    /// 面板不再逐个 Getter 重复 `load_events_range + project_events` 连续扫描
    /// 事件文件；日期口径统一由 Core 日历决定，平台端不自己算周一起始。
    pub fn get_dashboard_snapshot(&self) -> Result<Value> {
        let today = crate::writing_stats::calendar::local_today_date();
        let week_start = crate::writing_stats::calendar::week_start_date(&today)?;
        let month_start = crate::writing_stats::calendar::month_start_date(&today)?;

        // 一次读盘覆盖本周与本月的最早起点；两个范围外的事件不参与。
        let load_start = if week_start <= month_start {
            week_start.as_str()
        } else {
            month_start.as_str()
        };
        let events = self.store.load_events_range(load_start, &today)?;

        let today_events = events_within(&events, &today, &today);
        let week_events = events_within(&events, &week_start, &today);
        let month_events = events_within(&events, &month_start, &today);

        let today_summary = project_events(&today_events, 60, None).summary;
        let week_projection = project_events(&week_events, 60, None);
        let month_summary = project_events(&month_events, 60, None).summary;

        let today_range = DateRange {
            start_date: today.clone(),
            end_date: today.clone(),
        };
        let week_range = DateRange {
            start_date: week_start,
            end_date: today.clone(),
        };
        let month_range = DateRange {
            start_date: month_start,
            end_date: today,
        };

        Ok(serde_json::json!({
            // 顶层 range 描述排行/曲线所在的统计口径：本周。
            "range": {
                "startDate": week_range.start_date,
                "endDate": week_range.end_date,
            },
            "today": summary_json(&today_range, &today_summary),
            "week": summary_json(&week_range, &week_projection.summary),
            "month": summary_json(&month_range, &month_summary),
            "projects": project_records_json(&week_projection),
            "chapters": chapter_records_json(&week_projection),
            "devices": device_records_json(&week_projection),
            "speedCurve": {
                "range": {
                    "startDate": week_range.start_date,
                    "endDate": week_range.end_date,
                },
                "bucketMinutes": 60,
                "buckets": speed_buckets_json(&week_projection),
            },
        }))
    }

    pub fn get_speed_curve(&self, range: &DateRange, bucket_minutes: u32) -> Result<Value> {
        // 钳到至少 1 分钟，避免 bucket_ms = 0 导致速度曲线死循环
        let bucket_minutes = bucket_minutes.max(1);
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
