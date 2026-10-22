use crate::writing_stats::api::StatsApi;
use crate::writing_stats::projection::{project_events, CurrentWritingSpeed};
use crate::writing_stats::store::StatsStore;
use crate::writing_stats::{DateRange, EventSource, Platform, WritingActivitySegment, WritingInputEvent};
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
        "",
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
        "",
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
        "",
        0,
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
        "",
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
        "",
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
        "",
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
        "",
        0,
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
        "",
        0,
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
        "",
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
        "",
        0,
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

/// #936：平台端固定 event_id 的事件重发只计一次；归属日按事件发生时间，
/// 重发时不能用「今天」覆盖。
#[test]
fn test_facade_record_editor_change_stats_dedups_and_keeps_occurred_date() {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let core = crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));

    let today = StatsApi::today_date();
    let yesterday = crate::writing_stats::calendar::shift_date(&today, -1).unwrap();
    let occurred_at_ms = chrono::Utc::now().timestamp_millis() - 24 * 60 * 60 * 1000;

    // 同 event_id 重发两次（保存失败重试 / 进程重启恢复都会这样）。
    for _ in 0..2 {
        core.record_editor_change_stats(
            "harmony",
            "proj1",
            "vol1",
            "chap1",
            crate::editor::EditorTransactionCause::Typing,
            10,
            0,
            "evt-stable-1",
            occurred_at_ms,
        )
        .unwrap();
    }

    // 只算一次，且落在事件发生当天（昨天），不漂到今天。
    let daily = core
        .get_writing_stats_summary(&yesterday, &yesterday)
        .unwrap();
    assert_eq!(daily["totalHumanTypedChars"], 10);
    let today_summary = core.get_writing_stats_summary(&today, &today).unwrap();
    assert_eq!(today_summary["totalHumanTypedChars"], 0);
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
        "",
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

// ---------------------------------------------------------------------------
// 问题1：替换操作的删除字数计入 totalDeletedChars
// ---------------------------------------------------------------------------

#[test]
fn test_typing_replace_counts_deleted() {
    // 选中5个字再输入2个字 = Typing, deleted=5, inserted=2
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event = WritingInputEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        device_id: "dev-1".to_string(),
        platform: Platform::Desktop,
        device_class: "desktop".to_string(),
        project_id: "proj1".to_string(),
        volume_id: "vol1".to_string(),
        chapter_id: "chap1".to_string(),
        source: EventSource::HumanTyped,
        inserted_chars: 2,
        deleted_chars: 5,
        pasted_chars: 0,
        ai_inserted_chars: 0,
        net_delta_chars: -3, // 2 - 5
        duration_seconds: 0,
        session_id: "s1".to_string(),
        local_date: String::new(),
    };
    api.record_event(event).unwrap();

    let today = StatsApi::today_date();
    let summary = api
        .get_stats_summary(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();

    assert_eq!(summary["totalHumanTypedChars"], 2);
    assert_eq!(summary["totalDeletedChars"], 5);
    assert_eq!(summary["totalNetDeltaChars"], -3);
}

#[test]
fn test_paste_replace_counts_deleted() {
    // 选中5个字再粘贴3个字 = Pasted, deleted=5, pasted=3
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event = WritingInputEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        device_id: "dev-1".to_string(),
        platform: Platform::Desktop,
        device_class: "desktop".to_string(),
        project_id: "proj1".to_string(),
        volume_id: "vol1".to_string(),
        chapter_id: "chap1".to_string(),
        source: EventSource::Pasted,
        inserted_chars: 0,
        deleted_chars: 5,
        pasted_chars: 3,
        ai_inserted_chars: 0,
        net_delta_chars: -2, // 3 - 5
        duration_seconds: 0,
        session_id: "s1".to_string(),
        local_date: String::new(),
    };
    api.record_event(event).unwrap();

    let today = StatsApi::today_date();
    let summary = api
        .get_stats_summary(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();

    assert_eq!(summary["totalPastedChars"], 3);
    assert_eq!(summary["totalDeletedChars"], 5);
    assert_eq!(summary["totalNetDeltaChars"], -2);
}

// ---------------------------------------------------------------------------
// 问题4：bucket_minutes = 0 钳到至少 1，不死循环
// ---------------------------------------------------------------------------

