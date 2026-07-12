use super::*;
// WritingStats DTOs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DateRangeDto {
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsSummaryDto {
    pub range: DateRangeDto,
    pub total_human_typed_chars: u64,
    pub total_pasted_chars: u64,
    pub total_deleted_chars: u64,
    pub total_ai_inserted_chars: u64,
    pub total_net_delta_chars: i64,
    pub total_active_seconds: u64,
    pub total_sessions: u32,
    pub days_count: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatsRecordDto {
    pub project_id: String,
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatsSummaryDto {
    pub range: DateRangeDto,
    pub projects: Vec<ProjectStatsRecordDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterStatsRecordDto {
    pub chapter_id: String,
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterStatsSummaryDto {
    pub range: DateRangeDto,
    pub chapters: Vec<ChapterStatsRecordDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatsRecordDto {
    pub device_id: String,
    pub platform: PlatformDto,
    pub device_class: String,
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
    pub sessions_count: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatsSummaryDto {
    pub range: DateRangeDto,
    pub devices: Vec<DeviceStatsRecordDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeedCurvePointDto {
    pub start_ms: i64,
    pub end_ms: i64,
    pub chars_typed: u32,
    pub chars_per_minute: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeedCurveSummaryDto {
    pub range: DateRangeDto,
    pub bucket_minutes: u32,
    pub buckets: Vec<SpeedCurvePointDto>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::PlatformDto;

    #[test]
    fn test_date_range_dto_serialization() {
        let dto = DateRangeDto {
            start_date: "2023-01-01".to_string(),
            end_date: "2023-01-31".to_string(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""startDate":"2023-01-01""#));
        assert!(json.contains(r#""endDate":"2023-01-31""#));

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
            total_human_typed_chars: 1000,
            total_pasted_chars: 500,
            total_deleted_chars: 200,
            total_ai_inserted_chars: 300,
            total_net_delta_chars: 1600,
            total_active_seconds: 7200,
            total_sessions: 10,
            days_count: 5,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""totalHumanTypedChars":1000"#));
        assert!(json.contains(r#""totalPastedChars":500"#));

        let deserialized: WritingStatsSummaryDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, deserialized);
    }

    #[test]
    fn test_device_stats_summary_dto_serialization() {
        let dto = DeviceStatsSummaryDto {
            range: DateRangeDto {
                start_date: "2023-01-01".to_string(),
                end_date: "2023-01-31".to_string(),
            },
            devices: vec![DeviceStatsRecordDto {
                device_id: "device1".to_string(),
                platform: PlatformDto::Desktop,
                device_class: "pc".to_string(),
                human_typed_chars: 100,
                pasted_chars: 50,
                deleted_chars: 10,
                ai_inserted_chars: 0,
                net_delta_chars: 140,
                active_seconds: 3600,
                sessions_count: 5,
            }],
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""startDate":"2023-01-01""#));
        assert!(json.contains(r#""deviceId":"device1""#));
        assert!(json.contains(r#""platform":"Desktop""#));

        let deserialized: DeviceStatsSummaryDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, deserialized);
    }

    #[test]
    fn test_speed_curve_summary_dto_serialization() {
        let dto = SpeedCurveSummaryDto {
            range: DateRangeDto {
                start_date: "2023-01-01".to_string(),
                end_date: "2023-01-01".to_string(),
            },
            bucket_minutes: 5,
            buckets: vec![
                SpeedCurvePointDto {
                    start_ms: 0,
                    end_ms: 300000,
                    chars_typed: 150,
                    chars_per_minute: 30.0,
                }
            ],
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""bucketMinutes":5"#));
        assert!(json.contains(r#""charsTyped":150"#));

        let deserialized: SpeedCurveSummaryDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, deserialized);
    }
}
