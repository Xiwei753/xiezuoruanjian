use super::*;
use serde_json::json;

#[test]
fn test_date_range_dto_serialization() {
    let dto = DateRangeDto {
        start_date: "2023-01-01".to_string(),
        end_date: "2023-01-31".to_string(),
    };
    let serialized = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        serialized,
        json!({
            "startDate": "2023-01-01",
            "endDate": "2023-01-31"
        })
    );
    let deserialized: DateRangeDto = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_device_stats_record_dto_serialization() {
    let dto = DeviceStatsRecordDto {
        device_id: "dev-123".to_string(),
        platform: PlatformDto::Android,
        device_class: "phone".to_string(),
        human_typed_chars: 100,
        pasted_chars: 20,
        deleted_chars: 10,
        ai_inserted_chars: 5,
        net_delta_chars: 115,
        active_seconds: 600,
        sessions_count: 2,
    };
    let serialized = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        serialized,
        json!({
            "deviceId": "dev-123",
            "platform": "Android",
            "deviceClass": "phone",
            "humanTypedChars": 100,
            "pastedChars": 20,
            "deletedChars": 10,
            "aiInsertedChars": 5,
            "netDeltaChars": 115,
            "activeSeconds": 600,
            "sessionsCount": 2
        })
    );
    let deserialized: DeviceStatsRecordDto = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_speed_curve_point_dto_serialization() {
    let dto = SpeedCurvePointDto {
        start_ms: 1000,
        end_ms: 2000,
        chars_typed: 50,
        chars_per_minute: 120.5,
    };
    let serialized = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        serialized,
        json!({
            "startMs": 1000,
            "endMs": 2000,
            "charsTyped": 50,
            "charsPerMinute": 120.5
        })
    );
    let deserialized: SpeedCurvePointDto = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_writing_stats_summary_dto_serialization() {
    let dto = WritingStatsSummaryDto {
        range: DateRangeDto {
            start_date: "2023-10-01".to_string(),
            end_date: "2023-10-31".to_string(),
        },
        total_human_typed_chars: 1000,
        total_pasted_chars: 200,
        total_deleted_chars: 50,
        total_ai_inserted_chars: 300,
        total_net_delta_chars: 1450,
        total_active_seconds: 3600,
        total_sessions: 10,
        days_count: 5,
    };
    let serialized = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        serialized,
        json!({
            "range": {
                "startDate": "2023-10-01",
                "endDate": "2023-10-31"
            },
            "totalHumanTypedChars": 1000,
            "totalPastedChars": 200,
            "totalDeletedChars": 50,
            "totalAiInsertedChars": 300,
            "totalNetDeltaChars": 1450,
            "totalActiveSeconds": 3600,
            "totalSessions": 10,
            "daysCount": 5
        })
    );
    let deserialized: WritingStatsSummaryDto = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_project_stats_summary_dto_key_contract() {
    let summary = ProjectStatsSummaryDto {
        range: DateRangeDto {
            start_date: "2024-01-01".to_string(),
            end_date: "2024-01-31".to_string(),
        },
        projects: vec![ProjectStatsRecordDto {
            project_id: "p1".to_string(),
            human_typed_chars: 50,
            pasted_chars: 20,
            deleted_chars: 5,
            ai_inserted_chars: 2,
            net_delta_chars: 67,
            active_seconds: 120,
        }],
    };
    let json = serde_json::to_value(&summary).unwrap();
    assert_eq!(
        json,
        json!({
            "range": { "startDate": "2024-01-01", "endDate": "2024-01-31" },
            "projects": [{
                "projectId": "p1",
                "humanTypedChars": 50,
                "pastedChars": 20,
                "deletedChars": 5,
                "aiInsertedChars": 2,
                "netDeltaChars": 67,
                "activeSeconds": 120
            }]
        })
    );
    let restored: ProjectStatsSummaryDto = serde_json::from_value(json).unwrap();
    assert_eq!(restored, summary);
}