#[test]
fn test_speed_curve_zero_bucket_minutes_clamped() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let now_ms = chrono::Utc::now().timestamp_millis();
    let event = WritingInputEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        timestamp_ms: now_ms,
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

    let today = StatsApi::today_date();
    // 传 0 不应死循环，应钳到 1
    let curve = api
        .get_speed_curve(
            &DateRange {
                start_date: today.clone(),
                end_date: today,
            },
            0,
        )
        .unwrap();
    assert_eq!(curve["bucketMinutes"], 1);
    let buckets = curve["buckets"].as_array().unwrap();
    assert!(!buckets.is_empty());
}

// ---------------------------------------------------------------------------
// 问题3：daysCount 从 business_date 去重
// ---------------------------------------------------------------------------

#[test]
fn test_days_count_from_business_date() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    // 两个事件同一天
    let now_ms = chrono::Utc::now().timestamp_millis();
    let event1 = WritingInputEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        timestamp_ms: now_ms,
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
    api.record_event(event1).unwrap();

    let event2 = WritingInputEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        timestamp_ms: now_ms + 1000,
        device_id: "dev-1".to_string(),
        platform: Platform::Desktop,
        device_class: "desktop".to_string(),
        project_id: "proj1".to_string(),
        volume_id: "vol1".to_string(),
        chapter_id: "chap1".to_string(),
        source: EventSource::HumanTyped,
        inserted_chars: 3,
        deleted_chars: 0,
        pasted_chars: 0,
        ai_inserted_chars: 0,
        net_delta_chars: 3,
        duration_seconds: 0,
        session_id: "s1".to_string(),
        local_date: String::new(),
    };
    api.record_event(event2).unwrap();

    let today = StatsApi::today_date();
    let summary = api
        .get_stats_summary(&DateRange {
            start_date: today.clone(),
            end_date: today,
        })
        .unwrap();

    // 同一天 → daysCount = 1
    assert_eq!(summary["daysCount"], 1);
}

// ---------------------------------------------------------------------------
// #944 统计页 Explorer：生涯 + 时段卡 + 选中时段纪录/趋势
// ---------------------------------------------------------------------------

/// 钉住业务日的事件：时间戳只决定会话/窗口顺序，归属日由 `local_date` 决定。
fn explorer_event(
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

#[test]
fn test_explorer_empty_history_lifetime() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let snapshot = api.get_explorer_snapshot("lifetime", None, None).unwrap();
    let today = crate::writing_stats::calendar::local_today_date();

    // 只有本机事件历史：显式声明口径。
    assert_eq!(snapshot.history_scope, "localEvents");
    assert_eq!(snapshot.lifetime.total_human_typed_chars, 0);
    assert_eq!(snapshot.lifetime.first_writing_date, None);
    assert_eq!(snapshot.lifetime.last_writing_date, None);
    assert_eq!(snapshot.lifetime.elapsed_calendar_days, 0);
    assert_eq!(snapshot.lifetime.writing_days, 0);
    // 完全空生涯没有「首次输入日」：生涯日均仍是 null，不是 0。
    assert_eq!(snapshot.lifetime.average_chars_per_day, None);
    assert_eq!(snapshot.lifetime.average_chars_per_hour, None);

    // 空历史：range 退化成 今天..今天，趋势为空，纪录全 null，不用 0 冒充。
    assert_eq!(snapshot.selected.preset, "lifetime");
    assert_eq!(snapshot.selected.range.start_date, today);
    assert_eq!(snapshot.selected.range.end_date, today);
    assert_eq!(snapshot.selected.trend.granularity, "month");
    assert!(snapshot.selected.trend.points.is_empty());
    assert_eq!(
        snapshot.selected.records.best_day,
        crate::api::types::WritingStatsRecordDto {
            date: None,
            end_date: None,
            value: None
        }
    );
    assert!(snapshot.selected.records.fastest_hour.value.is_none());
    assert!(snapshot.selected.records.longest_session.date.is_none());
    assert!(snapshot.selected.records.longest_streak.date.is_none());

    // 四张卡顺序固定；没有自选日期时 custom 卡是空卡。
    let presets: Vec<&str> = snapshot
        .period_cards
        .iter()
        .map(|card| card.preset.as_str())
        .collect();
    assert_eq!(
        presets,
        vec!["last7Days", "last30Days", "last6Months", "custom"]
    );
    for card in snapshot.period_cards.iter().take(3) {
        assert!(card.has_selection);
        assert!(card.range.is_some());
        assert_eq!(card.total_human_typed_chars, 0);
        // 范围有效但没有输入：区间日均是 0，不是 null。
        assert_eq!(card.average_chars_per_day, Some(0.0));
        assert_eq!(card.average_chars_per_hour, None);
    }
    // 选中生涯的区间退化成 今天..今天，同样按 1 个自然日算：0 / 1 = 0。
    assert_eq!(snapshot.selected.average_chars_per_day, Some(0.0));
    assert_eq!(snapshot.selected.average_chars_per_hour, None);
    let custom = &snapshot.period_cards[3];
    assert!(!custom.has_selection);
    assert!(custom.range.is_none());
    assert_eq!(custom.total_active_seconds, 0);
}

