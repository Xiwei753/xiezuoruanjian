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
        Platform::Linux,
        "proj1",
        "vol1",
        "chap1",
        EventSource::HumanTyped,
        10,
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
        Platform::Linux,
        "proj1",
        "vol1",
        "chap1",
        EventSource::Pasted,
        0,
        0,
        20,
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
        Platform::Linux,
        "proj1",
        "vol1",
        "chap1",
        EventSource::HumanTyped,
        10,
        0,
        0,
        0,
        "s1",
    );
    api.record_event(event1).unwrap();

    let event2 = WritingInputEvent::new(
        "dev-1",
        Platform::Linux,
        "proj1",
        "vol1",
        "chap1",
        EventSource::Deleted,
        0,
        3,
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
        Platform::Linux,
        "proj1",
        "vol1",
        "chap1",
        EventSource::AiInserted,
        0,
        0,
        0,
        50,
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
        Platform::Linux,
        "proj1",
        "vol1",
        "chap1",
        EventSource::SyncRemote,
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
fn test_daily_aggregation_idempotent() {
    let temp_dir = tempdir().unwrap();
    let agg = StatsAggregator::new(temp_dir.path());

    let event = WritingInputEvent::new(
        "dev-1",
        Platform::Linux,
        "proj1",
        "vol1",
        "chap1",
        EventSource::HumanTyped,
        10,
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
        Platform::Linux,
        "proj1",
        "vol1",
        "chap1",
        EventSource::HumanTyped,
        10,
        0,
        0,
        0,
        "s1",
    );
    api.record_event(event1).unwrap();

    let event2 = WritingInputEvent::new(
        "dev-android",
        Platform::Android,
        "proj1",
        "vol1",
        "chap1",
        EventSource::HumanTyped,
        20,
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
            platform: Platform::Linux,
            project_id: "proj1".to_string(),
            volume_id: "vol1".to_string(),
            chapter_id: "chap1".to_string(),
            source: EventSource::HumanTyped,
            inserted_chars: 5,
            deleted_chars: 0,
            pasted_chars: 0,
            ai_inserted_chars: 0,
            net_delta_chars: 5,
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

#[test]
fn test_per_project_tracking() {
    let temp_dir = tempdir().unwrap();
    let api = StatsApi::new(temp_dir.path());

    let event = WritingInputEvent::new(
        "dev-1",
        Platform::Linux,
        "proj-abc",
        "vol1",
        "chap1",
        EventSource::HumanTyped,
        15,
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
        Platform::Linux,
        "proj1",
        "vol1",
        "chap-xyz",
        EventSource::HumanTyped,
        25,
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
        Platform::Linux,
        "proj1",
        "vol1",
        "chap1",
        EventSource::HumanTyped,
        10,
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
        platform: Platform::Linux,
        project_id: "proj1".to_string(),
        volume_id: "vol1".to_string(),
        chapter_id: "chap1".to_string(),
        source: EventSource::HumanTyped,
        inserted_chars: 5,
        deleted_chars: 0,
        pasted_chars: 0,
        ai_inserted_chars: 0,
        net_delta_chars: 5,
        session_id: "s1".to_string(),
    };
    store.record_event(event1).unwrap();

    let event2 = WritingInputEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        timestamp_ms: base_ms + 10 * 60 * 1000,
        device_id: "dev-1".to_string(),
        platform: Platform::Linux,
        project_id: "proj1".to_string(),
        volume_id: "vol1".to_string(),
        chapter_id: "chap1".to_string(),
        source: EventSource::HumanTyped,
        inserted_chars: 5,
        deleted_chars: 0,
        pasted_chars: 0,
        ai_inserted_chars: 0,
        net_delta_chars: 5,
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
        Platform::Linux,
        "proj1",
        "vol1",
        "chap1",
        EventSource::HumanTyped,
        char_count as u32,
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
    crate::workspace::create_workspace(temp_dir.path()).unwrap();
    let core = crate::facade::WriterCore::new(temp_dir.path());

    core.record_writing_event(
        "dev-1",
        "linux",
        "proj1",
        "vol1",
        "chap1",
        "human_typed",
        10,
        0,
        0,
        0,
        "s1",
    )
    .unwrap();

    core.record_writing_event(
        "dev-1", "linux", "proj1", "vol1", "chap1", "pasted", 0, 0, 20, 0, "s1",
    )
    .unwrap();

    core.record_writing_event(
        "dev-1", "linux", "proj1", "vol1", "chap1", "deleted", 0, 5, 0, 0, "s1",
    )
    .unwrap();

    core.record_writing_event(
        "dev-1",
        "android",
        "proj1",
        "vol1",
        "chap1",
        "ai_inserted",
        0,
        0,
        0,
        30,
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

#[test]
fn test_sync_blacklist_events_local() {
    assert!(crate::sync::SyncService::is_blacklisted_path(
        "app-meta/stats/events.local/2025-01-15.events.jsonl"
    ));
}

#[test]
fn test_sync_blacklist_stats_cache() {
    assert!(crate::sync::SyncService::is_blacklisted_path(
        "app-meta/stats/cache/something.json"
    ));
}

#[test]
fn test_sync_whitelist_daily_stats() {
    assert!(crate::sync::SyncService::is_whitelisted_path(
        "app-meta/stats/daily/2025-01-15.stats.json"
    ));
}

#[test]
fn test_load_chapter_does_not_produce_input_events() {
    let temp_dir = tempdir().unwrap();
    crate::workspace::create_workspace(temp_dir.path()).unwrap();
    let core = crate::facade::WriterCore::new(temp_dir.path());

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
