// StatsFormat — 统计页共享常量与纯格式化工具（Issue #945）。
//
// 页面只负责把 Core Explorer 快照转成人类可读文本：千分位、人类可读日期、
// 时长（时/分）、字/时、字/天。null 一律渲染成占位符 "—"，
// 不用 0 顶替「没有样本」——纪录字段为 null 就是没有纪录。
//
// 本文件不做任何统计口径计算：天数、纪录、平均速度全部来自 Core。
// EMPTY_STATS_EXPLORER 只是子组件 @Prop 的声明默认值（首帧构造用），
// 不是业务兜底数据：页面只在拿到真实快照后才渲染统计正文。

import { WritingStatsExplorer, WritingStatsRecord, WritingStatsRecords, WritingStatsTrend } from '../../../corebridge/dto/StatsDtos'

// ─── 宽屏展开面板状态（StatsScreen 持有，宽/窄内容共同消费） ───

export const STATS_PANEL_NONE: string = 'none'
export const STATS_PANEL_RANGE: string = 'range'
export const STATS_PANEL_LIFETIME: string = 'lifetime'

const EMPTY_RECORD: WritingStatsRecord = { date: null, endDate: null, value: null }
const EMPTY_RECORDS: WritingStatsRecords = {
  bestDay: EMPTY_RECORD,
  fastestHour: EMPTY_RECORD,
  longestSession: EMPTY_RECORD,
  longestStreak: EMPTY_RECORD
}
const EMPTY_TREND: WritingStatsTrend = { granularity: 'day', points: [] }

export const EMPTY_STATS_EXPLORER: WritingStatsExplorer = {
  historyScope: 'localEvents',
  lifetime: {
    totalHumanTypedChars: 0,
    firstWritingDate: null,
    lastWritingDate: null,
    elapsedCalendarDays: 0,
    writingDays: 0,
    averageCharsPerDay: null,
    timedHumanTypedChars: 0,
    measuredWritingSeconds: 0,
    averageCharsPerHour: null,
    hasMeasuredTiming: false
  },
  periodCards: [],
  selected: {
    preset: 'last7Days',
    range: { startDate: '', endDate: '' },
    totalHumanTypedChars: 0,
    totalActiveSeconds: 0,
    writingDays: 0,
    averageCharsPerDay: null,
    timedHumanTypedChars: 0,
    measuredWritingSeconds: 0,
    averageCharsPerHour: null,
    hasMeasuredTiming: false,
    records: EMPTY_RECORDS,
    trend: EMPTY_TREND
  },
  contributions: null
}

/** 千分位整数。29456 → "29,456"。 */
export function formatInt(value: number): string {
  const rounded: number = Math.round(value)
  const negative: boolean = rounded < 0
  const digits: string = `${Math.abs(rounded)}`
  let grouped: string = ''
  let count: number = 0
  for (let i = digits.length - 1; i >= 0; i--) {
    grouped = digits.charAt(i) + grouped
    count++
    if (count % 3 === 0 && i > 0) {
      grouped = `,${grouped}`
    }
  }
  return negative ? `-${grouped}` : grouped
}

/** 平均值：最多一位小数，整数不带 ".0"；null → "—"。 */
export function formatAverage(value: number | null): string {
  if (value === null) {
    return '—'
  }
  const roundedToTenth: number = Math.round(value * 10) / 10
  if (Math.abs(roundedToTenth - Math.round(roundedToTenth)) < 0.05) {
    return formatInt(Math.round(roundedToTenth))
  }
  return roundedToTenth.toFixed(1)
}

/** Core 已按真实计时样本返回字/时；null 保持 "—"，不把空数据当成 0。 */
export function formatAveragePerHour(value: number | null): string {
  return formatAverage(value)
}

/** 真实计时秒数 → 「X 时 Y 分」；无样本返回 "—"。 */
export function formatMeasuredDuration(seconds: number): string {
  const totalMinutes: number = Math.round(seconds / 60)
  if (totalMinutes <= 0) {
    return '0 分'
  }
  const hours: number = Math.floor(totalMinutes / 60)
  const minutes: number = totalMinutes % 60
  return hours > 0 ? `${hours} 时 ${minutes} 分` : `${minutes} 分`
}

/** "2026-09-13" → "2026/09/13"；null/空串 → "—"。 */
export function formatDateHuman(date: string | null): string {
  if (date === null || date.length === 0) {
    return '—'
  }
  return date.split('-').join('/')
}

/** 日期段：起止都取不到时返回 "—"；只有一端时原样显示该端。 */
export function formatDateRangeHuman(start: string | null, end: string | null): string {
  const hasStart: boolean = start !== null && start.length > 0
  const hasEnd: boolean = end !== null && end.length > 0
  if (hasStart && hasEnd) {
    return `${formatDateHuman(start)} - ${formatDateHuman(end)}`
  }
  if (hasStart) {
    return formatDateHuman(start)
  }
  if (hasEnd) {
    return formatDateHuman(end)
  }
  return '—'
}

/** 时长（秒 → 时/分/秒）。null / 0 → "—"，不足 1 分钟按秒显示。 */
export function formatDurationHuman(seconds: number | null): string {
  if (seconds === null || seconds <= 0) {
    return '—'
  }
  if (seconds < 60) {
    return `${seconds} 秒`
  }
  const totalMinutes: number = Math.round(seconds / 60)
  const hours: number = Math.floor(totalMinutes / 60)
  const minutes: number = totalMinutes % 60
  if (hours > 0 && minutes > 0) {
    return `${hours} 时 ${minutes} 分`
  }
  if (hours > 0) {
    return `${hours} 时`
  }
  return `${minutes} 分`
}

/** 纪录值：null → "—"；否则千分位 + 单位。 */
export function formatRecordValue(value: number | null, unit: string): string {
  if (value === null) {
    return '—'
  }
  return `${formatInt(value)} ${unit}`
}

/** 历史口径标注：Core 返回 localEvents / syncedDevices / partialSync。未知值返回空串。 */
export function formatHistoryScope(scope: string): string {
  if (scope === 'syncedDevices') {
    return '已同步设备记录'
  }
  if (scope === 'partialSync') {
    return '同步不完整'
  }
  if (scope === 'localEvents') {
    return '本机记录'
  }
  return ''
}

function pad2(value: number): string {
  return value < 10 ? `0${value}` : `${value}`
}

/** Date → Core 业务日 "YYYY-MM-DD"（用户所选日期原样透传，不做时区改写）。 */
export function formatIsoDate(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`
}

/** Core 业务日 "YYYY-MM-DD" → 本地 Date；格式不合法返回 null。 */
export function parseIsoDate(text: string): Date | null {
  if (text.length !== 10) {
    return null
  }
  const parts: string[] = text.split('-')
  if (parts.length !== 3) {
    return null
  }
  const year: number = parseInt(parts[0], 10)
  const month: number = parseInt(parts[1], 10)
  const day: number = parseInt(parts[2], 10)
  if (Number.isNaN(year) || Number.isNaN(month) || Number.isNaN(day)) {
    return null
  }
  if (month < 1 || month > 12 || day < 1 || day > 31) {
    return null
  }
  return new Date(year, month - 1, day)
}

/** 折线图横轴日期刻度：按月显示 "2026/10"，按天/周显示 "10/12"。 */
export function formatAxisDate(date: string, granularity: string): string {
  const parts: string[] = date.split('-')
  if (parts.length !== 3) {
    return date
  }
  if (granularity === 'month') {
    return `${parts[0]}/${parts[1]}`
  }
  return `${parts[1]}/${parts[2]}`
}