#[test]
fn test_explorer_rejects_invalid_preset_and_custom_dates() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    assert!(api.get_explorer_snapshot("lastWeek", None, None).is_err());
    assert!(api.get_explorer_snapshot("custom", None, None).is_err());
    assert!(api
        .get_explorer_snapshot("custom", Some("2026-05-01"), None)
        .is_err());
    assert!(api
        .get_explorer_snapshot("custom", None, Some("2026-05-31"))
        .is_err());
    assert!(api
        .get_explorer_snapshot("custom", Some("2026-05-31"), Some("2026-05-01"))
        .is_err());
    assert!(api
        .get_explorer_snapshot("custom", Some("2026-13-01"), Some("2026-05-31"))
        .is_err());
    // 非 custom 预设也不接受只给一端的自选区间。
    assert!(api
        .get_explorer_snapshot("last7Days", Some("2026-05-01"), None)
        .is_err());
}

#[test]
fn test_history_scope_reports_synced_devices_when_remote_partition_present() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    crate::settings::ensure_device_info(dir, "", "", Some("device_local")).unwrap();
    let remote_dir = dir.join("stats/events/device_remote");
    std::fs::create_dir_all(&remote_dir).unwrap();
    let event = explorer_event(1_714_550_400_000, "2024-05-01", EventSource::HumanTyped, 7);
    std::fs::write(
        remote_dir.join("2024-05-01.events.jsonl"),
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();

    let api = StatsApi::new(dir);
    let snapshot = api.get_explorer_snapshot("lifetime", None, None).unwrap();

    // 已到达其它设备的事件分区：如实声明范围，并参与聚合。
    assert_eq!(snapshot.history_scope, "syncedDevices");
    assert_eq!(snapshot.lifetime.total_human_typed_chars, 7);
}

#[test]
fn test_history_scope_reports_partial_sync_when_stats_group_failed() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    crate::settings::ensure_device_info(dir, "", "", Some("device_local")).unwrap();
    let remote_dir = dir.join("stats/events/device_remote");
    std::fs::create_dir_all(&remote_dir).unwrap();
    let event = explorer_event(1_714_550_400_000, "2024-05-01", EventSource::HumanTyped, 7);
    std::fs::write(
        remote_dir.join("2024-05-01.events.jsonl"),
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();

    // 该设备的统计同步组最近一次同步失败：不能宣称已同步完整。
    let group_dir = dir.join("app-meta/sync/groups/stats-device-6465766963655f72656d6f7465");
    std::fs::create_dir_all(&group_dir).unwrap();
    std::fs::write(
        group_dir.join("state.local.json"),
        "{\"last_error\":\"network timeout\"}",
    )
    .unwrap();

    let api = StatsApi::new(dir);
    let snapshot = api.get_explorer_snapshot("lifetime", None, None).unwrap();
    assert_eq!(snapshot.history_scope, "partialSync");
}

#[test]
fn test_history_scope_partial_when_local_only_and_sync_failed() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    let group_dir = dir.join("app-meta/sync/groups/stats-device-6465766963655f72656d6f7465");
    std::fs::create_dir_all(&group_dir).unwrap();
    std::fs::write(
        group_dir.join("state.local.json"),
        "{\"last_error\":\"offline\"}",
    )
    .unwrap();

    let api = StatsApi::new(dir);
    assert_eq!(api.history_scope(), "partialSync");
}

#[test]
fn test_explorer_custom_card_filled_even_when_selected_is_other_preset() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    // 选中近七天，但前端已经记住用户上一次的自选区间：卡片照算。
    let snapshot = api
        .get_explorer_snapshot("last7Days", Some("2026-05-10"), Some("2026-05-11"))
        .unwrap();
    assert_eq!(snapshot.selected.preset, "last7Days");
    let custom = &snapshot.period_cards[3];
    assert_eq!(custom.preset, "custom");
    assert!(custom.has_selection);
    let range = custom.range.as_ref().unwrap();
    assert_eq!(range.start_date, "2026-05-10");
    assert_eq!(range.end_date, "2026-05-11");
    // 区间里没有数据时数值为 0；日均按 2 个自然日算为 0，字/分为 null，range 仍是用户选的。
    assert_eq!(custom.total_human_typed_chars, 0);
    assert_eq!(custom.average_chars_per_day, Some(0.0));
    assert_eq!(custom.average_chars_per_hour, None);
}

