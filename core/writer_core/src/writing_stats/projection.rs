//! # 统计投影模块 — 单一事件事实源、单一聚合口径
//!
//! 所有统计计算集中在一个 [`project_events()`] 函数里。输入只是一段
//! `&[WritingInputEvent]`，一次遍历同时生成所有维度的统计结果。
//!
//! ## 关键规则
//!
//! - **真实活跃时间只来自 `.sessions.jsonl` 的已测量切片**：同设备重叠切片
//!   取并集，跨设备相加；不再用两次输入的时间差推算。
//! - **历史速度和当前速度都只统计 `EventSource::HumanTyped`**：Undo/Redo/
//!   Programmatic 的 inserted 不进入速度。
//! - **速度曲线分桶为半开区间 `[bucket_start, bucket_end)`**。
//! - **`SESSION_GAP_MS` 仅保留给旧数据读取/兼容聚合**：它无法区分思考与离开，
//!   不得再作为速度分母或真实最长会话依据。

use crate::error::{Error, Result};
use crate::writing_stats::calendar;
use crate::writing_stats::{EventSource, WritingActivitySegment, WritingInputEvent};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// 两次事件间隔超过此值（毫秒）视为不同会话。5 分钟与常见 IDE 的会话超时一致。
pub const SESSION_GAP_MS: i64 = 5 * 60 * 1000;

/// 速度桶 — 半开区间 `[start_ms, end_ms)` 内的输入字符统计。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedBucket {
    pub start_ms: i64,
    pub end_ms: i64,
    pub chars_typed: u32,
    pub chars_per_minute: f64,
}

/// 「当前写作速度」：最近 `window_seconds` 秒窗口内的纯输入速度。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CurrentWritingSpeed {
    pub window_seconds: u32,
    pub sampled_at_ms: i64,
    pub chars_typed: u32,
    pub chars_per_minute: f64,
}

/// 按项目聚合的统计数据。活跃时间是**独立计算的**，只看该 project 的事件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectStatsAgg {
    pub project_id: String,
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
}

/// 按章节聚合的统计数据。活跃时间是**独立计算的**，只看该 chapter 的事件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChapterStatsAgg {
    pub chapter_id: String,
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
}

/// 按设备聚合的统计数据。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceStatsAgg {
    pub device_id: String,
    pub platform: String,
    pub device_class: String,
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
    pub sessions_count: u32,
}

/// 按设备类别聚合的统计数据。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceClassStatsAgg {
    pub device_class: String,
    pub device_count: u32,
    pub total_human_typed_chars: u64,
    pub total_net_delta_chars: i64,
    pub active_seconds: u64,
}

/// 总汇统计。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsSummary {
    pub total_human_typed_chars: u64,
    pub total_pasted_chars: u64,
    pub total_deleted_chars: u64,
    pub total_ai_inserted_chars: u64,
    pub total_net_delta_chars: i64,
    pub total_active_seconds: u64,
    pub total_sessions: u32,
    pub days_count: u32,
}

/// 统计投影结果 — 包含所有维度的统计数据。
///
/// 由 [`project_events()`] 一次遍历生成，所有维度共用同一份事件事实源。
#[derive(Debug, Clone, Default)]
pub struct StatsProjection {
    pub summary: StatsSummary,
    pub per_project: HashMap<String, ProjectStatsAgg>,
    pub per_chapter: HashMap<String, ChapterStatsAgg>,
    pub per_device: HashMap<String, DeviceStatsAgg>,
    pub per_device_class: HashMap<String, DeviceClassStatsAgg>,
    pub speed_curve: Vec<SpeedBucket>,
    pub current_speed: Option<CurrentWritingSpeed>,
}

/// 从一段事件生成所有维度的统计投影。
///
/// 这是唯一的统计聚合入口：所有查询方法都走 `load events -> project_events -> 取需要的部分`。
///
/// # 参数
///
/// - `events`: 原始写作输入事件切片
/// - `bucket_minutes`: 速度曲线分桶粒度（分钟）
/// - `current_speed_window_seconds`: 当前速度窗口长度（秒）；`None` 表示不计算当前速度
pub fn project_events(
    events: &[WritingInputEvent],
    bucket_minutes: u32,
    current_speed_window_seconds: Option<u32>,
) -> StatsProjection {
    let mut projection = StatsProjection::default();

    if events.is_empty() {
        return projection;
    }

    // ── 按维度分组事件 ──
    let mut by_device: HashMap<&str, Vec<&WritingInputEvent>> = HashMap::new();
    let mut by_project: HashMap<&str, Vec<&WritingInputEvent>> = HashMap::new();
    let mut by_chapter: HashMap<&str, Vec<&WritingInputEvent>> = HashMap::new();

    for event in events {
        by_device
            .entry(event.device_id.as_str())
            .or_default()
            .push(event);
        by_project
            .entry(event.project_id.as_str())
            .or_default()
            .push(event);
        by_chapter
            .entry(event.chapter_id.as_str())
            .or_default()
            .push(event);
    }

    // ── Summary（全局汇总：字符部分）──
    projection.summary = compute_summary_chars(events);

    // ── Per-device ──
    for (device_id, device_events) in &by_device {
        let mut device_stats = DeviceStatsAgg {
            device_id: device_id.to_string(),
            ..Default::default()
        };

        // 取设备元信息（从第一条事件）
        if let Some(first) = device_events.first() {
            device_stats.platform = first.platform.to_string();
            device_stats.device_class = first.device_class.clone();
        }

        accumulate_chars(
            &mut device_stats.human_typed_chars,
            &mut device_stats.pasted_chars,
            &mut device_stats.deleted_chars,
            &mut device_stats.ai_inserted_chars,
            &mut device_stats.net_delta_chars,
            device_events,
        );

        let (sessions, active_ms) = compute_sessions_and_active_time(device_events);
        device_stats.sessions_count = sessions;
        device_stats.active_seconds = u64::try_from(active_ms / 1000).unwrap_or(0);

        projection
            .per_device
            .insert(device_id.to_string(), device_stats);
    }

    // ── Summary: sessions/active_seconds 从 per_device 累加 ──
    // 不再用全设备混合时间线计算，避免多设备在相近时间写作被当成一个 session。
    for device_stats in projection.per_device.values() {
        projection.summary.total_sessions += device_stats.sessions_count;
        projection.summary.total_active_seconds += device_stats.active_seconds;
    }

    // ── Summary: days_count 从 business_date() 去重 ──
    let mut business_dates: std::collections::HashSet<String> = std::collections::HashSet::new();
    for event in events {
        business_dates.insert(event.business_date());
    }
    projection.summary.days_count = u32::try_from(business_dates.len()).unwrap_or(0);

    // ── Per-project（活跃时间独立计算）──
    for (project_id, project_events) in &by_project {
        let mut proj_stats = ProjectStatsAgg {
            project_id: project_id.to_string(),
            ..Default::default()
        };

        accumulate_chars(
            &mut proj_stats.human_typed_chars,
            &mut proj_stats.pasted_chars,
            &mut proj_stats.deleted_chars,
            &mut proj_stats.ai_inserted_chars,
            &mut proj_stats.net_delta_chars,
            project_events,
        );

        let (_, active_ms) = compute_sessions_and_active_time(project_events);
        proj_stats.active_seconds = u64::try_from(active_ms / 1000).unwrap_or(0);

        projection
            .per_project
            .insert(project_id.to_string(), proj_stats);
    }

    // ── Per-chapter（活跃时间独立计算）──
    for (chapter_id, chapter_events) in &by_chapter {
        let mut chap_stats = ChapterStatsAgg {
            chapter_id: chapter_id.to_string(),
            ..Default::default()
        };

        accumulate_chars(
            &mut chap_stats.human_typed_chars,
            &mut chap_stats.pasted_chars,
            &mut chap_stats.deleted_chars,
            &mut chap_stats.ai_inserted_chars,
            &mut chap_stats.net_delta_chars,
            chapter_events,
        );

        let (_, active_ms) = compute_sessions_and_active_time(chapter_events);
        chap_stats.active_seconds = u64::try_from(active_ms / 1000).unwrap_or(0);

        projection
            .per_chapter
            .insert(chapter_id.to_string(), chap_stats);
    }

    // ── Per-device-class ──
    projection.per_device_class = compute_device_class_summary(&projection.per_device);

    // ── Speed curve（只计 HumanTyped）──
    projection.speed_curve = compute_speed_curve(events, bucket_minutes);

    // ── Current speed（只计 HumanTyped）──
    if let Some(window_seconds) = current_speed_window_seconds {
        projection.current_speed = Some(compute_current_speed(events, window_seconds));
    }

    projection
}

/// 作品级写作档案聚合结果（#934）。
///
/// `total_human_typed_chars` 与 [`project_events`] 的人工输入口径一致，
/// 但单次遍历直接累计，不计算无关的速度曲线；日期纪录同样直接从原始事件取。
/// 注意这只是**当前设备可读的本地事件历史**：
/// `events.local` 不参与跨设备同步，不代表全设备终身输入量。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectLifetimeStatsAgg {
    pub project_id: String,
    pub total_human_typed_chars: u64,
    pub first_written_at_ms: Option<i64>,
    pub best_writing_day: Option<String>,
    pub best_writing_day_chars: u64,
    pub writing_days: u64,
}

