// ⚠️ PROVISIONAL — 本文件是鸿蒙端临时 DTO，尚未与 WriterCoreApi 真实 DTO 对齐。
// 下一步必须用 Core 实际导出的类型替换，不得自行发明字段。
// 带 @speculative 标注的字段/接口为推测性占位，可能不存在于 Core。

export interface Workspace {
  path: string
  name: string
  isValid: boolean
  /** @speculative — Core 可能不暴露锁状态 */
  lockState?: 'unlocked' | 'locked' | 'unknown'
  /** @speculative — Core 可能不在此处暴露同步摘要 */
  syncConfig?: SyncConfigSummary
  /** @speculative */
  lastAccessedAt?: string
}

export interface WorkspaceState {
  workspace: Workspace
  projects: Project[]
  recentEdits: RecentEdit[]
  /** @speculative — Core 可能不在此处暴露同步状态 */
  syncState?: SyncState
}

export interface RecentEdit {
  projectId: string
  projectTitle: string
  chapterId: string
  chapterTitle: string
  /** @speculative */
  volumeId?: string
  editedAt: string
  /** @speculative */
  wordCount?: number
}

export interface Project {
  id: string
  name: string
  volumeCount: number
  chapterCount: number
  /** @speculative — Core 可能不在此层级暴露总字数 */
  totalWordCount?: number
  createdAt: string
  updatedAt: string
  /** @speculative */
  description?: string
  /** @speculative */
  coverImage?: string
}

export interface Volume {
  id: string
  projectId: string
  name: string
  order: number
  chapterCount: number
  /** @speculative */
  totalWordCount?: number
  /** @speculative */
  createdAt?: string
  /** @speculative */
  updatedAt?: string
}

export interface Chapter {
  id: string
  volumeId: string
  name: string
  wordCount: number
  order: number
  updatedAt: string
  /** @speculative */
  createdAt?: string
  /** @speculative — Core 当前无 draft/published/archived 状态概念 */
  status?: 'draft' | 'published' | 'archived'
}

export interface ChapterData {
  id: string
  title: string
  content: string
  wordCount: number
  volumeId: string
  projectId: string
  updatedAt: string
  /** @speculative */
  createdAt?: string
  /** @speculative */
  status?: 'draft' | 'published' | 'archived'
  /** @speculative — Core 可能不暴露章节元数据 */
  metadata?: ChapterMetadata
}

/** @speculative — Core 可能不定义此结构 */
export interface ChapterMetadata {
  author?: string
  tags?: string[]
  notes?: string
  customFields?: Record<string, string>
}

export interface SaveReceipt {
  success: boolean
  wordCount: number
  savedAt: string
  errorCode?: string
  errorMessage?: string
  /** @speculative */
  changedPaths?: string[]
}

// ─── StarMap DTOs ─── 过渡模型，禁止扩展。
// 等 Core 的 StarMap 合同稳定后整段替换为 typed DTO。
// sourceId/targetId → EndpointPath；parentId/childIds → ChildMapPlacement。

export interface StarMapMeta {
  id: string
  title: string
  description: string
  nodeCount: number
  edgeCount: number
  /** @speculative */
  projectId?: string
  createdAt: string
  updatedAt: string
  /** @speculative */
  layoutType?: 'force' | 'tree' | 'radial' | 'manual'
}

export interface StarMapGraph {
  id: string
  title: string
  description: string
  nodes: StarMapNode[]
  edges: StarMapEdge[]
  /** @speculative — Core 可能不暴露 embeds */
  embeds: StarMapEmbed[]
  /** @speculative — Core 可能不暴露 links */
  links: StarMapLink[]
  /** @speculative */
  viewport?: StarMapViewport
  /** @speculative */
  layout?: StarMapLayout
}

export interface StarMapNode {
  id: string
  label: string
  /** @speculative */
  description?: string
  x: number
  y: number
  /** @speculative */
  width?: number
  /** @speculative */
  height?: number
  type: NodeType
  /** @speculative */
  color?: string
  /** @speculative */
  icon?: string
  /** @speculative */
  metadata?: Record<string, string>
  // TODO: Replace with EndpointPath when Core stabilizes
  /** @speculative */
  parentId?: string
  /** @speculative */
  childIds?: string[]
}

export type NodeType = 'character' | 'location' | 'event' | 'theme' | 'plot' | 'item' | 'organization' | 'custom'

export interface StarMapEdge {
  id: string
  // TODO: Replace with EndpointPath model when Core stabilizes
  sourceId: string
  targetId: string
  /** @speculative */
  label?: string
  type: EdgeType
  /** @speculative */
  color?: string
  /** @speculative */
  style?: 'solid' | 'dashed' | 'dotted'
  /** @speculative */
  metadata?: Record<string, string>
}

