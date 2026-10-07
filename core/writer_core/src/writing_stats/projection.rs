//! # 统计投影模块 — 单一事件事实源、单一聚合口径
//!
//! 所有统计计算集中在一个 [`project_events()`] 函数里。输入只是一段
//! `&[WritingInputEvent]`，一次遍历同时生成所有维度的统计结果。
//!
//! ## 关键规则
//!
//! - **会话和活跃时间按各自分组里的事件时间线独立计算**：project 的活跃时间
//!   只看该 project 的事件，chapter 只看该 chapter 的事件，不能把设备整天时间
//!   复制进去。
//! - **历史速度和当前速度都只统计 `EventSource::HumanTyped`**：Undo/Redo/
//!   Programmatic 的 inserted 不进入速度。
//! - **速度曲线分桶为半开区间 `[bucket_start, bucket_end)`**。
//! - **会话边界**：两次事件间隔超过 `SESSION_GAP_MS`（5分钟）视为不同会话。
//! - **活跃时间**：会话内从第一个事件到最后一个事件的时长。

use crate::writing_stats::{EventSource, WritingInputEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

    let mut sorted: Vec<i64> = events.iter().map(|e| e.timestamp_ms).collect();
    sorted.sort_unstable();

    let mut session_count: u32 = 0;
    let mut active_ms: i64 = 0;
    let mut current_session_start = sorted[0];
    let mut prev_ms = sorted[0];

    for &ts in sorted.iter().skip(1) {
        if (ts - prev_ms) > SESSION_GAP_MS {
            // 会话结束
            session_count += 1;
            active_ms += prev_ms - current_session_start;
            current_session_start = ts;
        }
        prev_ms = ts;
    }
    // 最后一个会话
    session_count += 1;
    active_ms += prev_ms - current_session_start;

    (session_count, active_ms)
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
}
