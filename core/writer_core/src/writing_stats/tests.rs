use crate::writing_stats::api::StatsApi;
use crate::writing_stats::projection::{project_events, CurrentWritingSpeed};
use crate::writing_stats::store::StatsStore;
use crate::writing_stats::{DateRange, EventSource, Platform, WritingInputEvent};
use tempfile::tempdir;

#[test]
fn test_human_typed_counts_as_pure_input() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event = WritingInputEvent::new(
        "dev-1",
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
        "s1",
    );
    api.record_event(event).unwrap();

    let today = StatsApi::today_date();
    let summary = api
        .get_stats_summary(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();

    assert_eq!(summary["totalHumanTypedChars"], 10);
    assert_eq!(summary["totalPastedChars"], 0);
    assert_eq!(summary["totalDeletedChars"], 0);
    assert_eq!(summary["totalAiInsertedChars"], 0);
}

#[test]
fn test_pasted_does_not_count_as_human_typed() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event = WritingInputEvent::new(
        "dev-1",
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
        "s1",
    );
    api.record_event(event).unwrap();

    let today = StatsApi::today_date();
    let summary = api
        .get_stats_summary(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();

    assert_eq!(summary["totalHumanTypedChars"], 0);
    assert_eq!(summary["totalPastedChars"], 20);
}

#[test]
fn test_deleted_does_not_cancel_human_typed() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event1 = WritingInputEvent::new(
        "dev-1",
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
        "s1",
    );
    api.record_event(event1).unwrap();

    let event2 = WritingInputEvent::new(
        "dev-1",
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
        "s1",
    );
    api.record_event(event2).unwrap();

    let today = StatsApi::today_date();
    let summary = api
        .get_stats_summary(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();

    assert_eq!(summary["totalHumanTypedChars"], 10);
    assert_eq!(summary["totalDeletedChars"], 3);
    assert_eq!(summary["totalNetDeltaChars"], 7);
}

#[test]
fn test_ai_inserted_not_counted_as_human() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event = WritingInputEvent::new(
        "dev-1",
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
        "s1",
    );
    api.record_event(event).unwrap();

    let today = StatsApi::today_date();
    let summary = api
        .get_stats_summary(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();

    assert_eq!(summary["totalHumanTypedChars"], 0);
    assert_eq!(summary["totalAiInsertedChars"], 50);
}

#[test]
fn test_sync_remote_not_counted_as_local_input() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event = WritingInputEvent::new(
        "dev-1",
        Platform::Desktop,
        "desktop",
        "proj1",
        "vol1",
        "chap1",
        EventSource::SyncRemote,
        0,
        0,
        0,
        0,
        0,
        "s1",
    );
    api.record_event(event).unwrap();

    let today = StatsApi::today_date();
    let summary = api
        .get_stats_summary(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();

    assert_eq!(summary["totalHumanTypedChars"], 0);
    assert_eq!(summary["totalNetDeltaChars"], 0);
}

#[test]
fn test_empty_events_projection() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let today_str = StatsApi::today_date();
    let range = DateRange {
        start_date: today_str.clone(),
        end_date: today_str,
    };

    let summary = api.get_stats_summary(&range).unwrap();
    assert_eq!(summary["totalHumanTypedChars"], 0);
    assert_eq!(summary["totalDeletedChars"], 0);
    assert_eq!(summary["totalNetDeltaChars"], 0);
    assert_eq!(summary["totalActiveSeconds"], 0);
}

