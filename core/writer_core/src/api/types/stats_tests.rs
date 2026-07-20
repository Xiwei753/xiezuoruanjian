use crate::api::types::*;

#[test]
fn test_date_range_dto_serialization() {
    let dto = DateRangeDto {
        start_date: "2023-01-01".to_string(),
        end_date: "2023-01-31".to_string(),
    };

    let json = serde_json::to_string(&dto).unwrap();
    assert_eq!(json, r#"{"startDate":"2023-01-01","endDate":"2023-01-31"}"#);

    let deserialized: DateRangeDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, deserialized);
}

#[test]
fn test_writing_stats_summary_dto_serialization() {
    let dto = WritingStatsSummaryDto {
        range: DateRangeDto {
            start_date: "2023-01-01".to_string(),
            end_date: "2023-01-31".to_string(),
        },
        total_human_typed_chars: 100,
        total_pasted_chars: 20,
        total_deleted_chars: 5,
        total_ai_inserted_chars: 10,
        total_net_delta_chars: -125, // Testing negative net delta chars boundary
        total_active_seconds: 3600,
        total_sessions: 5,
        days_count: 2,
    };

    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains(r#""totalHumanTypedChars":100"#));
    assert!(json.contains(r#""totalNetDeltaChars":-125"#));

    let deserialized: WritingStatsSummaryDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, deserialized);
}

#[test]
fn test_device_stats_record_dto_serialization() {
    let dto = DeviceStatsRecordDto {
        device_id: "dev-123".to_string(),
        platform: PlatformDto::Desktop,
        device_class: "laptop".to_string(),
        human_typed_chars: 50,
        pasted_chars: 0,
        deleted_chars: 0,
        ai_inserted_chars: 0,
        net_delta_chars: 50,
        active_seconds: 600,
        sessions_count: 1,
    };

    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains(r#""platform":"Desktop""#));

    let deserialized: DeviceStatsRecordDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, deserialized);
}

#[test]
fn test_speed_curve_point_dto_edge_cases() {
    let dto = SpeedCurvePointDto {
        start_ms: -1000,
        end_ms: 0,
        chars_typed: 0,
        chars_per_minute: 0.0,
    };

    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains(r#""startMs":-1000"#));

    let deserialized: SpeedCurvePointDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, deserialized);
}
