use crate::writing_stats::aggregate::StatsAggregator;
use crate::writing_stats::api::StatsApi;
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
fn test_daily_stats_aggregation_empty_events() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    // Aggregate with no events
    let today_str = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let _ = api.aggregator().aggregate_and_save(&today_str, &today_str);

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
fn test_daily_aggregation_idempotent() {
    let temp_dir = tempdir().unwrap();
    let agg = StatsAggregator::new(temp_dir.path());

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

    agg.aggregate_single_event(&event).unwrap();

    let today = agg.store().timestamp_to_date(event.timestamp_ms).unwrap();
    let stats = agg.store().load_all_daily_stats_for_date(&today).unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].total_human_typed_chars, 10);

    agg.aggregate_single_event(&event).unwrap();
    agg.aggregate_single_event(&event).unwrap();

    let stats = agg.store().load_all_daily_stats_for_date(&today).unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].total_human_typed_chars, 30);
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
    WritingInputEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        timestamp_ms,
        device_id: "dev-1".to_string(),
        platform: Platform::Desktop,
        device_class: "desktop".to_string(),
        project_id: "proj1".to_string(),
        volume_id: "vol1".to_string(),
        chapter_id: "chap1".to_string(),
        source: EventSource::HumanTyped,
        inserted_chars,
        deleted_chars: 0,
        pasted_chars: 0,
        ai_inserted_chars: 0,
        net_delta_chars: inserted_chars as i32,
        duration_seconds: 0,
        session_id: "s1".to_string(),
    }
}

// 「当前写作速度」必须看得到还在内存缓冲里、尚未落盘的事件：
// record_event 有 3 秒防抖缓冲，用户刚停笔时最后一段输入只存在于 event_buffer。
#[test]
fn test_current_speed_reads_unflushed_buffer() {
    let temp_dir = tempdir().unwrap();
    // 走 StatsApi 记录事件：它内部的 aggregator 才是 app_service 真正用的那个
    // store（app_service/stats_ops.rs → self.api.get_current_writing_speed）。
    // 另起一个 StatsStore 会得到独立的 event_buffer，测不到同一条路径。
    let api = StatsApi::new(temp_dir.path());

    let now_ms = chrono::Utc::now().timestamp_millis();
    for i in 0..5 {
        api.record_event(speed_test_event(now_ms - 2_000 + i * 100, 10))
            .unwrap();
    }

    // 还没手动 flush：速度必须已经把缓冲里的 50 字算进去。
    let speed = api.aggregator().get_current_speed(60).unwrap();
    assert_eq!(speed.window_seconds, 60);
    assert_eq!(speed.chars_typed, 50);
    assert!((speed.chars_per_minute - 50.0).abs() < 0.001);

    // 落盘之后再查一次，数值不能变——已落盘事件和内存缓冲是同一份事实源，
    // 拼接不能重复计数。
    api.aggregator().store().flush_events().unwrap();
    let after_flush = api.aggregator().get_current_speed(60).unwrap();
    assert_eq!(after_flush.chars_typed, 50);
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
    api.aggregator().store().flush_events().unwrap();

    let speed = api.aggregator().get_current_speed(60).unwrap();
    assert_eq!(speed.chars_typed, 0);
    assert_eq!(speed.chars_per_minute, 0.0);

    // 历史曲线里那 500 字还在——曲线和实时速度职责不同，不互相污染。
    let today = StatsApi::today_date();
    let curve = api.aggregator().get_speed_curve(&today, &today, 1).unwrap();
    assert!(curve.iter().any(|b| b.chars_typed == 500));
}

// 0 秒窗口没有意义且无法折算速度，Core 钳到 1 秒而不是让除法炸掉。
#[test]
fn test_current_speed_clamps_zero_window() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let speed = api.aggregator().get_current_speed(0).unwrap();
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
    api.aggregator().store().flush_events().unwrap();

    assert_eq!(
        api.aggregator().get_current_speed(60).unwrap().chars_typed,
        7
    );
    assert_eq!(
        api.aggregator().get_current_speed(180).unwrap().chars_typed,
        7 + 999
    );
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
    store.flush_events().unwrap();

    let date = store.timestamp_to_date(event.timestamp_ms).unwrap();
    let events = store.load_events_for_date(&date).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].inserted_chars, 10);
}

#[test]
fn test_daily_stats_file_written() {
    let temp_dir = tempdir().unwrap();
    let store = StatsStore::new(temp_dir.path());

    let stats = crate::writing_stats::store::DailyStats {
        date: "2025-01-15".to_string(),
        device_id: "dev-1".to_string(),
        platform: "linux".to_string(),
        total_human_typed_chars: 100,
        ..Default::default()
    };

    store.save_or_merge_daily_stats(&stats).unwrap();

    let loaded = store.load_all_daily_stats_for_date("2025-01-15").unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].total_human_typed_chars, 100);
    assert_eq!(loaded[0].device_id, "dev-1");
}