#[test]
fn test_multi_device_no_overlap() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event1 = WritingInputEvent::new(
        "dev-linux",
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
        "s1",
    );
    api.record_event(event1).unwrap();

    let event2 = WritingInputEvent::new(
        "dev-android",
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
        "s2",
    );
    api.record_event(event2).unwrap();

    let today = StatsApi::today_date();
    let device_stats = api
        .get_stats_by_device(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();
    let devices = device_stats["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 2);

    let linux_dev = devices
        .iter()
        .find(|d| d["deviceId"] == "dev-linux")
        .unwrap();
    assert_eq!(linux_dev["humanTypedChars"], 10);

    let android_dev = devices
        .iter()
        .find(|d| d["deviceId"] == "dev-android")
        .unwrap();
    assert_eq!(android_dev["humanTypedChars"], 20);
}

#[test]
fn test_speed_buckets_generation() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let now_ms = chrono::Utc::now().timestamp_millis();

    for i in 0..5 {
        let event = WritingInputEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            timestamp_ms: now_ms + i * 1000,
            device_id: "dev-1".to_string(),
            platform: Platform::Desktop,
            device_class: "desktop".to_string(),
            project_id: "proj1".to_string(),
            volume_id: "vol1".to_string(),
            chapter_id: "chap1".to_string(),
            source: EventSource::HumanTyped,
            inserted_chars: 5,
            deleted_chars: 0,
            pasted_chars: 0,
            ai_inserted_chars: 0,
            net_delta_chars: 5,
            duration_seconds: 0,
            session_id: "s1".to_string(),
            local_date: String::new(),
        };
        api.record_event(event).unwrap();
    }

    let today = StatsApi::today_date();
    let speed_curve = api
        .get_speed_curve(
            &DateRange {
                start_date: today.clone(),
                end_date: today,
            },
            1,
        )
        .unwrap();
    let buckets = speed_curve["buckets"].as_array().unwrap();
    assert!(!buckets.is_empty());
    assert!(buckets
        .iter()
        .any(|b| b["charsTyped"].as_u64().unwrap() > 0));
}

fn speed_test_event(timestamp_ms: i64, inserted_chars: u32) -> WritingInputEvent {
    speed_test_event_with_source(timestamp_ms, inserted_chars, EventSource::HumanTyped)
}

fn speed_test_event_with_source(
    timestamp_ms: i64,
    inserted_chars: u32,
    source: EventSource,
) -> WritingInputEvent {
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
        inserted_chars,
        deleted_chars: 0,
        pasted_chars: 0,
        ai_inserted_chars: 0,
        net_delta_chars: i32::try_from(inserted_chars).unwrap_or(i32::MAX),
        duration_seconds: 0,
        session_id: "s1".to_string(),
        local_date: String::new(),
    }
}

// 当前速度只能计 HumanTyped：Undo/Redo/Programmatic/Load/Format 会带着真实
// inserted delta 落盘、但 source 映射成 Unknown，一次撤销恢复一大段文字不该把
// 「字/分」冲高。口径必须和中段 `totalHumanTypedChars` 一致。
#[test]
fn test_current_speed_counts_only_human_typed() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let now_ms = chrono::Utc::now().timestamp_millis();
    // 人工输入 20 字
    api.record_event(speed_test_event_with_source(
        now_ms - 5_000,
        20,
        EventSource::HumanTyped,
    ))
    .unwrap();
    // 撤销恢复 500 字：带真实 inserted delta，但 source 是 Unknown
    api.record_event(speed_test_event_with_source(
        now_ms - 4_000,
        500,
        EventSource::Unknown,
    ))
    .unwrap();
    // 粘贴 300 字
    api.record_event(speed_test_event_with_source(
        now_ms - 3_000,
        300,
        EventSource::Pasted,
    ))
    .unwrap();

    // 只计 HumanTyped 的 20 字，不是 20 + 500 + 300。
    let speed = api.get_current_speed(60).unwrap();
    assert_eq!(speed.chars_typed, 20);
    assert!((speed.chars_per_minute - 20.0).abs() < 0.001);
}

