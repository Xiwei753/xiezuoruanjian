// Project / Volume / Chapter / Tree / WritingStats DTOs.
// Issue #753：字段与 Core api/types/ 真实 DTO 一字不差对齐（camelCase）。
// Rust `Option<T>` 且未标 skip_serializing_if 的字段一定以 null 出现
// （如 ChapterMetaDto.note）→ 写成 `T | null`，不写成可选属性。

// ─── Project DTOs ───

export interface Project {
  id: string
  title: string
  createdAt: string
  updatedAt: string
}

// 作品列表元素 — Core ProjectSummaryDto（元数据 + 统计一次返回）。
export interface ProjectSummary {
  id: string
  title: string
  createdAt: string
  updatedAt: string
  totalWordCount: number
  volumeCount: number
  chapterCount: number
}

export interface ProjectStats {
  totalWordCount: number
  volumeCount: number
  chapterCount: number
}

// ─── Volume DTOs ───

export interface Volume {
  id: string
  title: string
  createdAt: string
  updatedAt: string
  order: number
}

// ─── Chapter DTOs ───

export interface ChapterMeta {
  id: string
  title: string
  createdAt: string
  updatedAt: string
  order: number
  wordCount: number
  hash: string
  note: string | null
}

export interface ChapterContent {
  meta: ChapterMeta
  content: string
}

export interface ChapterSaveReceipt {
  chapterRelativePath: string
  contentLen: number
  contentHash: string
  metaHash: string
  updatedAt: string
  wordCount: number
}

// ─── 作品树（Core ProjectWorkspaceSnapshotDto / VolumeWithChaptersDto） ───

export interface ProjectTree {
  project: Project
  volumes: VolumeTree[]
  stats: ProjectStats
}

export interface VolumeTree {
  volume: Volume
  chapters: ChapterMeta[]
}

// ─── Writing Stats（Core api/types/stats.rs，camelCase） ───

export interface DateRange {
  startDate: string
  endDate: string
}

// WritingStatsSummaryDto
export interface WritingStats {
  range: DateRange
  totalHumanTypedChars: number
  totalPastedChars: number
  totalDeletedChars: number
  totalAiInsertedChars: number
  totalNetDeltaChars: number
  totalActiveSeconds: number
  totalSessions: number
  daysCount: number
}
