use super::*;
use serde_json::json;

#[test]
fn test_date_range_dto_serialization() {
    let dto = DateRangeDto {
        start_date: "2023-10-01".to_string(),
        end_date: "2023-10-31".to_string(),
    };

    let serialized = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        serialized,
        json!({
            "startDate": "2023-10-01",
            "endDate": "2023-10-31"
        })
    );

    let deserialized: DateRangeDto = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_project_stats_record_dto_serialization() {
    let dto = ProjectStatsRecordDto {
        project_id: "proj-123".to_string(),
        human_typed_chars: 1000,
        pasted_chars: 200,
        deleted_chars: 50,
        ai_inserted_chars: 300,
        net_delta_chars: 1450,
        active_seconds: 3600,
    };

    let serialized = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        serialized,
        json!({
            "projectId": "proj-123",
            "humanTypedChars": 1000,
            "pastedChars": 200,
            "deletedChars": 50,
            "aiInsertedChars": 300,
            "netDeltaChars": 1450,
            "activeSeconds": 3600
        })
    );

    let deserialized: ProjectStatsRecordDto = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized, dto);
}