#[test]
fn test_explorer_custom_selected_counts_both_ends() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    // 05-10 100 字（两次输入），05-11 50 字，05-12 80 字；粘贴不计入人工码字。
    api.record_event(explorer_event(
        1_780_000_000_000,
        "2026-05-10",
        EventSource::HumanTyped,
        60,
    ))
    .unwrap();
    api.record_event(explorer_event(
        1_780_000_030_000,
        "2026-05-10",
        EventSource::HumanTyped,
        40,
    ))
    .unwrap();
    api.record_event(explorer_event(
        1_780_000_060_000,
        "2026-05-10",
        EventSource::Pasted,
        999,
    ))
    .unwrap();
    api.record_event(explorer_event(
        1_780_090_000_000,
        "2026-05-11",
        EventSource::HumanTyped,
        50,
    ))
    .unwrap();
    api.record_event(explorer_event(
        1_780_180_000_000,
        "2026-05-12",
        EventSource::HumanTyped,
        80,
    ))
    .unwrap();

    let snapshot = api
        .get_explorer_snapshot("custom", Some("2026-05-10"), Some("2026-05-11"))
        .unwrap();

    assert_eq!(snapshot.selected.preset, "custom");
    assert_eq!(snapshot.selected.range.start_date, "2026-05-10");
    assert_eq!(snapshot.selected.range.end_date, "2026-05-11");
    // 两端包含：05-10 + 05-11，区间外的 05-12 不算；粘贴不算。
    assert_eq!(snapshot.selected.total_human_typed_chars, 150);
    assert_eq!(snapshot.selected.writing_days, 2);
    assert_eq!(
        snapshot.selected.records.best_day.date.as_deref(),
        Some("2026-05-10")
    );
    assert_eq!(snapshot.selected.records.best_day.value, Some(100));
    assert_eq!(
        snapshot.selected.records.longest_streak.date.as_deref(),
        Some("2026-05-10")
    );
    assert_eq!(
        snapshot.selected.records.longest_streak.end_date.as_deref(),
        Some("2026-05-11")
    );
    assert_eq!(snapshot.selected.records.longest_streak.value, Some(2));
    assert_eq!(snapshot.selected.trend.granularity, "day");
    let points: Vec<(&str, u64)> = snapshot
        .selected
        .trend
        .points
        .iter()
        .map(|point| (point.date.as_str(), point.human_typed_chars))
        .collect();
    assert_eq!(points, vec![("2026-05-10", 100), ("2026-05-11", 50)]);

    // 生涯覆盖全部本地事件（含区间外的 05-12）。
    assert_eq!(snapshot.lifetime.total_human_typed_chars, 230);
    assert_eq!(
        snapshot.lifetime.first_writing_date.as_deref(),
        Some("2026-05-10")
    );
    assert_eq!(
        snapshot.lifetime.last_writing_date.as_deref(),
        Some("2026-05-12")
    );
    assert_eq!(snapshot.lifetime.writing_days, 3);
    let today = crate::writing_stats::calendar::local_today_date();
    let expected_elapsed =
        crate::writing_stats::calendar::day_span_inclusive("2026-05-10", &today).unwrap();
    assert_eq!(snapshot.lifetime.elapsed_calendar_days, expected_elapsed);
}

#[test]
fn test_explorer_last7_days_window_with_real_timestamps() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());
    let day_ms = 24 * 60 * 60 * 1000;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let today = crate::writing_stats::calendar::local_date_at(now_ms).unwrap();
    let three_days_ago = now_ms - 3 * day_ms;
    let ten_days_ago = now_ms - 10 * day_ms;

    for (timestamp_ms, chars) in [(now_ms, 10_u32), (three_days_ago, 3), (ten_days_ago, 40)] {
        api.record_event(explorer_event(
            timestamp_ms,
            &crate::writing_stats::calendar::local_date_at(timestamp_ms).unwrap(),
            EventSource::HumanTyped,
            chars,
        ))
        .unwrap();
    }

    let snapshot = api.get_explorer_snapshot("last7Days", None, None).unwrap();
    // 近七天含今天共 7 日：今天 + 三天前；十天前不在。
    assert_eq!(snapshot.period_cards[0].total_human_typed_chars, 13);
    // 近三十天/近半年包含十天前。
    assert_eq!(snapshot.period_cards[1].total_human_typed_chars, 53);
    assert_eq!(snapshot.period_cards[2].total_human_typed_chars, 53);

    assert_eq!(snapshot.selected.total_human_typed_chars, 13);
    assert_eq!(snapshot.selected.writing_days, 2);
    assert_eq!(snapshot.selected.trend.granularity, "day");
    assert_eq!(snapshot.selected.trend.points.len(), 7);
    let trend_sum: u64 = snapshot
        .selected
        .trend
        .points
        .iter()
        .map(|point| point.human_typed_chars)
        .sum();
    assert_eq!(trend_sum, 13);
    let first_point = &snapshot.selected.trend.points[0];
    assert_eq!(
        first_point.date,
        crate::writing_stats::calendar::shift_date(&today, -6).unwrap()
    );
}

