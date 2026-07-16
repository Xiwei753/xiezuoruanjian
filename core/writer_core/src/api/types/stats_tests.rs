#[cfg(test)]
mod tests {
    use crate::api::types::stats::*;
    use crate::api::types::platform::PlatformDto;

    #[test]
    fn test_date_range_dto_serialization() {
        let dto = DateRangeDto {
            start_date: "2023-01-01".to_string(),
            end_date: "2023-01-31".to_string(),
        };

        let json = serde_json::to_string(&dto).unwrap();
        let expected = r#"{"startDate":"2023-01-01","endDate":"2023-01-31"}"#;
        assert_eq!(json, expected);

        let deserialized: DateRangeDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, dto);
    }

    #[test]
    fn test_writing_stats_summary_dto_serialization() {
        let dto = WritingStatsSummaryDto {
            range: DateRangeDto {
                start_date: "2023-01-01".to_string(),
                end_date: "2023-01-01".to_string(),
            },
            total_human_typed_chars: 100,
            total_pasted_chars: 50,
            total_deleted_chars: 10,
            total_ai_inserted_chars: 20,
            total_net_delta_chars: 160,
            total_active_seconds: 3600,
            total_sessions: 2,
            days_count: 1,
        };

        let json = serde_json::to_string(&dto).unwrap();
        let expected = r#"{"range":{"startDate":"2023-01-01","endDate":"2023-01-01"},"totalHumanTypedChars":100,"totalPastedChars":50,"totalDeletedChars":10,"totalAiInsertedChars":20,"totalNetDeltaChars":160,"totalActiveSeconds":3600,"totalSessions":2,"daysCount":1}"#;
        assert_eq!(json, expected);

        let deserialized: WritingStatsSummaryDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, dto);
    }

    #[test]
    fn test_device_stats_record_dto_serialization() {
        let dto = DeviceStatsRecordDto {
            device_id: "dev-123".to_string(),
            platform: PlatformDto::Desktop,
            device_class: "desktop".to_string(),
            human_typed_chars: 500,
            pasted_chars: 100,
            deleted_chars: 50,
            ai_inserted_chars: 0,
            net_delta_chars: 550,
            active_seconds: 7200,
            sessions_count: 3,
        };

        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""platform":"Desktop""#));

        let deserialized: DeviceStatsRecordDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, dto);
    }
}