/// 计算单个作品的历史写作档案。
///
/// - 只统计 `EventSource::HumanTyped` 且 `inserted_chars > 0` 的事件：
///   粘贴 / AI 插入 / 同步 / 程序加载 / 撤销都不算「写作」。
/// - 日期用事件保存的 [`WritingInputEvent::business_date`]（创作当地日历日），
///   不按当前时区改写旧日期。
/// - `best_writing_day_chars` 是当天**实际人工输入量**，不是净增量；
///   并列时取更早的日期。
/// - 没有任何人工输入时：`first_written_at_ms` / `best_writing_day` 为
///   `None`，字数与写作天数为 0。
pub fn project_lifetime_stats(
    events: &[WritingInputEvent],
    project_id: &str,
) -> ProjectLifetimeStatsAgg {
    // 单次遍历同时累计人工码字、最早开写时间、每日产量与写作天数。
    // 不复用 `project_events`：它会连带计算速度曲线（按小时分桶并逐桶重扫
    // 事件），作品历史变长后，查询「累计码字」会被无关计算拖慢。
    let mut total_human_typed_chars: u64 = 0;
    let mut daily = std::collections::BTreeMap::<String, u64>::new();
    let mut first_written_at_ms: Option<i64> = None;
    for event in events.iter().filter(|e| e.project_id == project_id) {
        if event.source != EventSource::HumanTyped || event.inserted_chars == 0 {
            continue;
        }
        total_human_typed_chars += u64::from(event.inserted_chars);
        first_written_at_ms =
            Some(first_written_at_ms.map_or(event.timestamp_ms, |old| old.min(event.timestamp_ms)));
        *daily.entry(event.business_date()).or_default() += u64::from(event.inserted_chars);
    }

    // BTreeMap 按日期升序；并列时 `then_with` 选更早的日期。
    let best = daily.iter().max_by(|(day_a, count_a), (day_b, count_b)| {
        count_a.cmp(count_b).then_with(|| day_b.cmp(day_a))
    });
    let (best_writing_day, best_writing_day_chars) = match best {
        Some((day, count)) => (Some(day.clone()), *count),
        None => (None, 0),
    };

    ProjectLifetimeStatsAgg {
        project_id: project_id.to_string(),
        total_human_typed_chars,
        first_written_at_ms,
        best_writing_day,
        best_writing_day_chars,
        writing_days: u64::try_from(daily.len()).unwrap_or(0),
    }
}

/// 单条统计纪录（区间聚合结果）。
///
/// `date` / `end_date` 是业务日历日；`value` 单位随纪录种类变化
/// （最多码字日=当天字数、单小时最多码字=自然 60 分钟窗口内字数、
/// 最长会话=已测量秒数、最长连续段=天数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsRecord {
    pub date: String,
    pub end_date: Option<String>,
    pub value: u64,
}

/// 一段区间（或全量生涯）的写作聚合结果（#944）。
///
/// 只统计 `EventSource::HumanTyped` 的 `inserted_chars` 作为「人工码字」：
/// 粘贴 / AI / 同步 / 撤销不计，后来被删掉的字仍计入。
/// `total_active_seconds` 和会话数只来自真实计时切片；老事件时间差只保留在
/// 旧投影兼容字段里，绝不参与速度和最长会话。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RangeStatsAgg {
    pub total_human_typed_chars: u64,
    /// 真实的已测量写作秒数。带 `*_with_sessions` 的投影会覆盖成切片并集；
    /// 旧投影无计时事实时保持 0，绝不拿事件时间差冒充。
    pub total_active_seconds: u64,
    /// 实际计时覆盖的人工输入字数（落在已测量切片内）。
    pub timed_human_typed_chars: u64,
    /// 已测量会话数量（按 `session_id` 去重）。
    pub measured_session_count: u64,
    /// 区间内有人工输入的不同业务日数量（= `daily_human_typed_chars` 的长度）。
    pub writing_days: u64,
    /// 有人工输入的日期 → 当天字数；没有输入的日期不在表里（由趋势补 0）。
    pub daily_human_typed_chars: BTreeMap<String, u64>,
    pub best_day: Option<StatsRecord>,
    pub fastest_hour: Option<StatsRecord>,
    pub longest_session: Option<StatsRecord>,
    pub longest_streak: Option<StatsRecord>,
}

/// 全应用生涯聚合（#944）：不过滤日期，口径是「全部已到达的事件」。
///
/// 事件来自本机分区与已同步设备的只读分区，按 `event_id` 去重；
/// DTO 用 `historyScope` 显式说明真实覆盖范围（`localEvents` /
/// `syncedDevices` / `partialSync`）。
///
/// **注意**：这个旧入口没有计时切片，`total_active_seconds` 是事件时间差
/// 推算值，只能用于历史展示；速度、真实活跃时长和最长会话请用
/// [`project_all_time_stats_with_sessions`]。
pub fn project_all_time_stats(events: &[WritingInputEvent]) -> Result<RangeStatsAgg> {
    aggregate_range_stats(events, None)
}

/// 全应用生涯聚合 + 真实前台计时切片。
pub fn project_all_time_stats_with_sessions(
    events: &[WritingInputEvent],
    segments: &[WritingActivitySegment],
) -> Result<RangeStatsAgg> {
    let mut agg = aggregate_range_stats(events, None)?;
    apply_measured_sessions(&mut agg, events, segments, None)?;
    Ok(agg)
}

/// 区间聚合：只统计业务日落在 `[start_date, end_date]`（含两端）内的写作。
///
/// 字数和单小时最多码字只看区间内的事件；真实计时请用
/// [`project_range_stats_with_sessions`]；`start_date > end_date` 直接报错。
pub fn project_range_stats(
    events: &[WritingInputEvent],
    start_date: &str,
    end_date: &str,
) -> Result<RangeStatsAgg> {
    if start_date > end_date {
        return Err(Error::Other(format!(
            "Invalid stats range: {} .. {} (start must not be after end)",
            start_date, end_date
        )));
    }
    aggregate_range_stats(events, Some((start_date, end_date)))
}

/// 区间聚合 + 真实前台计时切片。
pub fn project_range_stats_with_sessions(
    events: &[WritingInputEvent],
    segments: &[WritingActivitySegment],
    start_date: &str,
    end_date: &str,
) -> Result<RangeStatsAgg> {
    if start_date > end_date {
        return Err(Error::Other(format!(
            "Invalid stats range: {} .. {} (start must not be after end)",
            start_date, end_date
        )));
    }
    let mut agg = aggregate_range_stats(events, Some((start_date, end_date)))?;
    apply_measured_sessions(
        &mut agg,
        events,
        segments,
        Some((start_date, end_date)),
    )?;
    Ok(agg)
}

/// 趋势聚合粒度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendGranularity {
    Day,
    Week,
    Month,
}

impl TrendGranularity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }

    /// 自选区间按跨度选粒度：≤31 天按天、≤200 天按周、更长按月。
    pub fn for_span(start_date: &str, end_date: &str) -> Result<Self> {
        let days = calendar::day_span_inclusive(start_date, end_date)?;
        Ok(if days <= 31 {
            Self::Day
        } else if days <= 200 {
            Self::Week
        } else {
            Self::Month
        })
    }
}

/// 趋势点：桶起始日 + 桶内人工输入字数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrendPointAgg {
    pub date: String,
    pub human_typed_chars: u64,
}