#[test]
fn test_explorer_period_average_uses_natural_calendar_days() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());
    let today = crate::writing_stats::calendar::local_today_date();
    let now_ms = chrono::Utc::now().timestamp_millis();

    // 近七天里只写了一天：700 字。
    api.record_event(explorer_event(now_ms, &today, EventSource::HumanTyped, 700))
        .unwrap();

    let snapshot = api.get_explorer_snapshot("last7Days", None, None).unwrap();
    let card = &snapshot.period_cards[0];
    assert_eq!(card.preset, "last7Days");
    assert_eq!(card.total_human_typed_chars, 700);
    assert_eq!(card.writing_days, 1);
    // 分母是区间的自然日历天数（含 6 个无输入日）：700 / 7 = 100，不是 700 / 1。
    assert_eq!(card.average_chars_per_day, Some(100.0));
    // 选中时段与时段卡同口径。
    assert_eq!(snapshot.selected.average_chars_per_day, Some(100.0));
}

#[test]
fn test_explorer_lifetime_average_uses_elapsed_calendar_days() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());
    let today = crate::writing_stats::calendar::local_today_date();
    let first_day = crate::writing_stats::calendar::shift_date(&today, -9).unwrap();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let day_ms = 24 * 60 * 60 * 1000;

    // 生涯首写日到今天共 10 个自然日，只有 2 天写字：300 + 700 = 1000。
    api.record_event(explorer_event(
        now_ms - 9 * day_ms,
        &first_day,
        EventSource::HumanTyped,
        300,
    ))
    .unwrap();
    api.record_event(explorer_event(now_ms, &today, EventSource::HumanTyped, 700))
        .unwrap();

    let snapshot = api.get_explorer_snapshot("lifetime", None, None).unwrap();
    assert_eq!(
        snapshot.lifetime.first_writing_date.as_deref(),
        Some(first_day.as_str())
    );
    assert_eq!(snapshot.lifetime.elapsed_calendar_days, 10);
    assert_eq!(snapshot.lifetime.writing_days, 2);
    // 分母是生涯自然日历天数：1000 / 10 = 100，不是 1000 / 2。
    assert_eq!(snapshot.lifetime.average_chars_per_day, Some(100.0));
    // 选中生涯复用同一份生涯统计，与 lifetime 同口径。
    assert_eq!(snapshot.selected.preset, "lifetime");
    assert_eq!(snapshot.selected.average_chars_per_day, Some(100.0));
}

#[test]
fn test_explorer_trend_granularity_per_preset() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());
    let now_ms = chrono::Utc::now().timestamp_millis();
    let today = crate::writing_stats::calendar::local_today_date();
    api.record_event(explorer_event(now_ms, &today, EventSource::HumanTyped, 10))
        .unwrap();

    let granularity = |preset: &str, start: Option<&str>, end: Option<&str>| {
        api.get_explorer_snapshot(preset, start, end)
            .unwrap()
            .selected
            .trend
            .granularity
    };
    assert_eq!(granularity("last7Days", None, None), "day");
    assert_eq!(granularity("last30Days", None, None), "day");
    assert_eq!(granularity("last6Months", None, None), "week");
    assert_eq!(granularity("lifetime", None, None), "month");

    // 自选按跨度选粒度：≤31 天按天、≤200 天按周、更长按月。
    let short_start = crate::writing_stats::calendar::shift_date(&today, -30).unwrap();
    let week_start = crate::writing_stats::calendar::shift_date(&today, -40).unwrap();
    let month_start = crate::writing_stats::calendar::shift_date(&today, -250).unwrap();
    assert_eq!(
        granularity("custom", Some(&short_start), Some(&today)),
        "day"
    );
    assert_eq!(
        granularity("custom", Some(&week_start), Some(&today)),
        "week"
    );
    assert_eq!(
        granularity("custom", Some(&month_start), Some(&today)),
        "month"
    );
}

