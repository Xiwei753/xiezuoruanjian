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

/// 平台端上报一次写作事件的入参 DTO。
///
/// `writer_core_process_writing_event` 直接反序列化本类型，不再手写字段表，
/// 于是入参形状和 `WritingStatsSummaryDto` 一样只由 Core 单侧定义。
/// 缺字段沿用历史默认值：platform 缺省 `desktop`，其余空串/0。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WritingEventInputDto {
    pub device_id: String,
    pub platform: String,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub old_text: String,
    pub new_text: String,
    pub duration_seconds: u32,
    pub session_id: String,
}

impl Default for WritingEventInputDto {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            platform: "desktop".to_string(),
            project_id: String::new(),
            volume_id: String::new(),
            chapter_id: String::new(),
            old_text: String::new(),
            new_text: String::new(),
            duration_seconds: 0,
            session_id: String::new(),
        }
    }
}