#[test]
fn test_chapter_stats_summary_dto_key_contract() {
    let summary = ChapterStatsSummaryDto {
        range: DateRangeDto {
            start_date: "2024-01-01".to_string(),
            end_date: "2024-01-31".to_string(),
        },
        chapters: vec![ChapterStatsRecordDto {
            chapter_id: "c1".to_string(),
            human_typed_chars: 50,
            pasted_chars: 20,
            deleted_chars: 5,
            ai_inserted_chars: 2,
            net_delta_chars: 67,
            active_seconds: 120,
        }],
    };
    let json = serde_json::to_value(&summary).unwrap();
    assert_eq!(
        json,
        json!({
            "range": { "startDate": "2024-01-01", "endDate": "2024-01-31" },
            "chapters": [{
                "chapterId": "c1",
                "humanTypedChars": 50,
                "pastedChars": 20,
                "deletedChars": 5,
                "aiInsertedChars": 2,
                "netDeltaChars": 67,
                "activeSeconds": 120
            }]
        })
    );
    let restored: ChapterStatsSummaryDto = serde_json::from_value(json).unwrap();
    assert_eq!(restored, summary);
}

#[test]
fn test_device_stats_summary_dto_key_contract() {
    let summary = DeviceStatsSummaryDto {
        range: DateRangeDto {
            start_date: "2024-01-01".to_string(),
            end_date: "2024-01-31".to_string(),
        },
        devices: vec![DeviceStatsRecordDto {
            device_id: "d1".to_string(),
            platform: PlatformDto::Desktop,
            device_class: "phone".to_string(),
            human_typed_chars: 50,
            pasted_chars: 20,
            deleted_chars: 5,
            ai_inserted_chars: 2,
            net_delta_chars: 67,
            active_seconds: 120,
            sessions_count: 3,
        }],
    };
    let json = serde_json::to_value(&summary).unwrap();
    assert_eq!(
        json,
        json!({
            "range": { "startDate": "2024-01-01", "endDate": "2024-01-31" },
            "devices": [{
                "deviceId": "d1",
                "platform": "Desktop",
                "deviceClass": "phone",
                "humanTypedChars": 50,
                "pastedChars": 20,
                "deletedChars": 5,
                "aiInsertedChars": 2,
                "netDeltaChars": 67,
                "activeSeconds": 120,
                "sessionsCount": 3
            }]
        })
    );
    let restored: DeviceStatsSummaryDto = serde_json::from_value(json).unwrap();
    assert_eq!(restored, summary);
}

#[test]
fn test_speed_curve_summary_dto_contract() {
    let summary = SpeedCurveSummaryDto {
        range: DateRangeDto {
            start_date: "2024-05-01".to_string(),
            end_date: "2024-05-02".to_string(),
        },
        bucket_minutes: 5,
        buckets: vec![SpeedCurvePointDto {
            start_ms: 1714521600000,
            end_ms: 1714521900000,
            chars_typed: 150,
            chars_per_minute: 30.0,
        }],
    };
    let json = serde_json::to_value(&summary).unwrap();
    assert_eq!(
        json,
        json!({
            "range": { "startDate": "2024-05-01", "endDate": "2024-05-02" },
            "bucketMinutes": 5,
            "buckets": [{
                "startMs": 1714521600000_i64,
                "endMs": 1714521900000_i64,
                "charsTyped": 150,
                "charsPerMinute": 30.0
            }]
        })
    );
    let restored: SpeedCurveSummaryDto = serde_json::from_value(json).unwrap();
    assert_eq!(restored, summary);
}