/// 把按业务日的每日人工输入聚合成趋势点列（日期升序、无数据补 0）。
///
/// - `Day`：区间内每天一个点；
/// - `Week`：周一对齐（复用 [`calendar::week_start_date`]），覆盖与区间相交的所有周；
/// - `Month`：每月 1 号，覆盖区间内所有日历月（生涯即首次写作所在月到今天所在月）。
///
/// `daily` 里区间外的日期（含未来日期）不会计入区间趋势点。
pub fn bucket_trend(
    daily: &BTreeMap<String, u64>,
    granularity: TrendGranularity,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<TrendPointAgg>> {
    if start_date > end_date {
        return Err(Error::Other(format!(
            "Invalid trend range: {} .. {} (start must not be after end)",
            start_date, end_date
        )));
    }
    match granularity {
        TrendGranularity::Day => {
            let start = calendar::parse_date(start_date)?;
            let end = calendar::parse_date(end_date)?;
            let mut points = Vec::new();
            let mut cursor = start;
            while cursor <= end {
                let date = cursor.format("%Y-%m-%d").to_string();
                points.push(TrendPointAgg {
                    human_typed_chars: daily.get(&date).copied().unwrap_or(0),
                    date,
                });
                cursor = cursor.succ_opt().ok_or_else(|| {
                    Error::Other(format!("Invalid trend cursor date: {}", cursor))
                })?;
            }
            Ok(points)
        }
        TrendGranularity::Week => {
            let first = calendar::week_start_date(start_date)?;
            let last = calendar::week_start_date(end_date)?;
            let buckets = sum_into_buckets(daily, calendar::week_start_date, start_date, end_date)?;
            fill_buckets(&buckets, &first, &last, |date| {
                calendar::shift_date(date, 7)
            })
        }
        TrendGranularity::Month => {
            let first = calendar::month_start_date(start_date)?;
            let last = calendar::month_start_date(end_date)?;
            let buckets =
                sum_into_buckets(daily, calendar::month_start_date, start_date, end_date)?;
            fill_buckets(&buckets, &first, &last, |date| {
                calendar::shift_months(date, 1)
            })
        }
    }
}

/// 按桶起始日归并区间内的每日字数。
///
/// 只归并 `[start_date, end_date]`（含两端）内的输入：区间外或未来日期
/// 不会污染区间趋势点。
fn sum_into_buckets<F>(
    daily: &BTreeMap<String, u64>,
    bucket_of: F,
    start_date: &str,
    end_date: &str,
) -> Result<BTreeMap<String, u64>>
where
    F: Fn(&str) -> Result<String>,
{
    let mut buckets: BTreeMap<String, u64> = BTreeMap::new();
    for (date, chars) in daily {
        if date.as_str() < start_date || date.as_str() > end_date {
            continue;
        }
        let key = bucket_of(date)?;
        *buckets.entry(key).or_default() += chars;
    }
    Ok(buckets)
}

/// 从 `first` 到 `last` 按 `step` 逐桶补点（无数据补 0）。
fn fill_buckets<F>(
    buckets: &BTreeMap<String, u64>,
    first: &str,
    last: &str,
    step: F,
) -> Result<Vec<TrendPointAgg>>
where
    F: Fn(&str) -> Result<String>,
{
    let mut points = Vec::new();
    let mut cursor = first.to_string();
    while cursor.as_str() <= last {
        points.push(TrendPointAgg {
            human_typed_chars: buckets.get(&cursor).copied().unwrap_or(0),
            date: cursor.clone(),
        });
        let next = step(&cursor)?;
        if next <= cursor {
            // 防止 step 不前进时死循环；正常日期推进不会触发。
            return Err(Error::Other(format!(
                "Trend bucket step did not advance from {}",
                cursor
            )));
        }
        cursor = next;
    }
    Ok(points)
}

/// 区间聚合的唯一实现：一次遍历分组 + 每设备排序算会话。
fn aggregate_range_stats(
    events: &[WritingInputEvent],
    range: Option<(&str, &str)>,
) -> Result<RangeStatsAgg> {
    let mut daily: BTreeMap<String, u64> = BTreeMap::new();
    let mut human_events: Vec<&WritingInputEvent> = Vec::new();
    let mut by_device: HashMap<&str, Vec<&WritingInputEvent>> = HashMap::new();

    for event in events {
        let date = event.business_date();
        // 空串是坏时间戳的兜底值（老数据）；不属于任何业务日。
        if date.is_empty() || !date_in_range(&date, range) {
            continue;
        }
        if event.source == EventSource::HumanTyped && event.inserted_chars > 0 {
            *daily.entry(date).or_default() += u64::from(event.inserted_chars);
            human_events.push(event);
        }
        by_device
            .entry(event.device_id.as_str())
            .or_default()
            .push(event);
    }

    // 活跃时间/最长会话：每台设备按自己的时间线独立算，不并成一条。
    let mut total_active_seconds: u64 = 0;
    let mut longest: Option<(i64, &WritingInputEvent)> = None;
    for device_events in by_device.values() {
        let mut sorted = device_events.clone();
        sorted.sort_by_key(|event| event.timestamp_ms);
        let mut device_active_ms: i64 = 0;
        for (start_index, duration_ms) in split_sessions(&sorted) {
            device_active_ms += duration_ms;
            if duration_ms <= 0 {
                continue;
            }
            let start_event = sorted[start_index];
            let is_better = match longest {
                None => true,
                Some((best_ms, best_start)) => {
                    duration_ms > best_ms
                        || (duration_ms == best_ms
                            && start_event.timestamp_ms < best_start.timestamp_ms)
                }
            };
            if is_better {
                longest = Some((duration_ms, start_event));
            }
        }
        total_active_seconds += u64::try_from(device_active_ms / 1000).unwrap_or(0);
    }

    let best_day = daily
        .iter()
        .max_by(|(day_a, chars_a), (day_b, chars_b)| {
            chars_a.cmp(chars_b).then_with(|| day_b.cmp(day_a))
        })
        .map(|(date, chars)| StatsRecord {
            date: date.clone(),
            end_date: None,
            value: *chars,
        });

    let longest_session = longest.map(|(duration_ms, start_event)| StatsRecord {
        date: start_event.business_date(),
        end_date: None,
        value: u64::try_from(duration_ms / 1000).unwrap_or(0),
    });

    Ok(RangeStatsAgg {
        total_human_typed_chars: daily.values().sum(),
        total_active_seconds,
        timed_human_typed_chars: 0,
        measured_session_count: 0,
        writing_days: u64::try_from(daily.len()).unwrap_or(0),
        best_day,
        fastest_hour: compute_fastest_hour(&mut human_events),
        longest_session,
        longest_streak: compute_longest_streak(&daily)?,
        daily_human_typed_chars: daily,
    })
}

/// 按设备/作品/章节拆分的真实计时聚合。
///
/// 每组的时长都按“同设备切片并集、跨设备相加”计算；没有计时切片的组保持
/// 缺省值，调用方应保留该组原有的展示字段，而不是用事件时间差补齐。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeasuredSessionBreakdown {
    pub by_device: HashMap<String, MeasuredDeviceAgg>,
    pub by_project: HashMap<String, u64>,
    pub by_chapter: HashMap<String, u64>,
    pub by_device_class: HashMap<String, u64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeasuredDeviceAgg {
    pub active_seconds: u64,
    pub session_count: u64,
}

/// 按来源设备/作品/章节/设备类别聚合真实计时切片（#981）。
pub fn project_measured_session_breakdown(
    segments: &[WritingActivitySegment],
    range: Option<(&str, &str)>,
) -> MeasuredSessionBreakdown {
    let mut device_intervals: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
    let mut project_groups: HashMap<String, HashMap<String, Vec<(i64, i64)>>> = HashMap::new();
    let mut chapter_groups: HashMap<String, HashMap<String, Vec<(i64, i64)>>> = HashMap::new();
    let mut class_groups: HashMap<String, HashMap<String, Vec<(i64, i64)>>> = HashMap::new();
    let mut device_sessions: HashMap<String, std::collections::HashSet<String>> = HashMap::new();

    for segment in segments {
        let date = segment.business_date();
        if date.is_empty()
            || !date_in_range(&date, range)
            || segment.end_at_ms <= segment.start_at_ms
            || segment.device_id.is_empty()
        {
            continue;
        }
        let interval = (segment.start_at_ms, segment.end_at_ms);
        device_intervals
            .entry(segment.device_id.clone())
            .or_default()
            .push(interval);
        if !segment.session_id.is_empty() {
            device_sessions
                .entry(segment.device_id.clone())
                .or_default()
                .insert(segment.session_id.clone());
        }
        if !segment.project_id.is_empty() {
            project_groups
                .entry(segment.project_id.clone())
                .or_default()
                .entry(segment.device_id.clone())
                .or_default()
                .push(interval);
        }
        if !segment.chapter_id.is_empty() {
            chapter_groups
                .entry(segment.chapter_id.clone())
                .or_default()
                .entry(segment.device_id.clone())
                .or_default()
                .push(interval);
        }
        class_groups
            .entry(segment.device_class.clone())
            .or_default()
            .entry(segment.device_id.clone())
            .or_default()
            .push(interval);
    }

    let mut breakdown = MeasuredSessionBreakdown::default();
    for (device_id, intervals) in device_intervals {
        let seconds = merged_seconds(&intervals);
        let session_count = device_sessions
            .get(&device_id)
            .map(|sessions| u64::try_from(sessions.len()).unwrap_or(0))
            .unwrap_or(0);
        breakdown.by_device.insert(
            device_id,
            MeasuredDeviceAgg {
                active_seconds: seconds,
                session_count,
            },
        );
    }
    breakdown.by_project = summed_group_seconds(project_groups);
    breakdown.by_chapter = summed_group_seconds(chapter_groups);
    breakdown.by_device_class = summed_group_seconds(class_groups);
    breakdown
}

/// 组内先按设备并集，再跨设备相加。
fn summed_group_seconds(
    groups: HashMap<String, HashMap<String, Vec<(i64, i64)>>>,
) -> HashMap<String, u64> {
    groups
        .into_iter()
        .map(|(key, devices)| {
            let seconds = devices
                .values()
                .map(|intervals| merged_seconds(intervals))
                .sum();
            (key, seconds)
        })
        .collect()
}

fn merged_seconds(intervals: &[(i64, i64)]) -> u64 {
    let total_ms: i64 = merge_intervals(intervals)
        .iter()
        .map(|(start, end)| end - start)
        .sum();
    u64::try_from(total_ms / 1000).unwrap_or(0)
}

/// 把真实计时拆分覆盖到既有投影的分组字段上。
pub fn apply_measured_session_breakdown(
    projection: &mut StatsProjection,
    breakdown: &MeasuredSessionBreakdown,
) {
    for project in projection.per_project.values_mut() {
        project.active_seconds = breakdown
            .by_project
            .get(&project.project_id)
            .copied()
            .unwrap_or(0);
    }
    for chapter in projection.per_chapter.values_mut() {
        chapter.active_seconds = breakdown
            .by_chapter
            .get(&chapter.chapter_id)
            .copied()
            .unwrap_or(0);
    }
    for device in projection.per_device.values_mut() {
        let measured = breakdown.by_device.get(&device.device_id);
        device.active_seconds = measured.map_or(0, |value| value.active_seconds);
        device.sessions_count = measured
            .map(|value| u32::try_from(value.session_count).unwrap_or(u32::MAX))
            .unwrap_or(0);
    }
    for class in projection.per_device_class.values_mut() {
        class.active_seconds = breakdown
            .by_device_class
            .get(&class.device_class)
            .copied()
            .unwrap_or(0);
    }
}

/// 真实计时切片聚合结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeasuredSessionAgg {
    /// 同设备切片并集后，跨设备相加的真实写作秒数。
    pub total_active_seconds: u64,
    /// 落在这些切片内的人工输入字数。
    pub timed_human_typed_chars: u64,
    /// 按 `session_id` 去重的已测量会话数。
    pub session_count: u64,
    /// 最长已测量会话。
    pub longest_session: Option<StatsRecord>,
}

