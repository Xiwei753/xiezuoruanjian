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
    DateRangeDto, WritingStatsExplorerDto, WritingStatsHeatmapDayDto, WritingStatsHeatmapDto,
    WritingStatsLifetimeDto, WritingStatsPeriodCardDto, WritingStatsRecordDto,
    WritingStatsRecordsDto, WritingStatsSelectedPeriodDto, WritingStatsTrendDto,
    WritingStatsTrendPointDto,
};
use crate::error::Result;
use crate::writing_stats::calendar::{self, StatsPreset};
use crate::writing_stats::projection::{
    apply_measured_session_breakdown, bucket_trend, project_all_time_stats_with_sessions,
    project_events, project_lifetime_stats, project_measured_session_breakdown,
    project_measured_sessions, project_range_stats, project_range_stats_with_sessions,
    CurrentWritingSpeed, ProjectLifetimeStatsAgg, RangeStatsAgg, StatsProjection, StatsRecord,
    StatsSummary, TrendGranularity,
};
use crate::writing_stats::store::StatsStore;
use crate::writing_stats::{DateRange, WritingActivitySegment, WritingInputEvent};
use serde_json::Value;
use std::path::Path;

/// 每日码字日历（#956）接受的公历年份范围：早于此范围或过远的年份直接报错，
/// 不伪造成全年零数据。
pub(crate) const HEATMAP_MIN_YEAR: i32 = 1970;
pub(crate) const HEATMAP_MAX_YEAR: i32 = 2100;

/// 校验每日日历查询的年份范围；Core 查询与 FFI 参数校验共用同一份规则。
pub(crate) fn validate_heatmap_year(year: i32) -> Result<()> {
    if (HEATMAP_MIN_YEAR..=HEATMAP_MAX_YEAR).contains(&year) {
        return Ok(());
    }
    Err(crate::Error::Other(format!("invalid heatmap year: {year}")))
}

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
    /// 旧事件库迁移失败状态。
    ///
    /// 非空表示旧单机事件库尚未完整并入按设备分区的新事件库：
    /// 此时读取查询必须显式失败（统计页显示明确原因），不能把旧历史
    /// 静默当成 0 字。下一次查询会重试迁移，成功后清除。
    migration_error: std::sync::Mutex<Option<String>>,
}

impl StatsApi {
    pub fn new(app_data_root: &Path) -> Self {
        // 迁移：把旧版单机事件库一次性并入按设备分区的新事件库。
        let migration_error = if let Err(e) =
            crate::writing_stats::migration::migrate_stats_event_store(app_data_root)
        {
            eprintln!("[writing_stats] event store migration failed: {}", e);
            Some(e.to_string())
        } else {
            None
        };
        Self {
            store: StatsStore::new(app_data_root),
            migration_error: std::sync::Mutex::new(migration_error),
        }
    }

    pub fn store(&self) -> &StatsStore {
        &self.store
    }

    /// 读取查询前的迁移闸门：失败时显式报错并允许下一次重试。
    ///
    /// 迁移成功前不能把新目录当成唯一事实源；`record_event` 不受此限制，
    /// 迁移重试会按 `event_id` 幂等并入旧事件，不会重复计数。
    fn ensure_migrated(&self) -> Result<()> {
        let pending = self
            .migration_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if pending.is_none() {
            return Ok(());
        }
        match crate::writing_stats::migration::migrate_stats_event_store(self.store.app_data_root())
        {
            Ok(()) => {
                *self
                    .migration_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
                Ok(())
            }
            Err(e) => {
                *self
                    .migration_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(e.to_string());
                Err(crate::Error::Other(format!(
                    "writing stats migration incomplete: {e}"
                )))
            }
        }
    }

    /// 统计数据的真实覆盖范围。
    ///
    /// - `localEvents`：只有本机事件，且统计设备同步没有报错；
    /// - `syncedDevices`：已包含其它设备的事件，且同步没有报错；
    /// - `partialSync`：任一统计设备同步组最近同步失败，历史可能不完整。
    ///
    /// 不因「同步开关已打开」就宣称全量成功：只有真实拿到的事件参与聚合。
    pub fn history_scope(&self) -> String {
        if self.stats_sync_incomplete() {
            return "partialSync".to_string();
        }
        let local_origin = self.store.load_origin_device_id();
        let has_remote_data = match self.store.list_event_origins() {
            Ok(origins) => origins
                .iter()
                .any(|origin| Some(origin.as_str()) != local_origin.as_deref()),
            Err(e) => {
                eprintln!("[writing_stats] list event origins failed: {e}");
                false
            }
        };
        if has_remote_data {
            "syncedDevices".to_string()
        } else {
            "localEvents".to_string()
        }
    }