#[test]
fn test_writing_stats_explorer_dto_key_contract_with_nulls() {
    // 空历史 + 未选自选区间：钉住 camelCase 键名与 null 语义。
    let dto = WritingStatsExplorerDto {
        history_scope: "localEvents".to_string(),
        lifetime: WritingStatsLifetimeDto {
            total_human_typed_chars: 0,
            first_writing_date: None,
            last_writing_date: None,
            elapsed_calendar_days: 0,
            writing_days: 0,
            average_chars_per_day: None,
            timed_human_typed_chars: 0,
            measured_writing_seconds: 0,
            average_chars_per_hour: None,
            has_measured_timing: false,
        },
        period_cards: vec![WritingStatsPeriodCardDto {
            preset: "custom".to_string(),
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
        }],
        selected: WritingStatsSelectedPeriodDto {
            preset: "lifetime".to_string(),
            range: DateRangeDto {
                start_date: "2026-10-18".to_string(),
                end_date: "2026-10-18".to_string(),
            },
            total_human_typed_chars: 0,
            total_active_seconds: 0,
            writing_days: 0,
            average_chars_per_day: None,
            timed_human_typed_chars: 0,
            measured_writing_seconds: 0,
            average_chars_per_hour: None,
            has_measured_timing: false,
            heatmap_days: Vec::new(),
            records: WritingStatsRecordsDto {
                best_day: WritingStatsRecordDto {
                    date: None,
                    end_date: None,
                    value: None,
                },
                fastest_hour: WritingStatsRecordDto {
                    date: None,
                    end_date: None,
                    value: None,
                },
                longest_session: WritingStatsRecordDto {
                    date: None,
                    end_date: None,
                    value: None,
                },
                longest_streak: WritingStatsRecordDto {
                    date: None,
                    end_date: None,
                    value: None,
                },
            },
            trend: WritingStatsTrendDto {
                granularity: "month".to_string(),
                points: Vec::new(),
            },
        },
        contributions: None,
    };

    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        json,
        json!({
            "historyScope": "localEvents",
            "lifetime": {
                "totalHumanTypedChars": 0,
                "firstWritingDate": null,
                "lastWritingDate": null,
                "elapsedCalendarDays": 0,
                "writingDays": 0,
                "averageCharsPerDay": null,
                "timedHumanTypedChars": 0,
                "measuredWritingSeconds": 0,
                "averageCharsPerHour": null,
                "hasMeasuredTiming": false
            },
            "periodCards": [{
                "preset": "custom",
                "hasSelection": false,
                "range": null,
                "totalHumanTypedChars": 0,
                "totalActiveSeconds": 0,
                "writingDays": 0,
                "averageCharsPerDay": null,
                "timedHumanTypedChars": 0,
                "measuredWritingSeconds": 0,
                "averageCharsPerHour": null,
                "hasMeasuredTiming": false
            }],
            "selected": {
                "preset": "lifetime",
                "range": { "startDate": "2026-10-18", "endDate": "2026-10-18" },
                "totalHumanTypedChars": 0,
                "totalActiveSeconds": 0,
                "writingDays": 0,
                "averageCharsPerDay": null,
                "timedHumanTypedChars": 0,
                "measuredWritingSeconds": 0,
                "averageCharsPerHour": null,
                "hasMeasuredTiming": false,
                "heatmapDays": [],
                "records": {
                    "bestDay": { "date": null, "endDate": null, "value": null },
                    "fastestHour": { "date": null, "endDate": null, "value": null },
                    "longestSession": { "date": null, "endDate": null, "value": null },
                    "longestStreak": { "date": null, "endDate": null, "value": null }
                },
                "trend": { "granularity": "month", "points": [] }
            }
        })
    );
    let restored: WritingStatsExplorerDto = serde_json::from_value(json).unwrap();
    assert_eq!(restored, dto);
}