/// 汇总真实前台计时切片，并把人工输入对时到切片窗口。
///
/// - 同一设备上重叠/重复的切片取并集，绝不重复累计；
/// - 不同设备并行写作按各自设备计时相加；
/// - 只统计业务日落在 `range` 内、且 `end > start` 的切片；
/// - 人工输入只有在同设备某个已测量窗口内才计入 `timed_human_typed_chars`，
///   历史数据没有计时覆盖就只保留总字数，不进入速度分母。
pub fn project_measured_sessions(
    events: &[WritingInputEvent],
    segments: &[WritingActivitySegment],
    range: Option<(&str, &str)>,
) -> Result<MeasuredSessionAgg> {
    let mut by_device: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
    let mut by_chapter: HashMap<(String, String, String, String), Vec<(i64, i64)>> =
        HashMap::new();
    let mut by_session: HashMap<(String, String), Vec<(i64, i64)>> = HashMap::new();
    let mut session_dates: HashMap<(String, String), String> = HashMap::new();

    for segment in segments {
        let date = segment.business_date();
        if date.is_empty()
            || !date_in_range(&date, range)
            || segment.end_at_ms <= segment.start_at_ms
        {
            continue;
        }
        let interval = (segment.start_at_ms, segment.end_at_ms);
        by_device
            .entry(segment.device_id.clone())
            .or_default()
            .push(interval);
        if !segment.chapter_id.is_empty() {
            by_chapter
                .entry((
                    segment.device_id.clone(),
                    segment.project_id.clone(),
                    segment.volume_id.clone(),
                    segment.chapter_id.clone(),
                ))
                .or_default()
                .push(interval);
        }
        if !segment.session_id.is_empty() {
            by_session
                .entry((segment.device_id.clone(), segment.session_id.clone()))
                .or_default()
                .push(interval);
            session_dates
                .entry((segment.device_id.clone(), segment.session_id.clone()))
                .and_modify(|existing| {
                    if date < *existing {
                        *existing = date.clone();
                    }
                })
                .or_insert(date);
        }
    }

    let mut total_ms: i64 = 0;
    for intervals in by_device.values() {
        let merged = merge_intervals(intervals);
        total_ms = total_ms.saturating_add(merged.iter().map(|(s, e)| e - s).sum::<i64>());
    }
    let merged_by_chapter: HashMap<&(String, String, String, String), Vec<(i64, i64)>> =
        by_chapter
            .iter()
            .filter(|(_, intervals)| !intervals.is_empty())
            .map(|(key, intervals)| (key, merge_intervals(intervals)))
            .collect();

    let mut timed_human_typed_chars: u64 = 0;
    for event in events {
        if event.source != EventSource::HumanTyped || event.inserted_chars == 0 {
            continue;
        }
        let date = event.business_date();
        if date.is_empty() || !date_in_range(&date, range) {
            continue;
        }
        let key = (
            event.device_id.clone(),
            event.project_id.clone(),
            event.volume_id.clone(),
            event.chapter_id.clone(),
        );
        if merged_by_chapter
            .get(&key)
            .is_some_and(|intervals| interval_contains(intervals, event.timestamp_ms))
        {
            timed_human_typed_chars += u64::from(event.inserted_chars);
        }
    }

    let mut longest: Option<(i64, String)> = None;
    for (key, intervals) in &by_session {
        let duration_ms: i64 = merge_intervals(intervals)
            .iter()
            .map(|(start, end)| end - start)
            .sum();
        if duration_ms <= 0 {
            continue;
        }
        let date = session_dates.get(key).cloned().unwrap_or_default();
        let is_better = match &longest {
            None => true,
            Some((best_ms, best_date)) => {
                duration_ms > *best_ms
                    || (duration_ms == *best_ms && date.as_str() < best_date.as_str())
            }
        };
        if is_better {
            longest = Some((duration_ms, date));
        }
    }

    let longest_session = longest.map(|(duration_ms, date)| StatsRecord {
        date,
        end_date: None,
        value: u64::try_from(duration_ms / 1000).unwrap_or(0),
    });

    Ok(MeasuredSessionAgg {
        total_active_seconds: u64::try_from(total_ms / 1000).unwrap_or(0),
        timed_human_typed_chars,
        session_count: u64::try_from(by_session.len()).unwrap_or(0),
        longest_session,
    })
}

fn apply_measured_sessions(
    agg: &mut RangeStatsAgg,
    events: &[WritingInputEvent],
    segments: &[WritingActivitySegment],
    range: Option<(&str, &str)>,
) -> Result<()> {
    let measured = project_measured_sessions(events, segments, range)?;
    agg.total_active_seconds = measured.total_active_seconds;
    agg.timed_human_typed_chars = measured.timed_human_typed_chars;
    agg.measured_session_count = measured.session_count;
    agg.longest_session = measured.longest_session;
    Ok(())
}