// 停笔后实时速度必须回落到 0：速度曲线最后一桶会一直挂着非零值，
// 这正是把「当前速度」收回 Core 重算的原因。
#[test]
fn test_current_speed_falls_to_zero_after_stopping() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    // 5 分钟前的一笔输入，早就落盘，不在最近 60 秒窗口内。
    let old_ms = chrono::Utc::now().timestamp_millis() - 5 * 60 * 1000;
    api.record_event(speed_test_event(old_ms, 500)).unwrap();

    let speed = api.get_current_speed(60).unwrap();
    assert_eq!(speed.chars_typed, 0);
    assert_eq!(speed.chars_per_minute, 0.0);

    // 历史曲线里那 500 字还在——曲线和实时速度职责不同，不互相污染。
    let today = StatsApi::today_date();
    let curve = api
        .get_speed_curve(
            &DateRange {
                start_date: today.clone(),
                end_date: today,
            },
            1,
        )
        .unwrap();
    let buckets = curve["buckets"].as_array().unwrap();
    assert!(buckets
        .iter()
        .any(|b| b["charsTyped"].as_u64().unwrap() == 500));
}

// 0 秒窗口没有意义且无法折算速度，Core 钳到 1 秒而不是让除法炸掉。
#[test]
fn test_current_speed_clamps_zero_window() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let speed = api.get_current_speed(0).unwrap();
    assert_eq!(speed.window_seconds, 1);
    assert_eq!(speed.chars_typed, 0);
    assert!(speed.chars_per_minute.is_finite());
}

// 窗口外的输入不计入当前速度，但落在窗口边界内的一定计入。
#[test]
fn test_current_speed_window_boundaries() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let now_ms = chrono::Utc::now().timestamp_millis();
    // 窗口内：30 秒前
    api.record_event(speed_test_event(now_ms - 30_000, 7))
        .unwrap();
    // 窗口外：2 分钟前
    api.record_event(speed_test_event(now_ms - 120_000, 999))
        .unwrap();

    assert_eq!(api.get_current_speed(60).unwrap().chars_typed, 7);
    assert_eq!(api.get_current_speed(180).unwrap().chars_typed, 7 + 999);
}

#[test]
fn test_per_project_tracking() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event = WritingInputEvent::new(
        "dev-1",
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
        "s1",
    );
    api.record_event(event).unwrap();

    let today = StatsApi::today_date();
    let project_stats = api
        .get_stats_by_project(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();
    let projects = project_stats["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["projectId"], "proj-abc");
    assert_eq!(projects[0]["humanTypedChars"], 15);
}

#[test]
fn test_per_chapter_tracking() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event = WritingInputEvent::new(
        "dev-1",
        Platform::Desktop,
        "desktop",
        "proj1",
        "vol1",
        "chap-xyz",
        EventSource::HumanTyped,
        25,
        0,
        0,
        0,
        0,
        "s1",
    );
    api.record_event(event).unwrap();

    let today = StatsApi::today_date();
    let chapter_stats = api
        .get_stats_by_chapter(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();
    let chapters = chapter_stats["chapters"].as_array().unwrap();
    assert_eq!(chapters.len(), 1);
    assert_eq!(chapters[0]["chapterId"], "chap-xyz");
    assert_eq!(chapters[0]["humanTypedChars"], 25);
}

#[test]
fn test_event_file_written() {
    let temp_dir = tempdir().unwrap();
    let store = StatsStore::new(temp_dir.path());

    let event = WritingInputEvent::new(
        "dev-1",
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
        "s1",
    );

    store.record_event(event.clone()).unwrap();

    // 直接落盘，不需要 flush
    let date = store.timestamp_to_date(event.timestamp_ms).unwrap();
    let events = store.load_events_for_date(&date).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].inserted_chars, 10);
}

