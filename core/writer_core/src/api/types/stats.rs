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
/// - `history_scope`：历史口径说明，取 `"localEvents"` / `"syncedDevices"` /
///   `"partialSync"`：事件历史来自本机分区与已同步设备分区按 `event_id` 去重的
///   合并结果，缺失或同步不完整时如实标注，不宣称为全量。
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

/// 统计页 Explorer 一次性快照（#944）：生涯摘要 + 四张时段卡 + 当前选中时段详情。
///
/// 宽屏总览、窄屏单页都从这同一个 DTO 取数，端侧只做格式化与绘制，
/// 不再自行统计天数、纪录或速度。
///
/// - `history_scope`：历史口径，取 `"localEvents"` / `"syncedDevices"` /
///   `"partialSync"`（本机 + 已同步设备事件分区合并去重的真实覆盖范围）。
/// - `lifetime`：全应用生涯摘要。
/// - `period_cards`：近七天 / 近三十天 / 近半年 / 自选区间四张卡，顺序固定。
/// - `selected`：当前选中时段（可以是生涯或任一预设/自选区间）的汇总、纪录与趋势。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsExplorerDto {
    pub history_scope: String,
    pub lifetime: WritingStatsLifetimeDto,
    pub period_cards: Vec<WritingStatsPeriodCardDto>,
    pub selected: WritingStatsSelectedPeriodDto,
}

/// 全应用生涯摘要。
///
/// 「陪伴」口径与「实际写作」口径分开：`elapsed_calendar_days` 是从首次
/// 写作到今天（含两端）的自然日历天数，不是 `writing_days`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsLifetimeDto {
    /// 累计人工输入字符数（含后来被删掉/重写的字；不含粘贴、AI、同步、撤销）。
    pub total_human_typed_chars: u64,
    /// 首次有人工输入的本地业务日；没有任何输入时为 null。
    pub first_writing_date: Option<String>,
    /// 最近一次有人工输入的本地业务日；没有任何输入时为 null。
    pub last_writing_date: Option<String>,
    /// 首次写作日到今天（含两端）的自然日历天数；没有历史时为 0。
    pub elapsed_calendar_days: u64,
    /// 实际有人工输入的不同本地业务日数量。
    pub writing_days: u64,
    /// 平均每天字数 = 累计人工输入 / 首次写作日到今天（含两端）的自然日历
    /// 天数（同 `elapsed_calendar_days`，不排除休息日）；完全空生涯时为 null。
    pub average_chars_per_day: Option<f64>,
    /// 平均写作速度（字/分）= 累计人工输入 / 有效写作时长；零时长时为 null。
    pub average_chars_per_minute: Option<f64>,
}

/// 预设时段卡（近七天 / 近三十天 / 近半年 / 自选区间）。
///
/// `preset` 取值固定为 `"last7Days"`、`"last30Days"`、`"last6Months"`、
/// `"custom"`。自选区间尚未选择时 `range` 为 null、`has_selection` 为 false、
/// 数值为 0 / null；其余三张卡 `has_selection` 恒为 true
/// （没有数据时范围照算：数值为 0、区间日均字数为 0、字/分为 null）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsPeriodCardDto {
    pub preset: String,
    pub has_selection: bool,
    pub range: Option<DateRangeDto>,
    pub total_human_typed_chars: u64,
    pub total_active_seconds: u64,
    pub writing_days: u64,
    /// 平均每天字数 = 区间人工输入 / 该区间的自然日历天数（含无输入日、
    /// 不排除休息日）；区间有效但没有输入时为 0，自选区间未选择时为 null。
    pub average_chars_per_day: Option<f64>,
    /// 平均写作速度（字/分）= 区间人工输入 / 区间有效写作时长；零时长时为 null。
    pub average_chars_per_minute: Option<f64>,
}

/// 当前选中时段的完整详情：区间汇总 + 纪录 + 趋势。
///
/// `preset` 额外允许 `"lifetime"`（生涯逐月趋势）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsSelectedPeriodDto {
    pub preset: String,
    pub range: DateRangeDto,
    pub total_human_typed_chars: u64,
    pub total_active_seconds: u64,
    pub writing_days: u64,
    /// 平均每天字数 = 区间人工输入 / 该区间的自然日历天数（含无输入日、
    /// 不排除休息日）；区间有效但没有输入时为 0。
    pub average_chars_per_day: Option<f64>,
    /// 平均写作速度（字/分）= 区间人工输入 / 区间有效写作时长；零时长时为 null。
    pub average_chars_per_minute: Option<f64>,
    pub records: WritingStatsRecordsDto,
    pub trend: WritingStatsTrendDto,
}

