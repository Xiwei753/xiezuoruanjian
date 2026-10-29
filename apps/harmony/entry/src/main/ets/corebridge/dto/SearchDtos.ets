// Global search DTOs — 与 Core SearchResultPage / SearchResult / SearchTarget 对齐。
// Core 序列化为 camelCase；summaryMatchRanges 与 byteStart/byteEnd 都是 UTF-8 字节偏移，
// 平台在真实原文上转换后使用。分页游标由 Core 生成，平台只原样透传。

export type SearchScope =
  | 'chapterBody'
  | 'chapterTitle'
  | 'chapterNote'
  | 'projectTitle'
  | 'volumeTitle'
  | 'starmapTitle'
  | 'starmapNode'
  | 'starmapEdgeLabel'
  | 'starmapLink'
  | 'starmapEmbed'
  | 'annotationTag'
  | 'annotationEntry'
  | 'annotationLink'
  | 'setting'
  | 'all'

export type SearchField = 'title' | 'body'

export type SearchOriginScreen =
  | 'general'
  | 'home'
  | 'project'
  | 'writing'
  | 'starmap'
  | 'settings'

/** 发起搜索时的页面上下文。只影响排序，不过滤其他范围。 */
export interface SearchContext {
  originScreen: SearchOriginScreen
  currentProjectId?: string
  currentVolumeId?: string
  currentChapterId?: string
  currentStarmapId?: string
  currentSettingsSection?: string
}

/** 命中的可导航目标。未使用/不存在的字段为 null。 */
export interface SearchTarget {
  projectId: string | null
  volumeId: string | null
  chapterId: string | null
  starmapId: string | null
  nodeId: string | null
  edgeId: string | null
  linkId: string | null
  embedId: string | null
  settingKey: string | null
  settingSection: string | null
}

export interface SearchResult {
  objectId: string
  title: string
  path: string
  summary: string
  byteStart: number
  byteEnd: number
  summaryMatchRanges: number[][]
  field: SearchField
  score: number
  scope: SearchScope
  target: SearchTarget
}

export interface SearchResultPage {
  items: SearchResult[]
  nextCursor: string | null
}