export type EdgeType = 'relation' | 'dependency' | 'timeline' | 'conflict' | 'alliance' | 'custom'

/** @speculative — Core 可能不暴露嵌入引用 */
export interface StarMapEmbed {
  id: string
  nodeId: string
  chapterId: string
  offset: number
  length: number
  text: string
  context?: string
}

/** @speculative — Core 可能不暴露跨图链接 */
export interface StarMapLink {
  id: string
  sourceNodeId: string
  targetStarmapId: string
  label?: string
  description?: string
}

/** @speculative */
export interface StarMapViewport {
  centerX: number
  centerY: number
  zoom: number
  rotation: number
}

/** @speculative */
export interface StarMapLayout {
  type: 'force' | 'tree' | 'radial' | 'manual'
  params?: Record<string, number>
  seed?: number
}

// ⚠️ DO NOT EXPAND StarMap DTOs below until Core's StarMap contracts are finalized.
// These types are transitional mock-only placeholders.
// sourceId/targetId will be replaced by EndpointPath; parentId/childIds will be replaced by ChildMapPlacement.
// Hyperlink and ChildMapPlacement DTOs are blocked on Core stabilization.

// TODO: Add Hyperlink DTO when Core stabilizes
// export interface StarMapHyperlink { ... }

// TODO: Add ChildMapPlacement DTO when Core stabilizes
// export interface ChildMapPlacement { ... }

export interface LocalSettings {
  fontSize: number
  lineHeight: number
  fontFamily: string
  theme: ThemeMode
  autoSave: boolean
  autoSaveInterval: number
  autoIndent: boolean
  showWordCount: boolean
  /** @speculative — Core 可能不暴露此项 */
  showLineNumbers: boolean
  /** @speculative */
  wordWrap: boolean
  /** @speculative */
  spellCheck: boolean
  /** @speculative */
  lastWorkspacePath?: string
  /** @speculative */
  lastProjectId?: string
  /** @speculative */
  lastChapterId?: string
  /** @speculative */
  editorWidth?: number
  /** @speculative */
  uiLanguage?: string
}

export type ThemeMode = 'light' | 'dark' | 'system'

/** @speculative — 同步设置尚未在 Core 中实现 */
export interface SyncableSettings {
  fontSize?: number
  lineHeight?: number
  fontFamily?: string
  theme?: ThemeMode
  autoIndent?: boolean
  showWordCount?: boolean
  showLineNumbers?: boolean
  wordWrap?: boolean
  uiLanguage?: string
}

/** @speculative — 同步设置尚未在 Core 中实现 */
export interface EffectiveSettings extends LocalSettings {
  syncableOverrides?: Partial<SyncableSettings>
  isSynced: boolean
}

export interface ResultEnvelope<T> {
  success: boolean
  data?: T
  errorCode?: ErrorCode
  userMessage?: string
  /** @speculative */
  rawError?: string
  /** @speculative */
  warnings?: string[]
  /** @speculative */
  changedPaths?: string[]
  /** @speculative */
  changedEntities?: string[]
  /** @speculative */
  timestamp?: number
}

export interface ProjectTree {
  project: Project
  volumes: VolumeTree[]
  /** @speculative — Core 可能不在此处暴露统计 */
  stats?: ProjectStats
}

export interface VolumeTree {
  volume: Volume
  chapters: Chapter[]
}

/** @speculative — 统计接口待 Core 确认 */
export interface ProjectStats {
  totalChapters: number
  totalVolumes: number
  totalWordCount: number
  averageChapterLength: number
  lastUpdatedAt: string
}

export interface WritingStats {
  totalChars: number
  totalWords: number
  totalChapters: number
  /** @speculative */
  totalProjects?: number
  todayChars: number
  todayDuration: number
  /** @speculative */
  todaySessions?: number
  averageSpeed: number
  /** @speculative */
  longestStreak?: number
  /** @speculative */
  currentStreak?: number
  /** @speculative */
  weeklyStats?: DailyStats[]
}

/** @speculative */
export interface DailyStats {
  date: string
  charsAdded: number
  durationSeconds: number
  sessionsCount: number
}

// ─── Sync DTOs ─── 全部 @speculative，同步功能尚未在 Core 中实现。

/** @speculative */
export interface SyncConfig {
  enabled: boolean
  provider: SyncProvider
  remoteUrl?: string
  branch?: string
  autoSync: boolean
  autoSyncInterval?: number
  conflictStrategy: ConflictStrategy
  lastSyncAt?: string
  syncPath?: string
}

/** @speculative */
export type SyncProvider = 'github' | 'gitlab' | 'gitea' | 'custom' | 'none'
/** @speculative */
export type ConflictStrategy = 'keep_local' | 'keep_remote' | 'manual' | 'auto_merge'