    /// 任一统计设备同步组最近一次同步是否报错。
    ///
    /// 读不到组状态时按「未报错」处理，避免把正常的首次同步误标为不完整。
    fn stats_sync_incomplete(&self) -> bool {
        let groups_dir = self.store.app_data_root().join("app-meta/sync/groups");
        let Ok(entries) = std::fs::read_dir(&groups_dir) else {
            return false;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("stats-device-") {
                continue;
            }
            if !entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let state_path = entry.path().join("state.local.json");
            let Ok(bytes) = std::fs::read(&state_path) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            let last_error = value
                .get("last_error")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if !last_error.is_empty() {
                return true;
            }
        }
        false
    }

    /// 「今天」的写作汇总 —— 今天是哪一天由 **Core 的本地日历口径**决定。
    pub fn get_today_stats_summary(&self) -> Result<Value> {
        self.ensure_migrated()?;
        let today = crate::writing_stats::calendar::local_today_date();
        let range = DateRange {
            start_date: today.clone(),
            end_date: today,
        };
        self.get_stats_summary(&range)
    }

    pub fn get_stats_summary(&self, range: &DateRange) -> Result<Value> {
        self.ensure_migrated()?;
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let segments = self
            .store
            .load_segments_range(&range.start_date, &range.end_date)?;
        let measured = project_measured_sessions(
            &events,
            &segments,
            Some((&range.start_date, &range.end_date)),
        )?;
        let mut proj = project_events(&events, 60, None);
        proj.summary.total_active_seconds = measured.total_active_seconds;
        proj.summary.total_sessions =
            u32::try_from(measured.session_count).unwrap_or(u32::MAX);

        Ok(summary_json(range, &proj.summary))
    }

