//! # 写作统计 API 模块
//!
//! 所有查询方法走同一个「load events -> project_events -> 取需要的部分」入口。
//! 不再封装 `StatsAggregator`，直接持有 `StatsStore`。
//!
//! ## 返回格式
//!
//! 既有查询方法返回 `serde_json::Value`，便于直接序列化为 JSON 响应；
//! Explorer 一次性快照（#944）返回强类型 DTO，由服务层统一转 JSON。

use crate::api::types::{
    DateRangeDto, WritingStatsExplorerDto, WritingStatsLifetimeDto, WritingStatsPeriodCardDto,
    WritingStatsRecordDto, WritingStatsRecordsDto, WritingStatsSelectedPeriodDto,
    WritingStatsTrendDto, WritingStatsTrendPointDto,
};
use crate::error::Result;
use crate::writing_stats::calendar::{self, StatsPreset};
use crate::writing_stats::projection::{
    bucket_trend, project_all_time_stats, project_events, project_lifetime_stats,
    project_range_stats, CurrentWritingSpeed, ProjectLifetimeStatsAgg, RangeStatsAgg,
    StatsProjection, StatsRecord, StatsSummary, TrendGranularity,
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

    /// 统计页 Explorer 一次性快照（#944）：生涯摘要 + 四张时段卡 + 当前选中时段详情。
    ///
    /// 一次 [`StatsStore::load_all_events`] 读盘，所有时段共用同一份事件：
    /// 不逐卡重复扫描事件文件，也不让端侧各自算天数、纪录或趋势。
    ///
    /// - `preset` 固定为 `"last7Days"` / `"last30Days"` / `"last6Months"` /
    ///   `"lifetime"` / `"custom"`；`custom` 必须带有效自选区间；
    /// - `custom_start` / `custom_end` 是可选的用户自选闭区间（同给同给）；
    /// - 生涯/纪录/平均/趋势的口径全部由 Core 决定，端侧只做展示。
    pub fn get_explorer_snapshot(
        &self,
        preset: &str,
        custom_start: Option<&str>,
        custom_end: Option<&str>,
    ) -> Result<WritingStatsExplorerDto> {
        let (preset, custom_range) =
            calendar::parse_explorer_query(preset, custom_start, custom_end)?;
        let today = calendar::local_today_date();

        let events = self.store.load_all_events()?;
        let career = project_all_time_stats(&events)?;
        let first_writing_date = career.daily_human_typed_chars.keys().next().cloned();
        let last_writing_date = career.daily_human_typed_chars.keys().next_back().cloned();

        let lifetime = WritingStatsLifetimeDto {
            total_human_typed_chars: career.total_human_typed_chars,
            first_writing_date: first_writing_date.clone(),
            last_writing_date,
            elapsed_calendar_days: elapsed_calendar_days(first_writing_date.as_deref(), &today)?,
            writing_days: career.writing_days,
            average_chars_per_day: average_chars_per_day(
                career.total_human_typed_chars,
                career.writing_days,
            ),
            average_chars_per_minute: average_chars_per_minute(
                career.total_human_typed_chars,
                career.total_active_seconds,
            ),
        };

        // 四张时段卡顺序固定：近七天、近三十天、近半年、自选区间。
        let mut period_cards = Vec::with_capacity(calendar::PERIOD_CARD_PRESETS.len());
        for card_preset in calendar::PERIOD_CARD_PRESETS {
            period_cards.push(period_card(
                card_preset,
                &events,
                &today,
                first_writing_date.as_deref(),
                custom_range.as_ref(),
            )?);
        }

        let range = calendar::resolve_range(
            preset,
            &today,
            first_writing_date.as_deref(),
            custom_range.as_ref(),
        )?;
        // 生涯在上面已经聚合过，直接复用；其余时段在区间过滤后独立聚合。
        let selected_stats = match preset {
            StatsPreset::Lifetime => career,
            _ => project_range_stats(&events, &range.start_date, &range.end_date)?,
        };
        let granularity = trend_granularity(preset, &range)?;
        let points = if preset == StatsPreset::Lifetime && first_writing_date.is_none() {
            // 完全没有人工输入时没有「首次写作所在月」，返回空点列，
            // 不靠补一个 0 月冒充生涯起点。
            Vec::new()
        } else {
            bucket_trend(
                &selected_stats.daily_human_typed_chars,
                granularity,
                &range.start_date,
                &range.end_date,
            )?
            .into_iter()
            .map(|point| WritingStatsTrendPointDto {
                date: point.date,
                human_typed_chars: point.human_typed_chars,
            })
            .collect()
        };

        Ok(WritingStatsExplorerDto {
            history_scope: "localEvents".to_string(),
            lifetime,
            period_cards,
            selected: WritingStatsSelectedPeriodDto {
                preset: preset.as_str().to_string(),
                range: date_range_dto(&range),
                total_human_typed_chars: selected_stats.total_human_typed_chars,
                total_active_seconds: selected_stats.total_active_seconds,
                writing_days: selected_stats.writing_days,
                average_chars_per_day: average_chars_per_day(
                    selected_stats.total_human_typed_chars,
                    selected_stats.writing_days,
                ),
                average_chars_per_minute: average_chars_per_minute(
                    selected_stats.total_human_typed_chars,
                    selected_stats.total_active_seconds,
                ),
                records: records_dto(&selected_stats),
                trend: WritingStatsTrendDto {
                    granularity: granularity.as_str().to_string(),
                    points,
                },
            },
        })
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

/// 组装一张时段卡；`custom` 未选择日期时返回空卡而不是报错。
fn period_card(
    preset: StatsPreset,
    events: &[WritingInputEvent],
    today: &str,
    first_writing_date: Option<&str>,
    custom_range: Option<&DateRange>,
) -> Result<WritingStatsPeriodCardDto> {
    if preset == StatsPreset::Custom && custom_range.is_none() {
        return Ok(WritingStatsPeriodCardDto {
            preset: preset.as_str().to_string(),
            has_selection: false,
            range: None,
            total_human_typed_chars: 0,
            total_active_seconds: 0,
            writing_days: 0,
            average_chars_per_day: None,
            average_chars_per_minute: None,
        });
    }
    let range = calendar::resolve_range(preset, today, first_writing_date, custom_range)?;
    let stats = project_range_stats(events, &range.start_date, &range.end_date)?;
    Ok(WritingStatsPeriodCardDto {
        preset: preset.as_str().to_string(),
        has_selection: true,
        range: Some(date_range_dto(&range)),
        total_human_typed_chars: stats.total_human_typed_chars,
        total_active_seconds: stats.total_active_seconds,
        writing_days: stats.writing_days,
        average_chars_per_day: average_chars_per_day(
            stats.total_human_typed_chars,
            stats.writing_days,
        ),
        average_chars_per_minute: average_chars_per_minute(
            stats.total_human_typed_chars,
            stats.total_active_seconds,
        ),
    })
}

/// 选中时段的趋势粒度：短区间按天、半年按周、生涯按月，自选按跨度。
fn trend_granularity(preset: StatsPreset, range: &DateRange) -> Result<TrendGranularity> {
    Ok(match preset {
        StatsPreset::Last7Days | StatsPreset::Last30Days => TrendGranularity::Day,
        StatsPreset::Last6Months => TrendGranularity::Week,
        StatsPreset::Lifetime => TrendGranularity::Month,
        StatsPreset::Custom => TrendGranularity::for_span(&range.start_date, &range.end_date)?,
    })
}

/// 「陪伴 N 天」：首次写作日到今天（含两端）的自然日历天数。
///
/// 没有人工输入的历史返回 0；未来日期（系统时钟被改快）不产出反向跨度。
fn elapsed_calendar_days(first_writing_date: Option<&str>, today: &str) -> Result<u64> {
    match first_writing_date {
        Some(first) if first <= today => calendar::day_span_inclusive(first, today),
        _ => Ok(0),
    }
}

/// 平均每天字数 = 区间人工输入 / 区间内有输入的天数；没有输入日为 null。
fn average_chars_per_day(total_chars: u64, writing_days: u64) -> Option<f64> {
    if writing_days == 0 {
        return None;
    }
    Some(total_chars as f64 / writing_days as f64)
}

/// 平均写作速度（字/分）= 区间人工输入 × 60 / 区间有效写作时长；零时长时为 null。
fn average_chars_per_minute(total_chars: u64, active_seconds: u64) -> Option<f64> {
    if active_seconds == 0 {
        return None;
    }
    Some(total_chars as f64 * 60.0 / active_seconds as f64)
}

fn date_range_dto(range: &DateRange) -> DateRangeDto {
    DateRangeDto {
        start_date: range.start_date.clone(),
        end_date: range.end_date.clone(),
    }
}

fn record_dto(record: Option<&StatsRecord>) -> WritingStatsRecordDto {
    match record {
        Some(record) => WritingStatsRecordDto {
            date: Some(record.date.clone()),
            end_date: record.end_date.clone(),
            value: Some(record.value),
        },
        None => WritingStatsRecordDto {
            date: None,
            end_date: None,
            value: None,
        },
    }
}

fn records_dto(stats: &RangeStatsAgg) -> WritingStatsRecordsDto {
    WritingStatsRecordsDto {
        best_day: record_dto(stats.best_day.as_ref()),
        fastest_hour: record_dto(stats.fastest_hour.as_ref()),
        longest_session: record_dto(stats.longest_session.as_ref()),
        longest_streak: record_dto(stats.longest_streak.as_ref()),
    }
}