/** @speculative */
export interface SyncConfigSummary {
  enabled: boolean
  provider: SyncProvider
  lastSyncAt?: string
  status: SyncStatus
}

/** @speculative */
export type SyncStatus = 'idle' | 'syncing' | 'success' | 'failed' | 'conflict' | 'error'

/** @speculative */
export interface SyncState {
  status: SyncStatus
  lastSyncAt?: string
  lastError?: string
  pendingChanges?: number
  conflictFiles?: string[]
}

/** @speculative */
export interface SyncReport {
  filesToAdd: string[]
  filesToModify: string[]
  filesToDelete: string[]
  conflicts: string[]
  warnings: string[]
  estimatedDuration?: number
}

/** @speculative */
export interface SyncDiagnostics {
  isGitRepository: boolean
  hasRemote: boolean
  remoteUrl?: string
  branch?: string
  isClean: boolean
  lastCommit?: string
  lastCommitDate?: string
  errors: string[]
  warnings: string[]
}

/** @speculative */
export interface SyncResult {
  success: boolean
  filesAdded: number
  filesModified: number
  filesDeleted: number
  conflicts: string[]
  duration: number
  error?: string
}

export type ErrorCode =
  | 'WORKSPACE_NOT_FOUND'
  | 'WORKSPACE_LOCKED'
  | 'WORKSPACE_INVALID'
  | 'WORKSPACE_ALREADY_EXISTS'
  | 'PROJECT_NOT_FOUND'
  | 'PROJECT_ALREADY_EXISTS'
  | 'VOLUME_NOT_FOUND'
  | 'VOLUME_ALREADY_EXISTS'
  | 'CHAPTER_NOT_FOUND'
  | 'CHAPTER_ALREADY_EXISTS'
  | 'EMPTY_OVERWRITE_BLOCKED'
  | 'CONTENT_TOO_LARGE'
  | 'SYNC_MERGE_CONFLICT'
  | 'SYNC_AUTH_ERROR'
  | 'SYNC_NETWORK_ERROR'
  | 'SYNC_REMOTE_ERROR'
  | 'PERMISSION_DENIED'
  | 'IO_ERROR'
  | 'IO_READ_ERROR'
  | 'IO_WRITE_ERROR'
  | 'STARMAP_NOT_FOUND'
  | 'STARMAP_ALREADY_EXISTS'
  | 'NODE_NOT_FOUND'
  | 'EDGE_NOT_FOUND'
  | 'SETTINGS_NOT_FOUND'
  | 'SETTINGS_INVALID'
  | 'NOT_IMPLEMENTED'
  | 'NATIVE_BRIDGE_NOT_LOADED'
  | 'INVALID_ARGUMENT'
  | 'OPERATION_CANCELLED'
  | 'TIMEOUT'
  | 'UNKNOWN_ERROR'
  | 'PARSE_ERROR'
  | 'NATIVE_ERROR'

// ========== 布局策略类型（Core layout_policy） ==========

export enum FoldPosture {
  Unknown = 'Unknown',
  FullyOpened = 'FullyOpened',
  HalfOpened = 'HalfOpened',
  Closed = 'Closed'
}

export enum Orientation {
  Unknown = 'Unknown',
  Portrait = 'Portrait',
  Landscape = 'Landscape'
}

export enum PointerKind {
  Unknown = 'Unknown',
  Touch = 'Touch',
  Stylus = 'Stylus',
  Mouse = 'Mouse'
}

export enum WidthClass {
  Compact = 'Compact',
  Medium = 'Medium',
  Expanded = 'Expanded'
}

export enum HeightClass {
  Compact = 'Compact',
  Medium = 'Medium',
  Expanded = 'Expanded'
}

export enum ShellMode {
  SinglePane = 'SinglePane',
  SupportingPane = 'SupportingPane',
  TwoPane = 'TwoPane'
}

export enum EditorMode {
  FullWidth = 'FullWidth',
  CenteredPaper = 'CenteredPaper'
}

export enum NavigationMode {
  Stack = 'Stack',
  ListDetail = 'ListDetail'
}

export interface WindowMetrics {
  widthVp: number
  heightVp: number
  safeTopVp: number
  safeBottomVp: number
  keyboardVisible: boolean
  foldPosture: FoldPosture
  orientation: Orientation
  pointer: PointerKind
}

export interface LayoutPlan {
  widthClass: WidthClass
  heightClass: HeightClass
  shellMode: ShellMode
  editorMode: EditorMode
  navigationMode: NavigationMode
  contentMaxWidthVp: number
  pagePaddingVp: number
  gridColumns: number
  showSidePanel: boolean
  showBottomBar: boolean
}