#[test]
fn test_explorer_short_lifetime_trend_starts_at_first_writing_month() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());
    let today = crate::writing_stats::calendar::local_today_date();
    // 五个月前开写：短于一年，生涯趋势按「首次写作所在月 → 今天所在月」。
    let first_date = crate::writing_stats::calendar::shift_months(&today, -5).unwrap();
    api.record_event(explorer_event(
        chrono::Utc::now().timestamp_millis(),
        &first_date,
        EventSource::HumanTyped,
        7,
    ))
    .unwrap();

    let snapshot = api.get_explorer_snapshot("lifetime", None, None).unwrap();
    assert_eq!(
        snapshot.lifetime.first_writing_date.as_deref(),
        Some(first_date.as_str())
    );
    assert_eq!(snapshot.selected.range.start_date, first_date);
    assert_eq!(snapshot.selected.trend.granularity, "month");
    assert_eq!(snapshot.selected.trend.points.len(), 6);
    let first_month = crate::writing_stats::calendar::month_start_date(&first_date).unwrap();
    let last_month = crate::writing_stats::calendar::month_start_date(&today).unwrap();
    assert_eq!(snapshot.selected.trend.points[0].date, first_month);
    assert_eq!(snapshot.selected.trend.points[0].human_typed_chars, 7);
    assert_eq!(snapshot.selected.trend.points[5].date, last_month);
    assert_eq!(snapshot.selected.trend.points[5].human_typed_chars, 0);
}

#[test]
fn test_explorer_zero_human_input_keeps_records_null() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());
    // 只有粘贴：不产生「写作」纪录，也不产生字/分。
    api.record_event(explorer_event(
        1_780_000_000_000,
        "2026-05-10",
        EventSource::Pasted,
        500,
    ))
    .unwrap();

    let snapshot = api
        .get_explorer_snapshot("custom", Some("2026-05-10"), Some("2026-05-10"))
        .unwrap();
    assert_eq!(snapshot.selected.total_human_typed_chars, 0);
    assert_eq!(snapshot.selected.writing_days, 0);
    // 区间有效（1 个自然日）但没写字：日均是 0，不是 null。
    assert_eq!(snapshot.selected.average_chars_per_day, Some(0.0));
    assert_eq!(snapshot.selected.average_chars_per_hour, None);
    assert!(snapshot.selected.records.best_day.value.is_none());
    assert!(snapshot.selected.records.fastest_hour.value.is_none());
    assert!(snapshot.selected.records.longest_streak.value.is_none());
    // 趋势照常补 0，不是一个点都没有。
    assert_eq!(snapshot.selected.trend.points.len(), 1);
    assert_eq!(snapshot.selected.trend.points[0].human_typed_chars, 0);
    // 生涯没有人工码字：没有首次输入日，生涯日均保持 null（空生涯不是 0）。
    assert_eq!(snapshot.lifetime.total_human_typed_chars, 0);
    assert_eq!(snapshot.lifetime.elapsed_calendar_days, 0);
    assert_eq!(snapshot.lifetime.average_chars_per_day, None);
}

#[test]
fn test_explorer_duplicate_event_resend_counts_once() {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let core = crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let now_ms = chrono::Utc::now().timestamp_millis();

    // 同一编辑事件重发三次（保存失败重试 / 进程恢复）：只落一次、只计一次。
    for _ in 0..3 {
        core.record_editor_change_stats(
            "harmony",
            "proj1",
            "vol1",
            "chap1",
            crate::editor::EditorTransactionCause::Typing,
            10,
            0,
            "evt-explorer-resend",
            now_ms,
        )
        .unwrap();
    }

    let snapshot = core
        .get_writing_stats_explorer("last7Days", None, None)
        .unwrap();
    assert_eq!(snapshot["historyScope"], "localEvents");
    assert_eq!(snapshot["lifetime"]["totalHumanTypedChars"], 10);
    assert_eq!(snapshot["selected"]["totalHumanTypedChars"], 10);
    assert_eq!(snapshot["selected"]["writingDays"], 1);
    // 单条事件没有会话时长：不捏造「字/分」。
    assert!(snapshot["selected"]["averageCharsPerHour"].is_null());
    assert_eq!(
        snapshot["selected"]["records"]["longestSession"]["date"],
        serde_json::Value::Null
    );
}