#[test]
fn test_session_gap_detection() {
    let temp_dir = tempdir().unwrap();
    let store = StatsStore::new(temp_dir.path());

    // Align base_ms to the middle of a day to ensure base_ms and base_ms + 10 min fall on the same day.
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
    };
    store.record_event(event2).unwrap();
    store.flush_events().unwrap();

    let date = store.timestamp_to_date(base_ms).unwrap();
    let events = store.load_events_for_date(&date).unwrap();
    let daily_stats = store.aggregate_events(&events).unwrap();

    assert_eq!(daily_stats.len(), 1);
    assert_eq!(daily_stats[0].sessions_count, 2);
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
fn test_facade_record_writing_event() {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let core = crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));

    core.record_writing_event(
        "dev-1",
        "linux",
        "desktop",
        "proj1",
        "vol1",
        "chap1",
        "human_typed",
        10,
        0,
        0,
        0,
        0,
        "s1",
    )
    .unwrap();

    core.record_writing_event(
        "dev-1", "linux", "desktop", "proj1", "vol1", "chap1", "pasted", 0, 0, 20, 0, 0, "s1",
    )
    .unwrap();

    core.record_writing_event(
        "dev-1", "linux", "desktop", "proj1", "vol1", "chap1", "deleted", 0, 5, 0, 0, 0, "s1",
    )
    .unwrap();

    core.record_writing_event(
        "dev-1",
        "android",
        "phone",
        "proj1",
        "vol1",
        "chap1",
        "ai_inserted",
        0,
        0,
        0,
        30,
        0,
        "s1",
    )
    .unwrap();

    core.flush_writing_stats().unwrap();

    let today = StatsApi::today_date();
    let summary = core.get_writing_stats_summary(&today, &today).unwrap();
    assert_eq!(summary["totalHumanTypedChars"], 10);
    assert_eq!(summary["totalPastedChars"], 20);
    assert_eq!(summary["totalDeletedChars"], 5);
    assert_eq!(summary["totalAiInsertedChars"], 30);
    assert_eq!(summary["totalNetDeltaChars"], 55);
}

/// Android 直接按 Core cause 分类后传回的 source 字符串必须显式映射，
/// undo/redo/programmatic/selection 不得靠 `_ => HumanTyped` 默认分支落入人工输入。
#[test]
fn test_facade_record_writing_event_non_typed_sources_never_human_typed() {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let core = crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));

    // Android 按 cause 明确分类后发送的字符串：
    // - "typing"（Typing/TypingCommit/ImeComposition）→ HumanTyped；
    // - "pasted"（Paste）→ Pasted；
    // - "deleted"（Delete）→ Deleted；
    // - "undo"/"redo"/"programmatic"（Undo/Redo/Programmatic）→ 明确的非 HumanTyped
    //   （Unknown：不计入分类计数器，但仍计入 net_delta）。
    // - "selection"（纯光标移动不进入统计，防御性显式映射）。
    core.record_writing_event(
        "dev-1", "linux", "desktop", "proj1", "vol1", "chap1", "typing", 10, 0, 0, 0, 0, "s1",
    )
    .unwrap();
    core.record_writing_event(
        "dev-1", "linux", "desktop", "proj1", "vol1", "chap1", "pasted", 0, 0, 7, 0, 0, "s1",
    )
    .unwrap();
    core.record_writing_event(
        "dev-1", "linux", "desktop", "proj1", "vol1", "chap1", "deleted", 0, 3, 0, 0, 0, "s1",
    )
    .unwrap();
    core.record_writing_event(
        "dev-1", "linux", "desktop", "proj1", "vol1", "chap1", "undo", 0, 2, 0, 0, 0, "s1",
    )
    .unwrap();
    core.record_writing_event(
        "dev-1", "linux", "desktop", "proj1", "vol1", "chap1", "redo", 4, 0, 0, 0, 0, "s1",
    )
    .unwrap();
    core.record_writing_event(
        "dev-1",
        "linux",
        "desktop",
        "proj1",
        "vol1",
        "chap1",
        "programmatic",
        5,
        1,
        0,
        0,
        0,
        "s1",
    )
    .unwrap();
    core.record_writing_event(
        "dev-1",
        "linux",
        "desktop",
        "proj1",
        "vol1",
        "chap1",
        "selection",
        0,
        0,
        0,
        0,
        0,
        "s1",
    )
    .unwrap();

    core.flush_writing_stats().unwrap();

    let today = StatsApi::today_date();
    let summary = core.get_writing_stats_summary(&today, &today).unwrap();
    // typing 是唯一落入人工输入的来源；undo/redo/programmatic/selection 一律不是。
    assert_eq!(summary["totalHumanTypedChars"], 10);
    assert_eq!(summary["totalPastedChars"], 7);
    assert_eq!(summary["totalDeletedChars"], 3);
    // net_delta = 10(typing) + 7(pasted) - 3(deleted) - 2(undo) + 4(redo) + 5-1(programmatic)
    // 未知/非人工来源仍计入净增量，但不进任何分类计数器。
    assert_eq!(summary["totalNetDeltaChars"], 20);
}

#[test]
fn test_sync_stats_paths_outside_repo_not_blacklisted() {
    // 统计事件/缓存位于 app_data_root/app-meta/stats，不在作品仓库内。
    // events.local 不再被黑名单特判；cache/ 仍被通用 cache 模式覆盖（防御性）。
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
fn test_sync_daily_stats_not_whitelisted_in_project() {
    // 统计日报位于 app_data_root，不参与作品同步。
    assert!(!crate::sync::SyncService::is_whitelisted_path(
        "app-meta/stats/daily/2025-01-15.stats.json",
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
