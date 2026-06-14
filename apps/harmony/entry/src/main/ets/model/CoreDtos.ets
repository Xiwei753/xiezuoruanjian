export interface Workspace {
  path: string
  name: string
  isValid: boolean
  lockState?: 'unlocked' | 'locked' | 'unknown'
  syncConfig?: SyncConfigSummary
  lastAccessedAt?: string
}

export interface WorkspaceState {
  workspace: Workspace
  projects: Project[]
  recentEdits: RecentEdit[]
  syncState?: SyncState
}

export interface RecentEdit {
  projectId: string
  projectTitle: string
  chapterId: string
  chapterTitle: string
  volumeId?: string
  editedAt: string
  wordCount?: number
}

export interface Project {
  id: string
  name: string
  volumeCount: number
  chapterCount: number
  totalWordCount?: number
  createdAt: string
  updatedAt: string
  description?: string
  coverImage?: string
}

export interface Volume {
  id: string
  projectId: string
  name: string
  order: number
  chapterCount: number
  totalWordCount?: number
  createdAt?: string
  updatedAt?: string
}

export interface Chapter {
  id: string
  volumeId: string
  name: string
  wordCount: number
  order: number
  updatedAt: string
  createdAt?: string
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
  createdAt?: string
  status?: 'draft' | 'published' | 'archived'
  metadata?: ChapterMetadata
}

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
  changedPaths?: string[]
}

export interface StarMapMeta {
  id: string
  title: string
  description: string
  nodeCount: number
  edgeCount: number
  projectId?: string
  createdAt: string
  updatedAt: string
  layoutType?: 'force' | 'tree' | 'radial' | 'manual'
}

export interface StarMapGraph {
  id: string
  title: string
  description: string
  nodes: StarMapNode[]
  edges: StarMapEdge[]
  embeds: StarMapEmbed[]
  links: StarMapLink[]
  viewport?: StarMapViewport
  layout?: StarMapLayout
}

export interface StarMapNode {
  id: string
  label: string
  description?: string
  x: number
  y: number
  width?: number
  height?: number
  type: NodeType
  color?: string
  icon?: string
  metadata?: Record<string, string>
  // TODO: Replace with EndpointPath when Core stabilizes
  parentId?: string
  childIds?: string[]
}

export type NodeType = 'character' | 'location' | 'event' | 'theme' | 'plot' | 'item' | 'organization' | 'custom'

export interface StarMapEdge {
  id: string
  // TODO: Replace with EndpointPath model when Core stabilizes
  sourceId: string
  targetId: string
  label?: string
  type: EdgeType
  color?: string
  style?: 'solid' | 'dashed' | 'dotted'
  metadata?: Record<string, string>
}

export type EdgeType = 'relation' | 'dependency' | 'timeline' | 'conflict' | 'alliance' | 'custom'

export interface StarMapEmbed {
  id: string
  nodeId: string
  chapterId: string
  offset: number
  length: number
  text: string
  context?: string
}

export interface StarMapLink {
  id: string
  sourceNodeId: string
  targetStarmapId: string
  label?: string
  description?: string
}

export interface StarMapViewport {
  centerX: number
  centerY: number
  zoom: number
  rotation: number
}

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
  showLineNumbers: boolean
  wordWrap: boolean
  spellCheck: boolean
  lastWorkspacePath?: string
  lastProjectId?: string
  lastChapterId?: string
  editorWidth?: number
  uiLanguage?: string
}

export type ThemeMode = 'light' | 'dark' | 'system'

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

export interface EffectiveSettings extends LocalSettings {
  syncableOverrides?: Partial<SyncableSettings>
  isSynced: boolean
}

export interface ResultEnvelope<T> {
  success: boolean
  data?: T
  errorCode?: ErrorCode
  userMessage?: string
  rawError?: string
  warnings?: string[]
  changedPaths?: string[]
  changedEntities?: string[]
  timestamp?: number
}

export interface ProjectTree {
  project: Project
  volumes: VolumeTree[]
  stats?: ProjectStats
}

export interface VolumeTree {
  volume: Volume
  chapters: Chapter[]
}

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
  totalProjects?: number
  todayChars: number
  todayDuration: number
  todaySessions?: number
  averageSpeed: number
  longestStreak?: number
  currentStreak?: number
  weeklyStats?: DailyStats[]
}

export interface DailyStats {
  date: string
  charsAdded: number
  durationSeconds: number
  sessionsCount: number
}

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

export type SyncProvider = 'github' | 'gitlab' | 'gitea' | 'custom' | 'none'
export type ConflictStrategy = 'keep_local' | 'keep_remote' | 'manual' | 'auto_merge'

export interface SyncConfigSummary {
  enabled: boolean
  provider: SyncProvider
  lastSyncAt?: string
  status: SyncStatus
}

export type SyncStatus = 'idle' | 'syncing' | 'success' | 'failed' | 'conflict' | 'error'

export interface SyncState {
  status: SyncStatus
  lastSyncAt?: string
  lastError?: string
  pendingChanges?: number
  conflictFiles?: string[]
}

export interface SyncReport {
  filesToAdd: string[]
  filesToModify: string[]
  filesToDelete: string[]
  conflicts: string[]
  warnings: string[]
  estimatedDuration?: number
}

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