#[test]
fn test_writing_stats_explorer_dto_key_contract_with_values() {
    let dto = WritingStatsExplorerDto {
        history_scope: "localEvents".to_string(),
        lifetime: WritingStatsLifetimeDto {
            total_human_typed_chars: 1234,
            first_writing_date: Some("2026-03-10".to_string()),
            last_writing_date: Some("2026-10-18".to_string()),
            elapsed_calendar_days: 223,
            writing_days: 42,
            average_chars_per_day: Some(29.38),
            timed_human_typed_chars: 1234,
            measured_writing_seconds: 4000,
            average_chars_per_hour: Some(1110.6),
            has_measured_timing: true,
        },
        period_cards: vec![WritingStatsPeriodCardDto {
            preset: "last7Days".to_string(),
            has_selection: true,
            range: Some(DateRangeDto {
                start_date: "2026-10-12".to_string(),
                end_date: "2026-10-18".to_string(),
            }),
            total_human_typed_chars: 300,
            total_active_seconds: 600,
            writing_days: 5,
            average_chars_per_day: Some(60.0),
            timed_human_typed_chars: 300,
            measured_writing_seconds: 600,
            average_chars_per_hour: Some(1800.0),
            has_measured_timing: true,
        }],
        selected: WritingStatsSelectedPeriodDto {
            preset: "last7Days".to_string(),
            range: DateRangeDto {
                start_date: "2026-10-12".to_string(),
                end_date: "2026-10-18".to_string(),
            },
            total_human_typed_chars: 300,
            total_active_seconds: 600,
            writing_days: 5,
            average_chars_per_day: Some(60.0),
            timed_human_typed_chars: 300,
            measured_writing_seconds: 600,
            average_chars_per_hour: Some(1800.0),
            has_measured_timing: true,
            heatmap_days: Vec::new(),
            records: WritingStatsRecordsDto {
                best_day: WritingStatsRecordDto {
                    date: Some("2026-10-15".to_string()),
                    end_date: None,
                    value: Some(120),
                },
                fastest_hour: WritingStatsRecordDto {
                    date: Some("2026-10-15".to_string()),
                    end_date: None,
                    value: Some(90),
                },
                longest_session: WritingStatsRecordDto {
                    date: Some("2026-10-16".to_string()),
                    end_date: None,
                    value: Some(1800),
                },
                longest_streak: WritingStatsRecordDto {
                    date: Some("2026-10-14".to_string()),
                    end_date: Some("2026-10-18".to_string()),
                    value: Some(5),
                },
            },
            trend: WritingStatsTrendDto {
                granularity: "day".to_string(),
                points: vec![
                    WritingStatsTrendPointDto {
                        date: "2026-10-12".to_string(),
                        human_typed_chars: 0,
                    },
                    WritingStatsTrendPointDto {
                        date: "2026-10-13".to_string(),
                        human_typed_chars: 80,
                    },
                ],
            },
        },
        contributions: None,
    };

    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        json,
        json!({
            "historyScope": "localEvents",
            "lifetime": {
                "totalHumanTypedChars": 1234,
                "firstWritingDate": "2026-03-10",
                "lastWritingDate": "2026-10-18",
                "elapsedCalendarDays": 223,
                "writingDays": 42,
                "averageCharsPerDay": 29.38,
                "timedHumanTypedChars": 1234,
                "measuredWritingSeconds": 4000,
                "averageCharsPerHour": 1110.6,
                "hasMeasuredTiming": true
            },
            "periodCards": [{
                "preset": "last7Days",
                "hasSelection": true,
                "range": { "startDate": "2026-10-12", "endDate": "2026-10-18" },
                "totalHumanTypedChars": 300,
                "totalActiveSeconds": 600,
                "writingDays": 5,
                "averageCharsPerDay": 60.0,
                "timedHumanTypedChars": 300,
                "measuredWritingSeconds": 600,
                "averageCharsPerHour": 1800.0,
                "hasMeasuredTiming": true
            }],
            "selected": {
                "preset": "last7Days",
                "range": { "startDate": "2026-10-12", "endDate": "2026-10-18" },
                "totalHumanTypedChars": 300,
                "totalActiveSeconds": 600,
                "writingDays": 5,
                "averageCharsPerDay": 60.0,
                "timedHumanTypedChars": 300,
                "measuredWritingSeconds": 600,
                "averageCharsPerHour": 1800.0,
                "hasMeasuredTiming": true,
                "heatmapDays": [],
                "records": {
                    "bestDay": { "date": "2026-10-15", "endDate": null, "value": 120 },
                    "fastestHour": { "date": "2026-10-15", "endDate": null, "value": 90 },
                    "longestSession": { "date": "2026-10-16", "endDate": null, "value": 1800 },
                    "longestStreak": {
                        "date": "2026-10-14",
                        "endDate": "2026-10-18",
                        "value": 5
                    }
                },
                "trend": {
                    "granularity": "day",
                    "points": [
                        { "date": "2026-10-12", "humanTypedChars": 0 },
                        { "date": "2026-10-13", "humanTypedChars": 80 }
                    ]
                }
            }
        })
    );
    let restored: WritingStatsExplorerDto = serde_json::from_value(json).unwrap();
    assert_eq!(restored, dto);
}
