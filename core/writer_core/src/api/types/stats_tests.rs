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
