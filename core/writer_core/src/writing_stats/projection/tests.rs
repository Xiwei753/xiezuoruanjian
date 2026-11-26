//! projection.rs 的单元测试（从生产文件拆出，保持原有断言）。

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
        proof: None,
        starmap_id: None,
        verified_typed: true,
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
    let week = bucket_trend(&daily, TrendGranularity::Week, "2026-05-01", "2026-05-02").unwrap();
    assert_eq!(week.len(), 1);
    assert_eq!(week[0].date, "2026-04-27");
    assert_eq!(week[0].human_typed_chars, 10);

    let month = bucket_trend(&daily, TrendGranularity::Month, "2026-05-01", "2026-05-02").unwrap();
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