    pub fn get_stats_by_project(&self, range: &DateRange) -> Result<Value> {
        self.ensure_migrated()?;
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let segments = self
            .store
            .load_segments_range(&range.start_date, &range.end_date)?;
        let breakdown = project_measured_session_breakdown(
            &segments,
            Some((&range.start_date, &range.end_date)),
        );
        let mut proj = project_events(&events, 60, None);
        apply_measured_session_breakdown(&mut proj, &breakdown);
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
        self.ensure_migrated()?;
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let segments = self
            .store
            .load_segments_range(&range.start_date, &range.end_date)?;
        let breakdown = project_measured_session_breakdown(
            &segments,
            Some((&range.start_date, &range.end_date)),
        );
        let mut proj = project_events(&events, 60, None);
        apply_measured_session_breakdown(&mut proj, &breakdown);
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
        self.ensure_migrated()?;
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let segments = self
            .store
            .load_segments_range(&range.start_date, &range.end_date)?;
        let breakdown = project_measured_session_breakdown(
            &segments,
            Some((&range.start_date, &range.end_date)),
        );
        let mut proj = project_events(&events, 60, None);
        apply_measured_session_breakdown(&mut proj, &breakdown);
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
        self.ensure_migrated()?;
        let events = self
            .store
            .load_events_range(&range.start_date, &range.end_date)?;
        let segments = self
            .store
            .load_segments_range(&range.start_date, &range.end_date)?;
        let breakdown = project_measured_session_breakdown(
            &segments,
            Some((&range.start_date, &range.end_date)),
        );
        let mut proj = project_events(&events, 60, None);
        apply_measured_session_breakdown(&mut proj, &breakdown);
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
        self.ensure_migrated()?;
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
        self.ensure_migrated()?;
        let (preset, custom_range) =
            calendar::parse_explorer_query(preset, custom_start, custom_end)?;
        let today = calendar::local_today_date();

        let events = self.store.load_all_events()?;
        let segments = self.store.load_all_segments()?;
        let career = project_all_time_stats_with_sessions(&events, &segments)?;
        let history_scope = self.history_scope();
        // 覆盖起点必须来自完整事件集合，不能用被区间截断后的 daily 投影。
        let coverage_anchor = events_coverage_anchor(&events);
        let first_writing_date = career.daily_human_typed_chars.keys().next().cloned();
        let last_writing_date = career.daily_human_typed_chars.keys().next_back().cloned();

        let elapsed_days = elapsed_calendar_days(first_writing_date.as_deref(), &today)?;
        let lifetime = WritingStatsLifetimeDto {
            total_human_typed_chars: career.total_human_typed_chars,
            first_writing_date: first_writing_date.clone(),
            last_writing_date,
            elapsed_calendar_days: elapsed_days,
            writing_days: career.writing_days,
            // 生涯日均按自然日历天数（首次写作日到今天）均摊，不排除休息日。
            average_chars_per_day: average_chars_per_day(
                career.total_human_typed_chars,
                elapsed_days,
            ),
            timed_human_typed_chars: career.timed_human_typed_chars,
            measured_writing_seconds: career.total_active_seconds,
            average_chars_per_hour: average_chars_per_hour(
                career.timed_human_typed_chars,
                career.total_active_seconds,
            ),
            has_measured_timing: career.total_active_seconds > 0,
        };

        // 四张时段卡顺序固定：近七天、近三十天、近半年、自选区间。
        let mut period_cards = Vec::with_capacity(calendar::PERIOD_CARD_PRESETS.len());
        for card_preset in calendar::PERIOD_CARD_PRESETS {
            period_cards.push(period_card(
                card_preset,
                &events,
                &segments,
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
            _ => project_range_stats_with_sessions(
                &events,
                &segments,
                &range.start_date,
                &range.end_date,
            )?,
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

        // 选中时段的日均按区间自然日历天数均摊（含无输入日），与时段卡同口径。
        let selected_days = calendar::day_span_inclusive(&range.start_date, &range.end_date)?;
        // 逐日热力格与 selected 同一份区间投影，和 trend 的周/月聚合无关。
        let heatmap_days = build_heatmap_days(
            &range.start_date,
            &range.end_date,
            &selected_stats.daily_human_typed_chars,
            coverage_anchor.as_deref(),
            &today,
            history_scope == "partialSync",
        )?;

        Ok(WritingStatsExplorerDto {
            history_scope,
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
                    selected_days,
                ),
                timed_human_typed_chars: selected_stats.timed_human_typed_chars,
                measured_writing_seconds: selected_stats.total_active_seconds,
                average_chars_per_hour: average_chars_per_hour(
                    selected_stats.timed_human_typed_chars,
                    selected_stats.total_active_seconds,
                ),
                has_measured_timing: selected_stats.total_active_seconds > 0,
                heatmap_days,
                records: records_dto(&selected_stats),
                trend: WritingStatsTrendDto {
                    granularity: granularity.as_str().to_string(),
                    points,
                },
            },
        })
    }

    /// 每日码字日历快照（#956）：指定公历年的逐日人工输入真值。
    ///
    /// 与 Explorer 一样一次 [`StatsStore::load_all_events`] 读盘，复用
    /// `project_range_stats` 的 `daily_human_typed_chars` 投影；不把周/月
    /// 趋势点拆成假每日值。已过但无记录的日期为 0；未来日期按日期字段
    /// 原样给出，由端侧标记为「未到」而不是「没写作」。
    ///
    /// `intensity` 由 Core 从该年正数日的字数分布分级（0–4，见
    /// [`heatmap_intensity_thresholds`]），端侧不重算人工字数；非法年份
    /// 返回结构化错误，不造全年零数据。
    pub fn get_heatmap_snapshot(&self, year: i32) -> Result<WritingStatsHeatmapDto> {
        self.ensure_migrated()?;
        validate_heatmap_year(year)?;
        let start_date = format!("{year:04}-01-01");
        let end_date = format!("{year:04}-12-31");

        let events = self.store.load_all_events()?;
        // 覆盖范围与 Explorer 同源：更早未建立记录、或统计同步失败的区间不能画成真实 0。
        let history_scope = self.history_scope();
        let sync_incomplete = history_scope == "partialSync";
        let coverage_anchor = events_coverage_anchor(&events);
        let today = calendar::local_today_date();
        let stats = project_range_stats(&events, &start_date, &end_date)?;
        let days = build_heatmap_days(
            &start_date,
            &end_date,
            &stats.daily_human_typed_chars,
            coverage_anchor.as_deref(),
            &today,
            sync_incomplete,
        )?;

        Ok(WritingStatsHeatmapDto {
            year,
            start_date,
            end_date,
            history_scope,
            days,
        })
    }

    /// 统计面板一次性快照（#935）：一次读盘，按 Core 本地业务日历切分
    /// 今日 / 本周（周一开始）/ 本月，本周范围的作品、章节、设备与速度曲线
    /// 都从同一份投影取。
    ///
    /// 面板不再逐个 Getter 重复 `load_events_range + project_events` 连续扫描
    /// 事件文件；日期口径统一由 Core 日历决定，平台端不自己算周一起始。
    pub fn get_dashboard_snapshot(&self) -> Result<Value> {
        self.ensure_migrated()?;
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

        let segments = self.store.load_segments_range(load_start, &today)?;
        let measured_today =
            project_measured_sessions(&today_events, &segments, Some((&today, &today)))?;
        let measured_week = project_measured_sessions(
            &week_events,
            &segments,
            Some((&week_start, &today)),
        )?;
        let measured_month = project_measured_sessions(
            &month_events,
            &segments,
            Some((&month_start, &today)),
        )?;

        let mut today_summary = project_events(&today_events, 60, None).summary;
        let mut week_projection = project_events(&week_events, 60, None);
        let mut month_summary = project_events(&month_events, 60, None).summary;
        let week_breakdown = project_measured_session_breakdown(
            &segments,
            Some((&week_start, &today)),
        );
        apply_measured_session_breakdown(&mut week_projection, &week_breakdown);
        week_projection.summary.total_active_seconds = measured_week.total_active_seconds;
        week_projection.summary.total_sessions =
            u32::try_from(measured_week.session_count).unwrap_or(u32::MAX);
        today_summary.total_active_seconds = measured_today.total_active_seconds;
        today_summary.total_sessions =
            u32::try_from(measured_today.session_count).unwrap_or(u32::MAX);
        month_summary.total_active_seconds = measured_month.total_active_seconds;
        month_summary.total_sessions =
            u32::try_from(measured_month.session_count).unwrap_or(u32::MAX);

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
        self.ensure_migrated()?;
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

    /// 记录一段平台已测量的前台写作计时切片。
    ///
    /// Core 负责补齐设备身份、按本地日期切分并幂等持久化；平台端只上报
    /// 单调时钟测得的真实片段，不自行计算速度。
    pub fn record_activity_segment(&self, segment: WritingActivitySegment) -> Result<()> {
        self.ensure_migrated()?;
        self.store.record_activity_segments(vec![segment])?;
        Ok(())
    }

    /// 批量记录计时切片（用于异步队列一次提交多段）。
    pub fn record_activity_segments(
        &self,
        segments: Vec<WritingActivitySegment>,
    ) -> Result<usize> {
        self.ensure_migrated()?;
        self.store.record_activity_segments(segments)
    }

    /// 「当前写作速度」：以调用时刻为终点的实时纯输入速度。
    pub fn get_current_speed(&self, window_seconds: u32) -> Result<CurrentWritingSpeed> {
        self.ensure_migrated()?;
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

/// 完整事件集合的最早业务日（不受查询区间截断）。
///
/// 用于区分「该日早于统计起点、覆盖未知」与「已确认覆盖且确实没写」。
fn events_coverage_anchor(events: &[WritingInputEvent]) -> Option<String> {
    events
        .iter()
        .map(|event| event.business_date())
        .filter(|date| !date.is_empty())
        .min()
}

/// 生成一段闭区间内的逐日热力格，供全年日历与 Explorer 选中时段共用。
///
/// - `daily_human_typed_chars` 必须是该区间的逐日人工输入投影；
/// - `coverage_anchor` 必须来自未截断的完整事件集合；
/// - 未来未到 / 已确认（可为真实 0）/ 覆盖不完整或同步失败三种状态在此判定；
/// - `intensity` 按本区间正数日的分位阈值分级，端侧不重算。
fn build_heatmap_days(
    start_date: &str,
    end_date: &str,
    daily_human_typed_chars: &std::collections::BTreeMap<String, u64>,
    coverage_anchor: Option<&str>,
    today: &str,
    sync_incomplete: bool,
) -> Result<Vec<WritingStatsHeatmapDayDto>> {
    let positive: Vec<u64> = daily_human_typed_chars
        .values()
        .copied()
        .filter(|value| *value > 0)
        .collect();
    let thresholds = heatmap_intensity_thresholds(positive);

    let start = calendar::parse_date(start_date)?;
    let end = calendar::parse_date(end_date)?;
    let mut days = Vec::new();
    let mut date = start;
    while date <= end {
        let date_str = date.format("%Y-%m-%d").to_string();
        let human_typed_chars = daily_human_typed_chars
            .get(&date_str)
            .copied()
            .unwrap_or(0);
        // 单元格覆盖状态：未来未到 / 已确认（可为真实 0）/ 覆盖不完整或同步失败。
        let status = if date_str.as_str() > today {
            "future"
        } else if human_typed_chars > 0 {
            "known"
        } else if sync_incomplete {
            "unknown"
        } else if coverage_anchor.is_some_and(|anchor| date_str.as_str() >= anchor) {
            "known"
        } else {
            "unknown"
        };
        days.push(WritingStatsHeatmapDayDto {
            date: date_str,
            human_typed_chars,
            intensity: heatmap_intensity(human_typed_chars, thresholds),
            status: status.to_string(),
        });
        if date == end {
            break;
        }
        date = date.checked_add_days(chrono::Days::new(1)).ok_or_else(|| {
            crate::Error::Other(format!(
                "heatmap date overflow for {start_date}..={end_date}"
            ))
        })?;
    }
    Ok(days)
}

/// 每日码字热力图的强度阈值：给定范围正数日字数的 25% / 50% / 75% 分位（最近阶）。
///
/// 没有正数日时三个阈值都是 0（所有日期强度为 0）。所有正数相同（或只有
/// 一个正数日）时三个阈值相等：判定用严格小于，这些日期都会拿到最高强度 4，
/// 同一组数据的显示始终稳定，不出现除零或 NaN。
fn heatmap_intensity_thresholds(mut positive: Vec<u64>) -> [u64; 3] {
    if positive.is_empty() {
        return [0; 3];
    }
    positive.sort_unstable();
    let last = positive.len() - 1;
    let pick = |quarter: usize| -> u64 {
        let index = positive.len() * quarter / 4;
        positive[index.min(last)]
    };
    [pick(1), pick(2), pick(3)]
}

/// 单日强度：0（无输入）恒为 0；正数按分位阈值落入 1–4，最高档为 4。
fn heatmap_intensity(value: u64, thresholds: [u64; 3]) -> u8 {
    if value == 0 {
        return 0;
    }
    if value < thresholds[0] {
        1
    } else if value < thresholds[1] {
        2
    } else if value < thresholds[2] {
        3
    } else {
        4
    }
}

/// 组装一张时段卡；`custom` 未选择日期时返回空卡而不是报错。
fn period_card(
    preset: StatsPreset,
    events: &[WritingInputEvent],
    segments: &[WritingActivitySegment],
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
            timed_human_typed_chars: 0,
            measured_writing_seconds: 0,
            average_chars_per_hour: None,
            has_measured_timing: false,
        });
    }
    let range = calendar::resolve_range(preset, today, first_writing_date, custom_range)?;
    let stats = project_range_stats_with_sessions(
        events,
        segments,
        &range.start_date,
        &range.end_date,
    )?;
    // 卡片日均按区间自然日历天数均摊（含无输入日），不按实际写作天数。
    let calendar_days = calendar::day_span_inclusive(&range.start_date, &range.end_date)?;
    Ok(WritingStatsPeriodCardDto {
        preset: preset.as_str().to_string(),
        has_selection: true,
        range: Some(date_range_dto(&range)),
        total_human_typed_chars: stats.total_human_typed_chars,
        total_active_seconds: stats.total_active_seconds,
        writing_days: stats.writing_days,
        average_chars_per_day: average_chars_per_day(stats.total_human_typed_chars, calendar_days),
        timed_human_typed_chars: stats.timed_human_typed_chars,
        measured_writing_seconds: stats.total_active_seconds,
        average_chars_per_hour: average_chars_per_hour(
            stats.timed_human_typed_chars,
            stats.total_active_seconds,
        ),
        has_measured_timing: stats.total_active_seconds > 0,
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

/// 平均每天字数 = 时间范围内人工输入 / 该范围的自然日历天数（含无输入日）。
///
/// 分母是该范围的「陪伴天数」，不是实际写作天数：有效区间没有写字时为 0；
/// 分母为 0（完全空生涯，没有首次输入日）时为 null。
fn average_chars_per_day(total_chars: u64, calendar_days: u64) -> Option<f64> {
    if calendar_days == 0 {
        return None;
    }
    Some(total_chars as f64 / calendar_days as f64)
}

/// 真实平均写作速度（字/时）= 计时覆盖的人工输入 × 3600 / 已测量秒数。
///
/// 分子必须是 [`RangeStatsAgg::timed_human_typed_chars`]，不能再用累计总字数；
/// 没有计时样本时返回 null，界面显示「—」，不拿旧事件时间差凑分母。
fn average_chars_per_hour(timed_chars: u64, measured_seconds: u64) -> Option<f64> {
    if measured_seconds == 0 {
        return None;
    }
    Some(timed_chars as f64 * 3600.0 / measured_seconds as f64)
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
