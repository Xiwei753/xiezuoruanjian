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
    let project = &json["projects"][0];
    assert_eq!(project["projectId"], "p1");
    assert_eq!(project["humanTypedChars"], 50);
    assert_eq!(project["netDeltaChars"], 67);
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
    let chapter = &json["chapters"][0];
    assert_eq!(chapter["chapterId"], "c1");
    assert_eq!(chapter["humanTypedChars"], 50);
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
    let device = &json["devices"][0];
    assert_eq!(device["deviceId"], "d1");
    assert_eq!(device["platform"], "Desktop");
    assert_eq!(device["deviceClass"], "phone");
    assert_eq!(device["sessionsCount"], 3);
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
    assert_eq!(json["bucketMinutes"], 5);
    let record = &json["buckets"][0];
    assert_eq!(record["startMs"], 1714521600000_i64);
    assert_eq!(record["charsTyped"], 150);
    assert!((record["charsPerMinute"].as_f64().unwrap() - 30.0).abs() < 1e-6);
    let restored: SpeedCurveSummaryDto = serde_json::from_value(json).unwrap();
    assert_eq!(restored, summary);
}