#[test]
fn test_explorer_multi_device_same_window_not_one_long_session() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    // dev-a：10:00–10:01；dev-b：10:00:30–10:01:30。混在一起会变成 90 秒假会话。
    let base = 1_780_000_000_000_i64;
    let mut a1 = explorer_event(base, "2026-05-10", EventSource::HumanTyped, 10);
    a1.device_id = "dev-a".to_string();
    let mut a2 = explorer_event(base + 60_000, "2026-05-10", EventSource::HumanTyped, 10);
    a2.device_id = "dev-a".to_string();
    let mut b1 = explorer_event(base + 30_000, "2026-05-10", EventSource::HumanTyped, 10);
    b1.device_id = "dev-b".to_string();
    let mut b2 = explorer_event(base + 90_000, "2026-05-10", EventSource::HumanTyped, 10);
    b2.device_id = "dev-b".to_string();
    for event in [a1, a2, b1, b2] {
        api.record_event(event).unwrap();
    }

    // 真实计时来自平台切片，不是首末按键时间差：两台设备各 60 秒，并列最长。
    for (event_id, device_id, session_id, start, end) in [
        ("seg-a", "dev-a", "sess-a", base, base + 60_000),
        ("seg-b", "dev-b", "sess-b", base + 30_000, base + 90_000),
    ] {
        let segment = WritingActivitySegment::new(
            event_id.to_string(),
            device_id,
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            session_id,
            start,
            end,
            "heartbeat",
        )
        .unwrap();
        api.record_activity_segment(segment).unwrap();
    }

    let snapshot = api
        .get_explorer_snapshot("custom", Some("2026-05-10"), Some("2026-05-10"))
        .unwrap();
    assert_eq!(snapshot.selected.total_active_seconds, 120);
    assert_eq!(
        snapshot.selected.records.longest_session.value,
        Some(60),
        "不同设备的并发会话不能并成一条超长会话"
    );
    assert_eq!(
        snapshot.selected.records.longest_session.date.as_deref(),
        Some("2026-05-10")
    );
    // 落在计时窗口内的 30 字 × 3600 / 120 秒 = 900 字/时。
    assert_eq!(snapshot.selected.timed_human_typed_chars, 30);
    assert_eq!(snapshot.selected.measured_writing_seconds, 120);
    let per_hour = snapshot.selected.average_chars_per_hour.unwrap();
    assert!((per_hour - 900.0).abs() < 0.001);
}

// ---------------------------------------------------------------------------
// #956 每日码字日历：完整自然年逐日真值 + 强度分级
// ---------------------------------------------------------------------------

#[test]
fn test_heatmap_year_length_follows_gregorian_calendar() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let leap = api.get_heatmap_snapshot(2024).unwrap();
    assert_eq!(leap.year, 2024);
    assert_eq!(leap.start_date, "2024-01-01");
    assert_eq!(leap.end_date, "2024-12-31");
    assert_eq!(leap.days.len(), 366, "2024 是闰年，应返回 366 个自然日");
    assert_eq!(leap.days.first().unwrap().date, "2024-01-01");
    assert_eq!(leap.days.last().unwrap().date, "2024-12-31");
    // 按日期升序且每天唯一。
    for pair in leap.days.windows(2) {
        assert!(pair[0].date < pair[1].date);
    }

    let common = api.get_heatmap_snapshot(2025).unwrap();
    assert_eq!(common.days.len(), 365, "2025 是平年，应返回 365 个自然日");
    assert_eq!(common.start_date, "2025-01-01");
    assert_eq!(common.end_date, "2025-12-31");
}

#[test]
fn test_heatmap_reports_daily_chars_and_four_levels() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    for (ts, date, chars) in [
        (1_714_550_400_000_i64, "2024-05-01", 10_u32),
        (1_714_636_800_000, "2024-05-02", 20),
        (1_714_723_200_000, "2024-05-03", 30),
        (1_714_809_600_000, "2024-05-04", 40),
    ] {
        api.record_event(explorer_event(ts, date, EventSource::HumanTyped, chars))
            .unwrap();
    }

    let heatmap = api.get_heatmap_snapshot(2024).unwrap();
    let mut by_date = std::collections::HashMap::new();
    for day in &heatmap.days {
        by_date.insert(day.date.as_str(), day.clone());
    }

    assert_eq!(by_date["2024-05-01"].human_typed_chars, 10);
    assert_eq!(by_date["2024-05-01"].intensity, 1);
    assert_eq!(by_date["2024-05-02"].intensity, 2);
    assert_eq!(by_date["2024-05-03"].intensity, 3);
    assert_eq!(by_date["2024-05-04"].intensity, 4, "最大正数日应到最高一级");
    // 没有记录的已过日期是真实 0，不是缺失。
    assert_eq!(by_date["2024-06-01"].human_typed_chars, 0);
    assert_eq!(by_date["2024-06-01"].intensity, 0);
}

