// StatsDtos — 统计页 Explorer DTO（Issue #944/#945）。
// 字段与 Core `api/types/stats.rs` 的 WritingStatsExplorerDto 系列一字不差对齐（camelCase）。
//
// 与 ProjectDtos.ets 的关系：旧的 WritingStatsSummaryDto 系列（近 30 天汇总 / 速度曲线）
// 继续留在 ProjectDtos.ets；本文件只声明 Explorer 快照这一套，不重复声明同一结构。
//
// Rust `Option<T>` 且未标 skip_serializing_if 的字段一定以 null 出现
// （如 firstWritingDate / records.*.value）→ 写成 `T | null`，不写成可选属性；
// null 是无值，不是 0。

import { DateRange } from './ProjectDtos'

// ─── preset 固定字符串（Core WritingStatsPeriodCardDto / Selected 的 preset 取值） ───

export const STATS_PRESET_LAST_7_DAYS: string = 'last7Days'
export const STATS_PRESET_LAST_30_DAYS: string = 'last30Days'
export const STATS_PRESET_LAST_6_MONTHS: string = 'last6Months'
export const STATS_PRESET_LIFETIME: string = 'lifetime'
export const STATS_PRESET_CUSTOM: string = 'custom'

// ─── WritingStatsLifetimeDto：全应用生涯摘要 ───

export interface WritingStatsLifetime {
  totalHumanTypedChars: number
  firstWritingDate: string | null
  lastWritingDate: string | null
  /** 首次写作日到今天（含两端）的自然日历天数；没有历史时为 0 */
  elapsedCalendarDays: number
  writingDays: number
  averageCharsPerDay: number | null
  /** 有真实计时样本覆盖的人工输入字数 */
  timedHumanTypedChars: number
  /** 真实前台写作计时秒数；没有计时样本时为 0 */
  measuredWritingSeconds: number
  /** 仅用真实计时覆盖的字数与秒数计算；没有分母时 null */
  averageCharsPerHour: number | null
  /** 是否存在可用的真实计时样本 */
  hasMeasuredTiming: boolean
}

// ─── WritingStatsPeriodCardDto：四张预设时段卡 ───

export interface WritingStatsPeriodCard {
  preset: string
  hasSelection: boolean
  range: DateRange | null
  totalHumanTypedChars: number
  totalActiveSeconds: number
  writingDays: number
  averageCharsPerDay: number | null
  timedHumanTypedChars: number
  measuredWritingSeconds: number
  averageCharsPerHour: number | null
  hasMeasuredTiming: boolean
}

// ─── WritingStatsRecordDto / WritingStatsRecordsDto：时段纪录 ───

export interface WritingStatsRecord {
  /** 纪录所在日期（连续写作日期段为起始日）；无纪录为 null */
  date: string | null
  /** 仅连续写作日期段使用：结束日期；其他纪录为 null */
  endDate: string | null
  /** 纪录值，单位随字段不同；无纪录为 null */
  value: number | null
}

export interface WritingStatsRecords {
  /** 最多码字日：value = 当天人工输入字数 */
  bestDay: WritingStatsRecord
  /** 单小时最多码字：value = 连续 60 分钟窗口内字数（单位字，不是持续字/时） */
  fastestHour: WritingStatsRecord
  /** 最长写作会话：value = 时长秒数 */
  longestSession: WritingStatsRecord
  /** 最长连续有输入日期段：date/endDate = 起止日期，value = 连续天数 */
  longestStreak: WritingStatsRecord
}

// ─── WritingStatsTrendDto：折线趋势（端侧只画线，不重算） ───

export interface WritingStatsTrendPoint {
  /** 聚合桶起始日期：按天为该日、按周为该周周一、按月为该月 1 号 */
  date: string
  humanTypedChars: number
}

export interface WritingStatsTrend {
  /** "day" / "week" / "month" */
  granularity: string
  points: WritingStatsTrendPoint[]
}

// ─── WritingStatsSelectedPeriodDto：当前选中时段详情 ───

export interface WritingStatsSelectedPeriod {
  /** 额外允许 "lifetime"（生涯逐月趋势） */
  preset: string
  range: DateRange
  totalHumanTypedChars: number
  totalActiveSeconds: number
  writingDays: number
  averageCharsPerDay: number | null
  timedHumanTypedChars: number
  measuredWritingSeconds: number
  averageCharsPerHour: number | null
  hasMeasuredTiming: boolean
  records: WritingStatsRecords
  trend: WritingStatsTrend
}

// ─── WritingStatsExplorerDto：统计页一次性快照 ───

export interface WritingStatsExplorer {
  /** 历史口径："localEvents"（本机）/ "syncedDevices"（已同步设备）/ "partialSync"（同步不完整） */
  historyScope: string
  lifetime: WritingStatsLifetime
  /** 近七天 / 近三十天 / 近半年 / 自选区间，顺序固定 */
  periodCards: WritingStatsPeriodCard[]
  selected: WritingStatsSelectedPeriod
}

// ─── WritingActivitySegmentInputDto：平台端已测量完成的前台写作切片 ───

/** 与 Core `WritingActivitySegmentInputDto` 对齐；设备身份由 Core 补齐。 */
export interface WritingActivitySegmentInput {
  eventId: string
  platform: string
  projectId: string
  volumeId: string
  chapterId: string
  sessionId: string
  startAtMs: number
  endAtMs: number
  pauseReason: string
}

// ─── WritingStatsHeatmapDto：每日码字日历（Issue #956） ───
//
// 与 Core `api/types/stats.rs` 的 WritingStatsHeatmapDto 系列一字不差对齐。
// 每个真实公历日一项，按日期升序，全年 365/366 项。

export interface WritingStatsHeatmapDay {
  /** 本地日历日 YYYY-MM-DD（按日期升序、每天唯一） */
  date: string
  /** 当天人工输入字数真值；仅 status = known 的已过日期为真实 0 */
  humanTypedChars: number
  /** Core 按当年正数日分布统一分级：0–4（0 表示没有人工输入） */
  intensity: number
  /** 覆盖状态："known"（已确认覆盖，可为真实 0）/ "unknown"（覆盖不完整或同步失败，不是没写作）/ "future"（未来日期未到） */
  status: string
}

export interface WritingStatsHeatmap {
  /** 公历年 */
  year: number
  /** 该年第一天 YYYY-MM-DD */
  startDate: string
  /** 该年最后一天 YYYY-MM-DD */
  endDate: string
  /** 历史口径：同 Explorer 的三值（本机 / 已同步设备 / 同步不完整），端侧据此标注范围 */
  historyScope: string
  days: WritingStatsHeatmapDay[]
}
