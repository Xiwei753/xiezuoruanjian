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

/// 「当前写作速度」——以调用时刻为终点的实时速度。
///
/// 和 [`SpeedCurveSummaryDto`] 分开：速度曲线是历史分桶，拿它的最后一个桶
/// 当实时速度会在用户停笔后一直挂着停笔前的非零值（桶只生成到最后一个事件，
/// 不补当前这一分钟的 0 桶）。本 DTO 由 Core 以「最近 N 秒」为窗口重算，
/// 停笔超过一个窗口后自然回落到 0。
///
/// 字段与 `writing_stats::store::CurrentWritingSpeed` 一一对应。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentWritingSpeedDto {
    /// 窗口长度（秒）。`charsPerMinute` 按这个窗口折算。
    pub window_seconds: u32,
    /// 采样时刻（Unix 毫秒）。
    pub sampled_at_ms: i64,
    /// 窗口内累计的纯输入字符数。
    pub chars_typed: u32,
    /// 窗口内纯输入速度（字符/分钟）。
    pub chars_per_minute: f32,
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