#[test]
fn test_session_gap_detection() {
    let temp_dir = tempdir().unwrap();
    let store = StatsStore::new(temp_dir.path());

    let base_ms = chrono::DateTime::parse_from_rfc3339("2026-06-08T12:00:00Z")
        .unwrap()
        .timestamp_millis();

    let event1 = WritingInputEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        timestamp_ms: base_ms,
        device_id: "dev-1".to_string(),
        platform: Platform::Desktop,
        device_class: "desktop".to_string(),
        project_id: "proj1".to_string(),
        volume_id: "vol1".to_string(),
        chapter_id: "chap1".to_string(),
        source: EventSource::HumanTyped,
        inserted_chars: 5,
        deleted_chars: 0,
        pasted_chars: 0,
        ai_inserted_chars: 0,
        net_delta_chars: 5,
        duration_seconds: 0,
        session_id: "s1".to_string(),
        local_date: String::new(),
    };
    store.record_event(event1).unwrap();

    let event2 = WritingInputEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        timestamp_ms: base_ms + 10 * 60 * 1000,
        device_id: "dev-1".to_string(),
        platform: Platform::Desktop,
        device_class: "desktop".to_string(),
        project_id: "proj1".to_string(),
        volume_id: "vol1".to_string(),
        chapter_id: "chap1".to_string(),
        source: EventSource::HumanTyped,
        inserted_chars: 5,
        deleted_chars: 0,
        pasted_chars: 0,
        ai_inserted_chars: 0,
        net_delta_chars: 5,
        duration_seconds: 0,
        session_id: "s1".to_string(),
        local_date: String::new(),
    };
    store.record_event(event2).unwrap();

    let date = crate::writing_stats::calendar::local_date_at(base_ms).unwrap();
    let events = store.load_events_for_date(&date).unwrap();
    let proj = project_events(&events, 1, None);

    assert_eq!(proj.summary.total_sessions, 2);
}

#[test]
fn test_char_count_uses_unicode_scalar() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let chinese_text = "你好世界";
    let char_count = chinese_text.chars().count();

    let event = WritingInputEvent::new(
        "dev-1",
        Platform::Desktop,
        "desktop",
        "proj1",
        "vol1",
        "chap1",
        EventSource::HumanTyped,
        u32::try_from(char_count).unwrap(),
        0,
        0,
        0,
        0,
        "s1",
    );
    api.record_event(event).unwrap();

    let today = StatsApi::today_date();
    let summary = api
        .get_stats_summary(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();

    assert_eq!(summary["totalHumanTypedChars"], 4);
}

#[test]
fn test_facade_record_editor_change_stats() {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let core = crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));

    core.record_editor_change_stats(
        "linux",
        "proj1",
        "vol1",
        "chap1",
        crate::editor::EditorTransactionCause::Typing,
        10,
        0,
    )
    .unwrap();

    core.record_editor_change_stats(
        "linux",
        "proj1",
        "vol1",
        "chap1",
        crate::editor::EditorTransactionCause::Paste,
        20,
        0,
    )
    .unwrap();

    core.record_editor_change_stats(
        "linux",
        "proj1",
        "vol1",
        "chap1",
        crate::editor::EditorTransactionCause::Delete,
        0,
        5,
    )
    .unwrap();

    core.record_editor_change_stats(
        "android",
        "proj1",
        "vol1",
        "chap1",
        crate::editor::EditorTransactionCause::Programmatic,
        0,
        0,
    )
    .unwrap();

    let today = StatsApi::today_date();
    let summary = core.get_writing_stats_summary(&today, &today).unwrap();
    assert_eq!(summary["totalHumanTypedChars"], 10);
    assert_eq!(summary["totalPastedChars"], 20);
    assert_eq!(summary["totalDeletedChars"], 5);
    assert_eq!(summary["totalNetDeltaChars"], 25); // 10 + 20 - 5
}

