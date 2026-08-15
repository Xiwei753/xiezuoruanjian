// Project / Volume / Chapter / Tree / WritingStats DTOs.
// 字段与 Core api/types/ 真实 DTO 对齐（camelCase）。

// ─── Project DTOs ───

export interface Project {
  id: string
  title: string
  createdAt: string
  updatedAt: string
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
  note?: string
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

// ─── Tree Composition (客户端组合结构，非 Core DTO) ───

export interface ProjectTree {
  project: Project
  volumes: VolumeTree[]
  stats?: ProjectStats
}

export interface VolumeTree {
  volume: Volume
  chapters: ChapterMeta[]
}

// ─── Writing Stats (待 Core stats 模块稳定后对齐) ───

export interface WritingStats {
  totalChars: number
  totalWords: number
  totalChapters: number
  todayChars: number
  todayDuration: number
  averageSpeed: number
}
