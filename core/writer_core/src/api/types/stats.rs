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

/// 单作品写作档案（#934）。
///
/// - `created_at`：作品创建时间（RFC3339，来自 `project.json`）；
/// - `first_written_at`：该作品第一条有人工输入字符的编辑事件时间（RFC3339），
///   作品还没有任何人工输入时为 null；
/// - `current_word_count`：现有章节字数之和（正文存量，不是历史累计）；
/// - `total_human_typed_chars`：本地事件历史里的人工输入总量（含后来删掉重写的）；
/// - `best_writing_day` / `best_writing_day_chars`：本地业务日中单日人工输入
///   最多的一天及其字数；并列取更早日期；无输入时为 null / 0；
/// - `writing_days`：有人工输入的不同本地业务日数量；
/// - `history_scope`：历史口径说明，当前固定 `"localEvents"`
///   （`events.local` 不参与跨设备同步，只代表本机可读历史）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWritingLifetimeStatsDto {
    pub project_id: String,
    pub created_at: String,
    pub first_written_at: Option<String>,
    pub current_word_count: u64,
    pub total_human_typed_chars: u64,
    pub best_writing_day: Option<String>,
    pub best_writing_day_chars: u64,
    pub writing_days: u64,
    pub history_scope: String,
}

/// 统计面板一次性快照（#935）：今日 / 本周 / 本月 + 本周排行 + 速度曲线。
///
/// - `range`：排行与速度曲线所在的统计口径（本周，周一起始）；
/// - `today` / `week` / `month`：按 Core 本地业务日历切分的汇总；
/// - `projects` / `chapters` / `devices` / `speed_curve`：本周范围的排行与曲线。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsDashboardDto {
    pub range: DateRangeDto,
    pub today: WritingStatsSummaryDto,
    pub week: WritingStatsSummaryDto,
    pub month: WritingStatsSummaryDto,
    pub projects: Vec<ProjectStatsRecordDto>,
    pub chapters: Vec<ChapterStatsRecordDto>,
    pub devices: Vec<DeviceStatsRecordDto>,
    pub speed_curve: SpeedCurveSummaryDto,
}