/// Undo/Redo/Programmatic 不得靠默认分支落入人工输入。
#[test]
fn test_facade_record_editor_change_stats_non_typed_sources_never_human_typed() {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let core = crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));

    // Typing → HumanTyped
    core.record_editor_change_stats(
        "linux",
        "proj1",
        "vol1",
        "chap1",
        crate::editor::EditorTransactionCause::Typing,
        10,
        0,
    )
    .unwrap();
    // Paste → Pasted
    core.record_editor_change_stats(
        "linux",
        "proj1",
        "vol1",
        "chap1",
        crate::editor::EditorTransactionCause::Paste,
        7,
        0,
    )
    .unwrap();
    // Delete → Deleted
    core.record_editor_change_stats(
        "linux",
        "proj1",
        "vol1",
        "chap1",
        crate::editor::EditorTransactionCause::Delete,
        0,
        3,
    )
    .unwrap();
    // Undo → Unknown
    core.record_editor_change_stats(
        "linux",
        "proj1",
        "vol1",
        "chap1",
        crate::editor::EditorTransactionCause::Undo,
        0,
        2,
    )
    .unwrap();
    // Redo → Unknown
    core.record_editor_change_stats(
        "linux",
        "proj1",
        "vol1",
        "chap1",
        crate::editor::EditorTransactionCause::Redo,
        4,
        0,
    )
    .unwrap();
    // Programmatic → Unknown
    core.record_editor_change_stats(
        "linux",
        "proj1",
        "vol1",
        "chap1",
        crate::editor::EditorTransactionCause::Programmatic,
        5,
        1,
    )
    .unwrap();

    let today = StatsApi::today_date();
    let summary = core.get_writing_stats_summary(&today, &today).unwrap();
    // typing 是唯一落入人工输入的来源；undo/redo/programmatic 一律不是。
    assert_eq!(summary["totalHumanTypedChars"], 10);
    assert_eq!(summary["totalPastedChars"], 7);
    assert_eq!(summary["totalDeletedChars"], 3);
    // net_delta = 10(typing) + 7(pasted) - 3(deleted) - 2(undo) + 4(redo) + 5-1(programmatic)
    assert_eq!(summary["totalNetDeltaChars"], 20);
}

#[test]
fn test_sync_stats_paths_outside_repo_not_blacklisted() {
    assert!(!crate::sync::SyncService::is_blacklisted_path(
        "app-meta/stats/events.local/2025-01-15.events.jsonl",
        crate::sync::SyncScope::Project
    ));
    assert!(crate::sync::SyncService::is_blacklisted_path(
        "app-meta/stats/cache/something.json",
        crate::sync::SyncScope::Project
    ));
}

#[test]
fn test_load_chapter_does_not_produce_input_events() {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let core = crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));

    let project = core.create_project("Test").unwrap();
    let volume = core.create_volume(&project.id, "Vol").unwrap();
    let chapter = core.create_chapter(&project.id, &volume.id, "Ch1").unwrap();
    core.write_chapter(&project.id, &volume.id, &chapter.id, "Hello world")
        .unwrap();

    let today = StatsApi::today_date();
    let summary_before = core.get_writing_stats_summary(&today, &today).unwrap();

    let _content = core
        .read_chapter(&project.id, &volume.id, &chapter.id)
        .unwrap();

    let summary_after = core.get_writing_stats_summary(&today, &today).unwrap();
    assert_eq!(summary_before, summary_after);
}

// ---------------------------------------------------------------------------
// 业务日历日（本地午夜）口径
// ---------------------------------------------------------------------------

fn event_with_pinned_dates(
    timestamp_ms: i64,
    local_date: &str,
    source: EventSource,
    chars: u32,
) -> WritingInputEvent {
    let mut event = WritingInputEvent::new(
        "dev-1",
        Platform::Desktop,
        "desktop",
        "proj1",
        "vol1",
        "chap1",
        source,
        chars,
        0,
        0,
        0,
        0,
        "s1",
    );
    event.timestamp_ms = timestamp_ms;
    event.local_date = local_date.to_string();
    event
}

const TS_UTC8_LOCAL_0030: i64 = 1_791_217_800_000; // UTC 10-05 16:30 = UTC+8 本地 10-06 00:30
const TS_UTC8_LOCAL_2330: i64 = 1_791_271_800_000; // UTC 10-06 07:30 = UTC-8 本地 10-05 23:30