#[test]
fn test_heatmap_all_equal_positive_days_grade_stably() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    for (ts, date) in [
        (1_714_550_400_000_i64, "2024-05-01"),
        (1_714_636_800_000, "2024-05-02"),
    ] {
        api.record_event(explorer_event(ts, date, EventSource::HumanTyped, 50))
            .unwrap();
    }

    let heatmap = api.get_heatmap_snapshot(2024).unwrap();
    let graded: Vec<u8> = heatmap
        .days
        .iter()
        .filter(|day| day.human_typed_chars > 0)
        .map(|day| day.intensity)
        .collect();
    assert_eq!(graded, vec![4, 4], "正数全相同也要确定性分级，不除零");
}

#[test]
fn test_heatmap_rejects_invalid_year() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    assert!(api.get_heatmap_snapshot(1900).is_err());
    assert!(api.get_heatmap_snapshot(2200).is_err());
}

#[test]
fn test_heatmap_merges_remote_device_partition_and_keeps_future_days() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    let remote_dir = dir.join("stats/events/device_remote");
    std::fs::create_dir_all(&remote_dir).unwrap();
    let event = explorer_event(1_714_550_400_000, "2024-05-01", EventSource::HumanTyped, 7);
    std::fs::write(
        remote_dir.join("2024-05-01.events.jsonl"),
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();

    let api = StatsApi::new(dir);
    let heatmap = api.get_heatmap_snapshot(2024).unwrap();
    let day = heatmap
        .days
        .iter()
        .find(|day| day.date == "2024-05-01")
        .unwrap();
    assert_eq!(day.human_typed_chars, 7, "已同步设备的事件也应计入每日真值");

    // 尚未到来的日期仍在列表里，由日期字段交给 UI 标记未到，不伪造成 0 字历史。
    let future_year = api.get_heatmap_snapshot(2100).unwrap();
    assert_eq!(future_year.days.len(), 365);
    assert_eq!(future_year.days.first().unwrap().date, "2100-01-01");
    assert_eq!(future_year.days.first().unwrap().status, "future");
    assert_eq!(future_year.days.last().unwrap().date, "2100-12-31");
    assert_eq!(future_year.days.last().unwrap().status, "future");
}

#[test]
fn test_heatmap_status_distinguishes_known_unknown_and_future() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    // 2024-05-10T08:00Z，业务日 2024-05-10。
    api.record_event(explorer_event(
        1_715_328_000_000,
        "2024-05-10",
        EventSource::HumanTyped,
        6,
    ))
    .unwrap();

    let heatmap = api.get_heatmap_snapshot(2024).unwrap();
    let status = |date: &str| -> String {
        heatmap
            .days
            .iter()
            .find(|day| day.date == date)
            .unwrap()
            .status
            .clone()
    };

    assert_eq!(
        status("2024-05-10"),
        "known",
        "有记录的日子必须是已确认覆盖"
    );
    assert_eq!(
        status("2024-05-11"),
        "known",
        "覆盖锚点之后的已过日期可确认为真实 0"
    );
    assert_eq!(
        status("2024-05-09"),
        "unknown",
        "锚点之前没有覆盖依据，不得当没写"
    );
}

#[test]
fn test_heatmap_marks_unconfirmed_days_unknown_when_sync_incomplete() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    // 任一统计设备同步组最近同步失败：历史可能不完整。
    let group_dir = dir.join("app-meta/sync/groups/stats-device-6465766963655f72656d6f7465");
    std::fs::create_dir_all(&group_dir).unwrap();
    std::fs::write(
        group_dir.join("state.local.json"),
        "{\"last_error\":\"network timeout\"}",
    )
    .unwrap();

    let api = StatsApi::new(dir);
    api.record_event(explorer_event(
        1_715_328_000_000,
        "2024-05-10",
        EventSource::HumanTyped,
        6,
    ))
    .unwrap();

    let heatmap = api.get_heatmap_snapshot(2024).unwrap();
    assert_eq!(heatmap.history_scope, "partialSync");
    let recorded = heatmap
        .days
        .iter()
        .find(|day| day.date == "2024-05-10")
        .unwrap();
    assert_eq!(recorded.status, "known");
    let unconfirmed = heatmap
        .days
        .iter()
        .find(|day| day.date == "2024-05-11")
        .unwrap();
    assert_eq!(
        unconfirmed.status, "unknown",
        "同步不完整时，没记录的日子不能当真实 0"
    );
}
