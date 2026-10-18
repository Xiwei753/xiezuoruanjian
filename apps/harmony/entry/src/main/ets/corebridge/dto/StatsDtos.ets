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
  averageCharsPerMinute: number | null
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
  averageCharsPerMinute: number | null
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
  /** 最快码字：value = 连续 60 分钟窗口内字数（显示为字/时） */
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
  averageCharsPerMinute: number | null
  records: WritingStatsRecords
  trend: WritingStatsTrend
}

// ─── WritingStatsExplorerDto：统计页一次性快照 ───

export interface WritingStatsExplorer {
  /** 历史口径，当前固定 "localEvents"（本机记录） */
  historyScope: string
  lifetime: WritingStatsLifetime
  /** 近七天 / 近三十天 / 近半年 / 自选区间，顺序固定 */
  periodCards: WritingStatsPeriodCard[]
  selected: WritingStatsSelectedPeriod
}