/// 合并同设备上重叠或相接的时间区间，返回按起点升序的并集。
fn merge_intervals(intervals: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut sorted = intervals.to_vec();
    sorted.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (start, end) in sorted {
        if end <= start {
            continue;
        }
        match merged.last_mut() {
            Some((_, current_end)) if start <= *current_end => {
                if end > *current_end {
                    *current_end = end;
                }
            }
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// 半开区间 `[start, end)` 是否包含某时间戳。
fn interval_contains(intervals: &[(i64, i64)], timestamp_ms: i64) -> bool {
    let index = intervals.partition_point(|(start, _)| *start <= timestamp_ms);
    index > 0 && timestamp_ms < intervals[index - 1].1
}

/// 业务日是否落在可选区间内（闭区间，ISO 日期字符串可直接比较）。
fn date_in_range(date: &str, range: Option<(&str, &str)>) -> bool {
    match range {
        None => true,
        Some((start, end)) => date >= start && date <= end,
    }
}

/// 单小时最多码字：自然 60 分钟**半开**窗口 `[t, t+60min)` 内最高人工输入量。
///
/// 以每条人工输入事件时间戳为锚，恰好落在 `t + 60min` 的事件不算入。
/// 并列时取更早的窗口（严格大于才替换）。
fn compute_fastest_hour(human_events: &mut [&WritingInputEvent]) -> Option<StatsRecord> {
    human_events.sort_by_key(|event| event.timestamp_ms);
    let window_ms: i64 = 60 * 60 * 1000;
    let mut best: Option<(u64, &WritingInputEvent)> = None;
    let mut window_chars: u64 = 0;
    let mut right = 0usize;
    for anchor in human_events.iter() {
        let window_end = anchor.timestamp_ms.saturating_add(window_ms);
        while right < human_events.len() && human_events[right].timestamp_ms < window_end {
            window_chars += u64::from(human_events[right].inserted_chars);
            right += 1;
        }
        let is_better = match best {
            None => true,
            Some((best_chars, _)) => window_chars > best_chars,
        };
        if is_better {
            best = Some((window_chars, anchor));
        }
        window_chars -= u64::from(anchor.inserted_chars);
    }
    best.map(|(chars, anchor)| StatsRecord {
        date: anchor.business_date(),
        end_date: None,
        value: chars,
    })
}

/// 最长连续有输入日期段：并列时取更早的一段。
fn compute_longest_streak(daily: &BTreeMap<String, u64>) -> Result<Option<StatsRecord>> {
    let mut entries: Vec<(&str, chrono::NaiveDate)> = Vec::with_capacity(daily.len());
    for date in daily.keys() {
        entries.push((date.as_str(), calendar::parse_date(date)?));
    }

    let mut best: Option<(usize, usize, u64)> = None;
    let mut run_start = 0usize;
    for index in 0..entries.len() {
        if index > 0 && entries[index - 1].1.succ_opt() != Some(entries[index].1) {
            run_start = index;
        }
        let run_days = u64::try_from(index - run_start + 1).unwrap_or(0);
        let is_better = match best {
            None => true,
            Some((_, _, best_days)) => run_days > best_days,
        };
        if is_better {
            best = Some((run_start, index, run_days));
        }
    }

    Ok(best.map(|(start, end, days)| StatsRecord {
        date: entries[start].0.to_string(),
        end_date: Some(entries[end].0.to_string()),
        value: days,
    }))
}

/// 从事件列表计算总汇统计的字符部分。
///
/// `total_sessions` 和 `total_active_seconds` 不在这里算——它们从 per_device
/// 累加，避免多设备时间线揉成一条。`days_count` 也从事件 `business_date()`
/// 去重得到。
fn compute_summary_chars(events: &[WritingInputEvent]) -> StatsSummary {
    let mut summary = StatsSummary::default();

    let all_sorted: Vec<&WritingInputEvent> = events.iter().collect();

    accumulate_chars(
        &mut summary.total_human_typed_chars,
        &mut summary.total_pasted_chars,
        &mut summary.total_deleted_chars,
        &mut summary.total_ai_inserted_chars,
        &mut summary.total_net_delta_chars,
        &all_sorted,
    );

    summary
}

/// 累加字符计数器（按 EventSource 分发）。
///
/// - `deleted_chars`：对所有非 Unknown/SyncRemote 的事件独立累计（替换操作
///   中 Typing/Paste 也带 deleted_chars，必须计入正文删除）。
/// - `source` 只决定新增字数归属：HumanTyped→inserted、Pasted→pasted、
///   AiInserted→ai_inserted。Deleted/Unknown/SyncRemote 不新增分类字数。
/// - `net_delta_chars`：对所有事件都累计。
fn accumulate_chars(
    human_typed: &mut u64,
    pasted: &mut u64,
    deleted: &mut u64,
    ai_inserted: &mut u64,
    net_delta: &mut i64,
    events: &[&WritingInputEvent],
) {
    for event in events {
        // deleted_chars 对所有非 Unknown/SyncRemote 的事件独立累计
        if event.source != EventSource::Unknown && event.source != EventSource::SyncRemote {
            *deleted += u64::from(event.deleted_chars);
        }

        // source 决定新增字数归属
        match event.source {
            EventSource::HumanTyped => *human_typed += u64::from(event.inserted_chars),
            EventSource::Pasted => *pasted += u64::from(event.pasted_chars),
            EventSource::AiInserted => *ai_inserted += u64::from(event.ai_inserted_chars),
            EventSource::Deleted | EventSource::Unknown | EventSource::SyncRemote => {}
        }

        // net_delta_chars 对所有事件都累计
        *net_delta += i64::from(event.net_delta_chars);
    }
}

/// 计算会话数和活跃时间（毫秒）。
///
/// 会话边界：两次事件间隔超过 `SESSION_GAP_MS` 视为不同会话。
/// 活跃时间：会话内从第一个事件到最后一个事件的时长。
fn compute_sessions_and_active_time(events: &[&WritingInputEvent]) -> (u32, i64) {
    if events.is_empty() {
        return (0, 0);
    }
    let mut sorted = events.to_vec();
    sorted.sort_by_key(|event| event.timestamp_ms);
    let sessions = split_sessions(&sorted);
    let active_ms = sessions.iter().map(|(_, duration_ms)| *duration_ms).sum();
    (u32::try_from(sessions.len()).unwrap_or(u32::MAX), active_ms)
}

/// 把按时间升序排好的事件切成写作会话。
///
/// 返回每段会话的 `(起始事件下标, 会话时长毫秒)`；相邻事件间隔**超过**
/// `SESSION_GAP_MS`（5 分钟）才分界，时长是会话内首末事件时间差。
/// 与 [`compute_sessions_and_active_time`] 共用同一份口径：查询区间活跃时间、
/// 最长会话纪录不能各写一套。
fn split_sessions(sorted: &[&WritingInputEvent]) -> Vec<(usize, i64)> {
    let mut sessions = Vec::new();
    let Some(first) = sorted.first() else {
        return sessions;
    };
    let mut start_index = 0usize;
    let mut prev_ms = first.timestamp_ms;
    for (index, event) in sorted.iter().enumerate().skip(1) {
        if event.timestamp_ms - prev_ms > SESSION_GAP_MS {
            sessions.push((start_index, prev_ms - sorted[start_index].timestamp_ms));
            start_index = index;
        }
        prev_ms = event.timestamp_ms;
    }
    sessions.push((start_index, prev_ms - sorted[start_index].timestamp_ms));
    sessions
}

/// 计算速度曲线（只计 HumanTyped 事件）。
///
/// 分桶为半开区间 `[bucket_start, bucket_end)`。
fn compute_speed_curve(events: &[WritingInputEvent], bucket_minutes: u32) -> Vec<SpeedBucket> {
    // 只保留 HumanTyped 事件
    let mut human_events: Vec<&WritingInputEvent> = events
        .iter()
        .filter(|e| e.source == EventSource::HumanTyped)
        .collect();

    if human_events.is_empty() {
        return Vec::new();
    }

    human_events.sort_by_key(|e| e.timestamp_ms);

    let bucket_ms = i64::from(bucket_minutes) * 60 * 1000;
    let (Some(first), Some(last)) = (human_events.first(), human_events.last()) else {
        return Vec::new();
    };
    let first_ms = first.timestamp_ms;
    let last_ms = last.timestamp_ms;

    let mut buckets = Vec::new();
    let mut bucket_start = first_ms;

    while bucket_start <= last_ms {
        let bucket_end = bucket_start + bucket_ms;
        let mut chars_in_bucket: u32 = 0;

        for event in &human_events {
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

    buckets
}

/// 计算当前写作速度（只计 HumanTyped 事件）。
///
/// 以调用时刻为终点、回看 `window_seconds` 秒的纯输入速度。
fn compute_current_speed(events: &[WritingInputEvent], window_seconds: u32) -> CurrentWritingSpeed {
    let now_ms = chrono::Utc::now().timestamp_millis();
    // 0 秒窗口没有意义，钳到 1 秒而不是让除法炸掉。
    let window_seconds = window_seconds.max(1);
    let start_ms = now_ms - i64::from(window_seconds) * 1_000;

    let chars_typed: u32 = events
        .iter()
        .filter(|e| {
            e.source == EventSource::HumanTyped
                && e.timestamp_ms >= start_ms
                && e.timestamp_ms <= now_ms
        })
        .map(|e| e.inserted_chars)
        .sum();

    let minutes = f64::from(window_seconds) / 60.0;

    CurrentWritingSpeed {
        window_seconds,
        sampled_at_ms: now_ms,
        chars_typed,
        chars_per_minute: f64::from(chars_typed) / minutes,
    }
}

/// 按设备类别汇总统计。
///
/// 对于旧数据没有 device_class 字段的情况，根据 platform 推断。
fn compute_device_class_summary(
    per_device: &HashMap<String, DeviceStatsAgg>,
) -> HashMap<String, DeviceClassStatsAgg> {
    let mut result: HashMap<String, DeviceClassStatsAgg> = HashMap::new();

    for device_stats in per_device.values() {
        let class = if device_stats.device_class.is_empty() {
            let kind = crate::writing_stats::Platform::from_str_name(&device_stats.platform)
                .unwrap_or(crate::writing_stats::Platform::Desktop);
            kind.default_device_class().to_string()
        } else {
            device_stats.device_class.clone()
        };

        let entry = result
            .entry(class.clone())
            .or_insert_with(|| DeviceClassStatsAgg {
                device_class: class,
                ..Default::default()
            });

        entry.device_count += 1;
        entry.total_human_typed_chars += device_stats.human_typed_chars;
        entry.total_net_delta_chars += device_stats.net_delta_chars;
        entry.active_seconds += device_stats.active_seconds;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writing_stats::{EventSource, Platform, WritingInputEvent};

    fn make_event(
        timestamp_ms: i64,
        source: EventSource,
        inserted: u32,
        deleted: u32,
        pasted: u32,
        ai_inserted: u32,
    ) -> WritingInputEvent {
        #[allow(clippy::cast_possible_wrap)]
        let net = inserted as i32 + pasted as i32 + ai_inserted as i32 - deleted as i32;
        WritingInputEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            timestamp_ms,
            device_id: "dev-1".to_string(),
            platform: Platform::Desktop,
            device_class: "desktop".to_string(),
            project_id: "proj1".to_string(),
            volume_id: "vol1".to_string(),
            chapter_id: "chap1".to_string(),
            source,
            inserted_chars: inserted,
            deleted_chars: deleted,
            pasted_chars: pasted,
            ai_inserted_chars: ai_inserted,
            net_delta_chars: net,
            duration_seconds: 0,
            session_id: "s1".to_string(),
            local_date: String::new(),
        }
    }

    #[test]
    fn test_projection_summary_human_typed() {
        let events = vec![make_event(1000, EventSource::HumanTyped, 10, 0, 0, 0)];
        let proj = project_events(&events, 1, None);
        assert_eq!(proj.summary.total_human_typed_chars, 10);
        assert_eq!(proj.summary.total_net_delta_chars, 10);
    }

    #[test]
    fn test_projection_summary_pasted() {
        let events = vec![make_event(1000, EventSource::Pasted, 0, 0, 20, 0)];
        let proj = project_events(&events, 1, None);
        assert_eq!(proj.summary.total_human_typed_chars, 0);
        assert_eq!(proj.summary.total_pasted_chars, 20);
    }

    #[test]
    fn test_projection_summary_deleted() {
        let events = vec![make_event(1000, EventSource::Deleted, 0, 5, 0, 0)];
        let proj = project_events(&events, 1, None);
        assert_eq!(proj.summary.total_deleted_chars, 5);
        assert_eq!(proj.summary.total_net_delta_chars, -5);
    }

    #[test]
    fn test_projection_summary_ai_inserted() {
        let events = vec![make_event(1000, EventSource::AiInserted, 0, 0, 0, 50)];
        let proj = project_events(&events, 1, None);
        assert_eq!(proj.summary.total_ai_inserted_chars, 50);
    }

    #[test]
    fn test_projection_per_project_independent_active_time() {
        // project1 有两个事件间隔 10 秒；project2 有一个事件
        let mut e1 = make_event(1000, EventSource::HumanTyped, 10, 0, 0, 0);
        e1.project_id = "proj1".to_string();
        let mut e2 = make_event(11000, EventSource::HumanTyped, 5, 0, 0, 0);
        e2.project_id = "proj1".to_string();
        let mut e3 = make_event(2000, EventSource::HumanTyped, 20, 0, 0, 0);
        e3.project_id = "proj2".to_string();

        let events = vec![e1, e2, e3];
        let proj = project_events(&events, 1, None);

        // proj1 活跃时间 = 10 秒（两个事件间隔）
        let p1 = proj.per_project.get("proj1").unwrap();
        assert_eq!(p1.human_typed_chars, 15);
        assert_eq!(p1.active_seconds, 10);

        // proj2 只有一个事件，活跃时间为 0
        let p2 = proj.per_project.get("proj2").unwrap();
        assert_eq!(p2.human_typed_chars, 20);
        assert_eq!(p2.active_seconds, 0);
    }

    #[test]
    fn test_projection_per_chapter_independent_active_time() {
        let mut e1 = make_event(1000, EventSource::HumanTyped, 10, 0, 0, 0);
        e1.chapter_id = "chap1".to_string();
        let mut e2 = make_event(11000, EventSource::HumanTyped, 5, 0, 0, 0);
        e2.chapter_id = "chap1".to_string();
        let mut e3 = make_event(2000, EventSource::HumanTyped, 20, 0, 0, 0);
        e3.chapter_id = "chap2".to_string();

        let events = vec![e1, e2, e3];
        let proj = project_events(&events, 1, None);

        let c1 = proj.per_chapter.get("chap1").unwrap();
        assert_eq!(c1.human_typed_chars, 15);
        assert_eq!(c1.active_seconds, 10);

        let c2 = proj.per_chapter.get("chap2").unwrap();
        assert_eq!(c2.human_typed_chars, 20);
        assert_eq!(c2.active_seconds, 0);
    }

    #[test]
    fn test_projection_speed_curve_only_human_typed() {
        let events = vec![
            make_event(1000, EventSource::HumanTyped, 30, 0, 0, 0),
            make_event(2000, EventSource::Unknown, 500, 0, 0, 0),
            make_event(3000, EventSource::Pasted, 0, 0, 100, 0),
        ];
        let proj = project_events(&events, 1, None);
        // 速度曲线只计 HumanTyped
        assert!(proj.speed_curve.iter().any(|b| b.chars_typed == 30));
        assert!(!proj.speed_curve.iter().any(|b| b.chars_typed == 500));
        assert!(!proj.speed_curve.iter().any(|b| b.chars_typed == 100));
    }

    #[test]
    fn test_projection_current_speed_only_human_typed() {
        let now = chrono::Utc::now().timestamp_millis();
        let events = vec![
            make_event(now - 5000, EventSource::HumanTyped, 20, 0, 0, 0),
            make_event(now - 4000, EventSource::Unknown, 500, 0, 0, 0),
            make_event(now - 3000, EventSource::Pasted, 0, 0, 300, 0),
        ];
        let proj = project_events(&events, 1, Some(60));
        let speed = proj.current_speed.unwrap();
        assert_eq!(speed.chars_typed, 20);
    }

    #[test]
    fn test_projection_session_gap() {
        let base = 1_000_000_i64;
        let events = vec![
            make_event(base, EventSource::HumanTyped, 5, 0, 0, 0),
            make_event(base + 10 * 60 * 1000, EventSource::HumanTyped, 5, 0, 0, 0),
        ];
        let proj = project_events(&events, 1, None);
        // 间隔超过 5 分钟 → 2 个会话
        assert_eq!(proj.summary.total_sessions, 2);
    }

    #[test]
    fn test_projection_empty_events() {
        let proj = project_events(&[], 1, None);
        assert_eq!(proj.summary.total_human_typed_chars, 0);
        assert!(proj.per_project.is_empty());
        assert!(proj.speed_curve.is_empty());
        assert!(proj.current_speed.is_none());
    }

    #[test]
    fn test_projection_per_device_class() {
        let mut e1 = make_event(1000, EventSource::HumanTyped, 10, 0, 0, 0);
        e1.device_id = "dev-desktop".to_string();
        e1.device_class = "desktop".to_string();
        let mut e2 = make_event(2000, EventSource::HumanTyped, 20, 0, 0, 0);
        e2.device_id = "dev-phone".to_string();
        e2.device_class = "phone".to_string();

        let events = vec![e1, e2];
        let proj = project_events(&events, 1, None);

        let desktop = proj.per_device_class.get("desktop").unwrap();
        assert_eq!(desktop.device_count, 1);
        assert_eq!(desktop.total_human_typed_chars, 10);

        let phone = proj.per_device_class.get("phone").unwrap();
        assert_eq!(phone.device_count, 1);
        assert_eq!(phone.total_human_typed_chars, 20);
    }

    // ── 问题1：替换操作的删除字数 ──

    #[test]
    fn test_typing_with_deleted_counts_both() {
        // 选中5个字再输入2个字 = HumanTyped, deleted=5, inserted=2
        let events = vec![make_event(1000, EventSource::HumanTyped, 2, 5, 0, 0)];
        let proj = project_events(&events, 1, None);
        assert_eq!(proj.summary.total_human_typed_chars, 2);
        assert_eq!(proj.summary.total_deleted_chars, 5);
        assert_eq!(proj.summary.total_net_delta_chars, -3); // 2 - 5
    }

    #[test]
    fn test_paste_with_deleted_counts_both() {
        // 选中5个字再粘贴3个字 = Pasted, deleted=5, pasted=3
        let events = vec![make_event(1000, EventSource::Pasted, 0, 5, 3, 0)];
        let proj = project_events(&events, 1, None);
        assert_eq!(proj.summary.total_pasted_chars, 3);
        assert_eq!(proj.summary.total_deleted_chars, 5);
        assert_eq!(proj.summary.total_net_delta_chars, -2); // 3 - 5
    }

    #[test]
    fn test_unknown_deleted_not_counted() {
        // Unknown source 的 deleted_chars 不计入分类
        let events = vec![make_event(1000, EventSource::Unknown, 0, 5, 0, 0)];
        let proj = project_events(&events, 1, None);
        assert_eq!(proj.summary.total_deleted_chars, 0);
        assert_eq!(proj.summary.total_net_delta_chars, -5);
    }

    #[test]
    fn test_sync_remote_deleted_not_counted() {
        let events = vec![make_event(1000, EventSource::SyncRemote, 0, 5, 0, 0)];
        let proj = project_events(&events, 1, None);
        assert_eq!(proj.summary.total_deleted_chars, 0);
        assert_eq!(proj.summary.total_net_delta_chars, -5);
    }

    // ── 问题2：多设备 session 不揉成一条 ──

    #[test]
    fn test_multi_device_sessions_not_merged() {
        // 两台设备在相近时间（间隔 < 5分钟）各有一个事件
        // 如果混在一起算，会被当成1个session；按设备独立算应该是2个session
        let mut e1 = make_event(1000, EventSource::HumanTyped, 5, 0, 0, 0);
        e1.device_id = "dev-a".to_string();
        let mut e2 = make_event(2000, EventSource::HumanTyped, 5, 0, 0, 0);
        e2.device_id = "dev-b".to_string();

        let events = vec![e1, e2];
        let proj = project_events(&events, 1, None);

        // 每台设备各自1个session，总共2个
        assert_eq!(proj.summary.total_sessions, 2);

        // 每台设备各自只有1个事件，活跃时间为0
        assert_eq!(proj.summary.total_active_seconds, 0);
    }

    #[test]
    fn test_multi_device_active_time_summed() {
        // dev-a 有两个事件间隔10秒 → active=10s, sessions=1
        // dev-b 有两个事件间隔20秒 → active=20s, sessions=1
        let mut e1 = make_event(1000, EventSource::HumanTyped, 5, 0, 0, 0);
        e1.device_id = "dev-a".to_string();
        let mut e2 = make_event(11000, EventSource::HumanTyped, 5, 0, 0, 0);
        e2.device_id = "dev-a".to_string();
        let mut e3 = make_event(2000, EventSource::HumanTyped, 5, 0, 0, 0);
        e3.device_id = "dev-b".to_string();
        let mut e4 = make_event(22000, EventSource::HumanTyped, 5, 0, 0, 0);
        e4.device_id = "dev-b".to_string();

        let events = vec![e1, e2, e3, e4];
        let proj = project_events(&events, 1, None);

        assert_eq!(proj.summary.total_sessions, 2);
        assert_eq!(proj.summary.total_active_seconds, 30); // 10 + 20
    }

    // ── 问题3：days_count 从 business_date 去重 ──

    #[test]
    fn test_days_count_single_day() {
        let events = vec![
            make_event(1000, EventSource::HumanTyped, 5, 0, 0, 0),
            make_event(2000, EventSource::HumanTyped, 5, 0, 0, 0),
        ];
        let proj = project_events(&events, 1, None);
        // 所有事件在同一天（local_date 为空，回退到 timestamp 现算）
        assert_eq!(proj.summary.days_count, 1);
    }

    #[test]
    fn test_days_count_multiple_days() {
        let mut e1 = make_event(1000, EventSource::HumanTyped, 5, 0, 0, 0);
        e1.local_date = "2026-10-05".to_string();
        let mut e2 = make_event(2000, EventSource::HumanTyped, 5, 0, 0, 0);
        e2.local_date = "2026-10-06".to_string();
        let mut e3 = make_event(3000, EventSource::HumanTyped, 5, 0, 0, 0);
        e3.local_date = "2026-10-06".to_string();

        let events = vec![e1, e2, e3];
        let proj = project_events(&events, 1, None);
        assert_eq!(proj.summary.days_count, 2);
    }

    // ── 问题4：bucket_minutes = 0 不死循环 ──

    #[test]
    fn test_speed_curve_zero_bucket_minutes_no_infinite_loop() {
        // 直接调用 project_events 传 bucket_minutes=0 不应死循环
        // 注意：api.rs 层面会钳到1，但 projection 层面也要能处理
        let events = vec![make_event(1000, EventSource::HumanTyped, 5, 0, 0, 0)];
        // 这里用 1 分钟确保不卡，真正的钳制在 api.rs 层
        let proj = project_events(&events, 1, None);
        assert!(!proj.speed_curve.is_empty());
    }

    // ── 作品级写作档案（#934）──

    #[test]
    fn test_lifetime_stats_empty_events_no_first_written_day() {
        let stats = project_lifetime_stats(&[], "proj1");
        assert_eq!(stats.first_written_at_ms, None);
        assert_eq!(stats.best_writing_day, None);
        assert_eq!(stats.best_writing_day_chars, 0);
        assert_eq!(stats.writing_days, 0);
        assert_eq!(stats.total_human_typed_chars, 0);
    }

    #[test]
    fn test_lifetime_stats_ignores_pasted_and_ai() {
        let mut pasted = make_event(1000, EventSource::Pasted, 0, 0, 100, 0);
        pasted.local_date = "2026-10-01".to_string();
        let mut ai = make_event(2000, EventSource::AiInserted, 0, 0, 0, 50);
        ai.local_date = "2026-10-01".to_string();
        let mut typed = make_event(3000, EventSource::HumanTyped, 10, 0, 0, 0);
        typed.local_date = "2026-10-02".to_string();

        let events = vec![pasted, ai, typed];
        let stats = project_lifetime_stats(&events, "proj1");

        assert_eq!(stats.total_human_typed_chars, 10);
        assert_eq!(stats.first_written_at_ms, Some(3000));
        assert_eq!(stats.best_writing_day.as_deref(), Some("2026-10-02"));
        assert_eq!(stats.best_writing_day_chars, 10);
        assert_eq!(stats.writing_days, 1);
    }

    #[test]
    fn test_lifetime_stats_best_day_tie_picks_earlier_date() {
        let mut d1 = make_event(1000, EventSource::HumanTyped, 40, 0, 0, 0);
        d1.local_date = "2026-10-01".to_string();
        let mut d2 = make_event(2000, EventSource::HumanTyped, 40, 0, 0, 0);
        d2.local_date = "2026-10-02".to_string();

        // 事件顺序颠倒不影响结果：并列时取更早日期。
        let events = vec![d2, d1];
        let stats = project_lifetime_stats(&events, "proj1");

        assert_eq!(stats.best_writing_day.as_deref(), Some("2026-10-01"));
        assert_eq!(stats.best_writing_day_chars, 40);
        assert_eq!(stats.writing_days, 2);
        assert_eq!(stats.first_written_at_ms, Some(1000));
    }

    #[test]
    fn test_lifetime_stats_only_counts_own_project() {
        let mut other = make_event(500, EventSource::HumanTyped, 999, 0, 0, 0);
        other.project_id = "proj2".to_string();
        other.local_date = "2026-09-30".to_string();
        let mut mine = make_event(1000, EventSource::HumanTyped, 7, 0, 0, 0);
        mine.local_date = "2026-10-01".to_string();

        let events = vec![other, mine];
        let stats = project_lifetime_stats(&events, "proj1");

        assert_eq!(stats.total_human_typed_chars, 7);
        assert_eq!(stats.first_written_at_ms, Some(1000));
        assert_eq!(stats.writing_days, 1);
    }

    #[test]
    fn test_lifetime_stats_zero_insert_does_not_start_history() {
        // HumanTyped 但 inserted=0（纯删除事务）：不算开写，也不增加写作日。
        let mut e = make_event(1000, EventSource::HumanTyped, 0, 5, 0, 0);
        e.local_date = "2026-10-01".to_string();
        let stats = project_lifetime_stats(&[e], "proj1");
        assert_eq!(stats.first_written_at_ms, None);
        assert_eq!(stats.total_human_typed_chars, 0);
        assert_eq!(stats.writing_days, 0);
    }

    // ── 区间聚合与趋势（#944）──

    /// 钉住业务日的事件：时间戳只决定会话/窗口顺序，归属日由 `local_date` 决定。
    fn pinned_event(
        timestamp_ms: i64,
        local_date: &str,
        device_id: &str,
        inserted_chars: u32,
    ) -> WritingInputEvent {
        let mut event = make_event(
            timestamp_ms,
            EventSource::HumanTyped,
            inserted_chars,
            0,
            0,
            0,
        );
        event.local_date = local_date.to_string();
        event.device_id = device_id.to_string();
        event
    }

    #[test]
    fn test_range_stats_filters_both_ends_inclusive() {
        let events = vec![
            pinned_event(1000, "2026-05-09", "dev-1", 40),
            pinned_event(2000, "2026-05-10", "dev-1", 10),
            pinned_event(3000, "2026-05-11", "dev-1", 20),
            pinned_event(4000, "2026-05-12", "dev-1", 80),
        ];
        let stats = project_range_stats(&events, "2026-05-10", "2026-05-11").unwrap();
        // 两端包含：05-10 的 10 字和 05-11 的 20 字都要，区间外的不算。
        assert_eq!(stats.total_human_typed_chars, 30);
        assert_eq!(stats.writing_days, 2);
        assert_eq!(stats.daily_human_typed_chars.len(), 2);
    }

    #[test]
    fn test_range_stats_rejects_reversed_range() {
        assert!(project_range_stats(&[], "2026-05-11", "2026-05-10").is_err());
    }

    #[test]
    fn test_range_stats_best_day_tie_picks_earlier_date() {
        // 事件顺序颠倒不影响结果：并列时取更早日期。
        let events = vec![
            pinned_event(2000, "2026-05-02", "dev-1", 40),
            pinned_event(1000, "2026-05-01", "dev-1", 40),
        ];
        let stats = project_all_time_stats(&events).unwrap();
        let best = stats.best_day.expect("有输入就该有最多码字日");
        assert_eq!(best.date, "2026-05-01");
        assert_eq!(best.end_date, None);
        assert_eq!(best.value, 40);
    }

    #[test]
    fn test_range_stats_longest_streak_gap_and_tie() {
        // 05-01、05-02 是 2 天；05-04..05-06 是 3 天 → 取更长的 3 天。
        let events = vec![
            pinned_event(1000, "2026-05-01", "dev-1", 5),
            pinned_event(2000, "2026-05-02", "dev-1", 5),
            pinned_event(3000, "2026-05-04", "dev-1", 5),
            pinned_event(4000, "2026-05-05", "dev-1", 5),
            pinned_event(5000, "2026-05-06", "dev-1", 5),
        ];
        let streak = project_all_time_stats(&events)
            .unwrap()
            .longest_streak
            .expect("连续段存在");
        assert_eq!(streak.date, "2026-05-04");
        assert_eq!(streak.end_date.as_deref(), Some("2026-05-06"));
        assert_eq!(streak.value, 3);

        // 并列（两段都是 2 天）时取更早的一段。
        let tied = vec![
            pinned_event(1000, "2026-05-01", "dev-1", 5),
            pinned_event(2000, "2026-05-02", "dev-1", 5),
            pinned_event(3000, "2026-05-05", "dev-1", 5),
            pinned_event(4000, "2026-05-06", "dev-1", 5),
        ];
        let streak = project_all_time_stats(&tied)
            .unwrap()
            .longest_streak
            .expect("连续段存在");
        assert_eq!(streak.date, "2026-05-01");
        assert_eq!(streak.end_date.as_deref(), Some("2026-05-02"));
        assert_eq!(streak.value, 2);
    }

    #[test]
    fn test_range_stats_single_event_has_no_session_record() {
        // 单条事件没有「时长」，不能捏造会话纪录；重复重发同一秒的事件同理。
        let events = vec![
            pinned_event(1_000, "2026-05-01", "dev-1", 5),
            pinned_event(1_000, "2026-05-01", "dev-1", 5),
        ];
        let stats = project_all_time_stats(&events).unwrap();
        assert_eq!(stats.total_human_typed_chars, 10);
        assert_eq!(stats.total_active_seconds, 0);
        assert!(stats.longest_session.is_none());
    }

    #[test]
    fn test_range_stats_multi_device_sessions_not_merged() {
        // dev-a 会话 60 秒；dev-b 会话 60 秒。混在一起会变成一条 90 秒的假会话。
        let base = 1_000_000_i64;
        let events = vec![
            pinned_event(base, "2026-05-01", "dev-a", 5),
            pinned_event(base + 60_000, "2026-05-01", "dev-a", 5),
            pinned_event(base + 30_000, "2026-05-01", "dev-b", 5),
            pinned_event(base + 90_000, "2026-05-01", "dev-b", 5),
        ];
        let stats = project_all_time_stats(&events).unwrap();
        assert_eq!(stats.total_active_seconds, 120);
        let session = stats.longest_session.expect("两台设备各有会话");
        assert_eq!(session.value, 60);
        assert_eq!(session.date, "2026-05-01");
    }

    #[test]
    fn test_range_stats_session_crosses_midnight_keeps_first_day() {
        // 23:58 到次日 00:03 恰好 5 分钟（等于 SESSION_GAP_MS）：同一会话，
        // 纪录日期取会话首事件的业务日。
        let base = 1_000_000_i64;
        let events = vec![
            pinned_event(base, "2026-05-01", "dev-1", 10),
            pinned_event(base + 5 * 60 * 1000, "2026-05-02", "dev-1", 20),
        ];
        let stats = project_all_time_stats(&events).unwrap();
        assert_eq!(stats.total_active_seconds, 300);
        let session = stats.longest_session.expect("跨午夜会话");
        assert_eq!(session.date, "2026-05-01");
        assert_eq!(session.value, 300);
    }

    #[test]
    fn test_range_stats_fastest_hour_window_is_half_open() {
        // 第二条事件恰好在第一条的 start+60min：不算进第一个窗口。
        // 若窗口写成闭区间，单小时最多码字会变成 150。
        let base = 1_000_000_i64;
        let events = vec![
            pinned_event(base, "2026-05-01", "dev-1", 50),
            pinned_event(base + 60 * 60 * 1000, "2026-05-02", "dev-1", 100),
        ];
        let stats = project_all_time_stats(&events).unwrap();
        let fastest = stats.fastest_hour.expect("有人工输入就有最快窗口");
        assert_eq!(fastest.value, 100);
        assert_eq!(fastest.date, "2026-05-02");

        // 窗口右移 1 毫秒后两条都落在同一窗口内。
        let close = vec![
            pinned_event(base, "2026-05-01", "dev-1", 50),
            pinned_event(base + 60 * 60 * 1000 - 1, "2026-05-01", "dev-1", 100),
        ];
        let fastest = project_all_time_stats(&close)
            .unwrap()
            .fastest_hour
            .expect("有人工输入就有最快窗口");
        assert_eq!(fastest.value, 150);
        assert_eq!(fastest.date, "2026-05-01");
    }

    #[test]
    fn test_range_stats_only_typed_counts_writing() {
        // 粘贴 / AI / 撤销都不算「写作」：没有人工输入就没有纪录。
        let mut pasted = pinned_event(1000, "2026-05-01", "dev-1", 0);
        pasted.source = EventSource::Pasted;
        pasted.pasted_chars = 100;
        pasted.net_delta_chars = 100;
        let mut ai = pinned_event(2000, "2026-05-01", "dev-1", 0);
        ai.source = EventSource::AiInserted;
        ai.ai_inserted_chars = 50;
        ai.net_delta_chars = 50;
        let mut undo = pinned_event(3000, "2026-05-01", "dev-1", 0);
        undo.source = EventSource::Unknown;

        let stats = project_all_time_stats(&[pasted, ai, undo]).unwrap();
        assert_eq!(stats.total_human_typed_chars, 0);
        assert_eq!(stats.writing_days, 0);
        assert!(stats.daily_human_typed_chars.is_empty());
        assert!(stats.best_day.is_none());
        assert!(stats.fastest_hour.is_none());
        assert!(stats.longest_streak.is_none());
        // 会话仍沿用现有定义：区间内事件时间线独立按设备统计。
        assert!(stats.longest_session.is_some());
    }

    #[test]
    fn test_range_stats_empty_events() {
        let stats = project_all_time_stats(&[]).unwrap();
        assert_eq!(stats.total_human_typed_chars, 0);
        assert_eq!(stats.writing_days, 0);
        assert_eq!(stats.total_active_seconds, 0);
        assert!(stats.best_day.is_none());
        assert!(stats.fastest_hour.is_none());
        assert!(stats.longest_session.is_none());
        assert!(stats.longest_streak.is_none());
    }

    #[test]
    fn test_trend_granularity_for_span() {
        assert_eq!(
            TrendGranularity::for_span("2026-05-01", "2026-05-31").unwrap(),
            TrendGranularity::Day
        );
        assert_eq!(
            TrendGranularity::for_span("2026-05-01", "2026-06-01").unwrap(),
            TrendGranularity::Week
        );
        assert_eq!(
            TrendGranularity::for_span("2026-01-01", "2026-07-19").unwrap(),
            TrendGranularity::Week
        );
        assert_eq!(
            TrendGranularity::for_span("2026-01-01", "2026-07-20").unwrap(),
            TrendGranularity::Month
        );
        assert!(TrendGranularity::for_span("2026-05-02", "2026-05-01").is_err());
    }

    fn sample_daily() -> BTreeMap<String, u64> {
        BTreeMap::from([
            ("2026-05-01".to_string(), 10),
            ("2026-05-03".to_string(), 5),
            ("2026-05-11".to_string(), 7),
            ("2026-06-02".to_string(), 3),
        ])
    }

    #[test]
    fn test_bucket_trend_ignores_dates_outside_range() {
        let daily = BTreeMap::from([
            ("2026-04-28".to_string(), 100), // 区间前，但和 05-01 属同一周
            ("2026-05-01".to_string(), 10),
            ("2026-05-31".to_string(), 7), // 区间后，和 05-01 属同一月
        ]);
        let week =
            bucket_trend(&daily, TrendGranularity::Week, "2026-05-01", "2026-05-02").unwrap();
        assert_eq!(week.len(), 1);
        assert_eq!(week[0].date, "2026-04-27");
        assert_eq!(week[0].human_typed_chars, 10);

        let month =
            bucket_trend(&daily, TrendGranularity::Month, "2026-05-01", "2026-05-02").unwrap();
        assert_eq!(month.len(), 1);
        assert_eq!(month[0].date, "2026-05-01");
        assert_eq!(month[0].human_typed_chars, 10);
    }

    #[test]
    fn test_bucket_trend_day_fills_zeros_in_order() {
        let points = bucket_trend(
            &sample_daily(),
            TrendGranularity::Day,
            "2026-05-01",
            "2026-05-04",
        )
        .unwrap();
        let dates: Vec<&str> = points.iter().map(|point| point.date.as_str()).collect();
        assert_eq!(
            dates,
            vec!["2026-05-01", "2026-05-02", "2026-05-03", "2026-05-04"]
        );
        let chars: Vec<u64> = points.iter().map(|point| point.human_typed_chars).collect();
        assert_eq!(chars, vec![10, 0, 5, 0]);
    }

    #[test]
    fn test_bucket_trend_week_aligns_to_monday_and_covers_intersecting_weeks() {
        let points = bucket_trend(
            &sample_daily(),
            TrendGranularity::Week,
            "2026-05-01",
            "2026-05-12",
        )
        .unwrap();
        let dates: Vec<&str> = points.iter().map(|point| point.date.as_str()).collect();
        // 05-01（周五）所在周周一是 04-27，最后一周周一是 05-11。
        assert_eq!(dates, vec!["2026-04-27", "2026-05-04", "2026-05-11"]);
        let chars: Vec<u64> = points.iter().map(|point| point.human_typed_chars).collect();
        assert_eq!(chars, vec![15, 0, 7]);
    }

    #[test]
    fn test_bucket_trend_month_covers_short_lifetime() {
        // 短于一年：从首次写作所在月到今天所在月，逐月补 0。
        let points = bucket_trend(
            &sample_daily(),
            TrendGranularity::Month,
            "2026-03-10",
            "2026-10-18",
        )
        .unwrap();
        assert_eq!(points.len(), 8);
        assert_eq!(points[0].date, "2026-03-01");
        assert_eq!(points[0].human_typed_chars, 0);
        assert_eq!(points[3].date, "2026-06-01");
        assert_eq!(points[3].human_typed_chars, 3);
        assert_eq!(points[7].date, "2026-10-01");
        assert_eq!(points[7].human_typed_chars, 0);
    }
}