#[test]
fn test_projection_uses_event_local_date_not_utc() {
    let events = vec![event_with_pinned_dates(
        TS_UTC8_LOCAL_0030,
        "2026-10-06",
        EventSource::HumanTyped,
        30,
    )];
    let proj = project_events(&events, 1, None);
    assert_eq!(proj.summary.total_human_typed_chars, 30);
}

#[test]
fn test_projection_west_of_utc_local_date_wins() {
    let events = vec![event_with_pinned_dates(
        TS_UTC8_LOCAL_2330,
        "2026-10-05",
        EventSource::HumanTyped,
        17,
    )];
    let proj = project_events(&events, 1, None);
    assert_eq!(proj.summary.total_human_typed_chars, 17);
}

#[test]
fn test_new_event_stamps_local_date_not_utc() {
    let event = WritingInputEvent::new(
        "dev-1",
        Platform::Desktop,
        "desktop",
        "proj1",
        "vol1",
        "chap1",
        EventSource::HumanTyped,
        1,
        0,
        0,
        0,
        0,
        "s1",
    );
    assert_eq!(
        event.local_date,
        crate::writing_stats::calendar::local_date_at(event.timestamp_ms).unwrap()
    );
    assert_eq!(event.business_date(), event.local_date);
}

#[test]
fn test_old_event_without_local_date_falls_back_to_timestamp() {
    let json = r#"{
        "event_id": "evt-1",
        "timestamp_ms": 1791217800000,
        "device_id": "dev-1",
        "platform": "desktop",
        "project_id": "p1",
        "volume_id": "v1",
        "chapter_id": "c1",
        "source": "human_typed",
        "inserted_chars": 5,
        "deleted_chars": 0,
        "pasted_chars": 0,
        "ai_inserted_chars": 0,
        "net_delta_chars": 5,
        "duration_seconds": 0,
        "session_id": "s1"
    }"#;
    let event: WritingInputEvent = serde_json::from_str(json).unwrap();
    assert_eq!(event.local_date, "", "老事件反序列化后应为空串");
    assert_eq!(
        event.business_date(),
        crate::writing_stats::calendar::local_date_at(event.timestamp_ms).unwrap(),
        "老事件应回退到按 timestamp 现算本机本地日"
    );
}

#[test]
fn test_today_summary_matches_local_calendar_day() {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let core = crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let project = core.create_project("Test").unwrap();
    let volume = core.create_volume(&project.id, "Vol").unwrap();
    let chapter = core.create_chapter(&project.id, &volume.id, "Ch1").unwrap();

    core.record_editor_change_stats(
        "harmony",
        &project.id,
        &volume.id,
        &chapter.id,
        crate::editor::EditorTransactionCause::Typing,
        18,
        0,
    )
    .unwrap();

    let summary = core.get_today_writing_stats_summary().unwrap();
    assert_eq!(
        summary["totalHumanTypedChars"], 18,
        "今日汇总必须走 Core 本地日历口径"
    );
    let today = crate::writing_stats::calendar::local_today_date();
    assert_eq!(summary["range"]["startDate"], today.as_str());
}

// ---------------------------------------------------------------------------
// per_project / per_chapter 活跃时间独立计算
// ---------------------------------------------------------------------------

#[test]
fn test_per_project_active_time_is_independent() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let now_ms = chrono::Utc::now().timestamp_millis();

    // proj1 有两个事件间隔 10 秒
    let mut e1 = speed_test_event(now_ms, 10);
    e1.project_id = "proj1".to_string();
    api.record_event(e1).unwrap();

    let mut e2 = speed_test_event(now_ms + 10_000, 5);
    e2.project_id = "proj1".to_string();
    api.record_event(e2).unwrap();

    // proj2 只有一个事件
    let mut e3 = speed_test_event(now_ms + 5_000, 20);
    e3.project_id = "proj2".to_string();
    api.record_event(e3).unwrap();

    let today = StatsApi::today_date();
    let project_stats = api
        .get_stats_by_project(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();
    let projects = project_stats["projects"].as_array().unwrap();

    let p1 = projects.iter().find(|p| p["projectId"] == "proj1").unwrap();
    assert_eq!(p1["humanTypedChars"], 15);
    assert_eq!(p1["activeSeconds"], 10); // 独立计算的活跃时间

    let p2 = projects.iter().find(|p| p["projectId"] == "proj2").unwrap();
    assert_eq!(p2["humanTypedChars"], 20);
    assert_eq!(p2["activeSeconds"], 0); // 只有一个事件，活跃时间为 0
}