/// 单条纪录。没有事实依据时字段为 null，不用 0 冒充纪录。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsRecordDto {
    /// 纪录所在日期（连续写作日期段为起始日）。
    pub date: Option<String>,
    /// 仅连续写作日期段使用：结束日期；其他纪录为 null。
    pub end_date: Option<String>,
    /// 纪录值，单位随所在字段不同（见各字段说明）；无纪录时为 null。
    pub value: Option<u64>,
}

/// 时段纪录集合。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsRecordsDto {
    /// 最多码字日：`date` 为日期，`value` 为当天人工输入字数；并列取更早日期。
    pub best_day: WritingStatsRecordDto,
    /// 最快码字：连续 60 分钟半开窗口 `[start, start+60min)` 内的最高人工输入量；
    /// `date` 为窗口起始业务日，`value` 即窗口内字数（显示为字/时）。
    pub fastest_hour: WritingStatsRecordDto,
    /// 最长写作会话：沿用现有 `SESSION_GAP_MS` 会话定义（按设备独立分组，不把
    /// 不同设备并发事件接成一条）；`date` 为会话开始业务日，`value` 为时长秒数。
    /// 只有时长大于 0 的会话才算有效样本；没有时为 null。
    pub longest_session: WritingStatsRecordDto,
    /// 最长连续有输入日期段：`date` / `end_date` 为起止日期，`value` 为连续天数；
    /// 并列时取更早的一段。
    pub longest_streak: WritingStatsRecordDto,
}

/// 折线趋势：按业务日期聚合的人工输入字数点列。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsTrendDto {
    /// 聚合粒度：`"day"` / `"week"` / `"month"`。
    pub granularity: String,
    /// 日期升序的点列；无数据的日期/周/月补 0。
    pub points: Vec<WritingStatsTrendPointDto>,
}

/// 趋势点。`date` 是聚合桶起始日期：按天为该日、按周为该周周一、
/// 按月为该月 1 号；`human_typed_chars` 为桶内人工输入字数。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsTrendPointDto {
    pub date: String,
    pub human_typed_chars: u64,
}

/// 每日码字日历（#956）：一个公历年的逐日人工输入真值。
///
/// - `days`：该公历年的全部自然日（365/366 日），日期升序，每天只出现一次；
/// - `status = "known"`：已确认覆盖的日期，`human_typed_chars` 是真实值（可为真实 0）；
/// - `status = "unknown"`：历史覆盖不完整或统计同步失败，不得当成「确实没写」；
/// - `status = "future"`：未来日期，尚未到来；
/// - `intensity` 由 Core 从该年正数日的字数分布分级（0–4），端侧不重算人工字数；
/// - `human_typed_chars` 保留原值，点格子时显示实际数值（unknown 日期仅在确认为 0 时才画实心 0 档）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsHeatmapDayDto {
    /// 公历日期（`YYYY-MM-DD`，本地业务日）。
    pub date: String,
    /// 当天人工输入字符数。
    pub human_typed_chars: u64,
    /// 显示强度分级：0（无输入）到 4（该年最多）。
    pub intensity: u8,
    /// 覆盖状态：`known` / `unknown` / `future`（见类型文档）。
    pub status: String,
}

/// 每日码字日历快照（#956）。一次查询返回指定年份的完整自然日列表。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsHeatmapDto {
    /// 查询的公历年。
    pub year: i32,
    /// 该年第一天（`YYYY-01-01`）。
    pub start_date: String,
    /// 该年最后一天（`YYYY-12-31`）。
    pub end_date: String,
    /// 本份快照的历史覆盖范围，与 Explorer 同源的三值：
    /// `localEvents` / `syncedDevices` / `partialSync`，供端侧标注数据范围。
    pub history_scope: String,
    pub days: Vec<WritingStatsHeatmapDayDto>,
}