#[test]
fn test_per_chapter_active_time_is_independent() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let now_ms = chrono::Utc::now().timestamp_millis();

    // chap1 有两个事件间隔 10 秒
    let mut e1 = speed_test_event(now_ms, 10);
    e1.chapter_id = "chap1".to_string();
    api.record_event(e1).unwrap();

    let mut e2 = speed_test_event(now_ms + 10_000, 5);
    e2.chapter_id = "chap1".to_string();
    api.record_event(e2).unwrap();

    // chap2 只有一个事件
    let mut e3 = speed_test_event(now_ms + 5_000, 20);
    e3.chapter_id = "chap2".to_string();
    api.record_event(e3).unwrap();

    let today = StatsApi::today_date();
    let chapter_stats = api
        .get_stats_by_chapter(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();
    let chapters = chapter_stats["chapters"].as_array().unwrap();

    let c1 = chapters.iter().find(|c| c["chapterId"] == "chap1").unwrap();
    assert_eq!(c1["humanTypedChars"], 15);
    assert_eq!(c1["activeSeconds"], 10);

    let c2 = chapters.iter().find(|c| c["chapterId"] == "chap2").unwrap();
    assert_eq!(c2["humanTypedChars"], 20);
    assert_eq!(c2["activeSeconds"], 0);
}

// ---------------------------------------------------------------------------
// 速度只计 HumanTyped
// ---------------------------------------------------------------------------

#[test]
fn test_speed_curve_only_counts_human_typed() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let now_ms = chrono::Utc::now().timestamp_millis();

    // HumanTyped 30 字
    api.record_event(speed_test_event_with_source(
        now_ms,
        30,
        EventSource::HumanTyped,
    ))
    .unwrap();
    // Unknown 500 字 — 不应进入速度曲线
    api.record_event(speed_test_event_with_source(
        now_ms + 1_000,
        500,
        EventSource::Unknown,
    ))
    .unwrap();
    // Pasted 100 字 — 不应进入速度曲线
    api.record_event(speed_test_event_with_source(
        now_ms + 2_000,
        100,
        EventSource::Pasted,
    ))
    .unwrap();

    let today = StatsApi::today_date();
    let curve = api
        .get_speed_curve(
            &DateRange {
                start_date: today.clone(),
                end_date: today,
            },
            1,
        )
        .unwrap();
    let buckets = curve["buckets"].as_array().unwrap();
    // 只有 HumanTyped 的 30 字进入速度曲线
    assert!(buckets
        .iter()
        .any(|b| b["charsTyped"].as_u64().unwrap() == 30));
    assert!(!buckets
        .iter()
        .any(|b| b["charsTyped"].as_u64().unwrap() == 500));
    assert!(!buckets
        .iter()
        .any(|b| b["charsTyped"].as_u64().unwrap() == 100));
}

// ---------------------------------------------------------------------------
// CurrentWritingSpeed 类型从 projection 模块导出
// ---------------------------------------------------------------------------

#[test]
fn test_current_writing_speed_type_from_projection() {
    let speed = CurrentWritingSpeed {
        window_seconds: 60,
        sampled_at_ms: 0,
        chars_typed: 10,
        chars_per_minute: 10.0,
    };
    assert_eq!(speed.window_seconds, 60);
    assert_eq!(speed.chars_typed, 10);
}
