export interface Workspace {
  path: string
  name: string
  isValid: boolean
}

export interface WorkspaceState {
  workspace: Workspace
  projects: Project[]
  recentEdits: RecentEdit[]
}

export interface RecentEdit {
  projectId: string
  projectTitle: string
  chapterId: string
  chapterTitle: string
  editedAt: string
}

export interface Project {
  id: string
  name: string
  volumeCount: number
  chapterCount: number
  createdAt: string
  updatedAt: string
}

export interface Volume {
  id: string
  projectId: string
  name: string
  order: number
  chapterCount: number
}

export interface Chapter {
  id: string
  volumeId: string
  name: string
  wordCount: number
  order: number
  updatedAt: string
}

export interface ChapterData {
  id: string
  title: string
  content: string
  wordCount: number
  volumeId: string
  projectId: string
  updatedAt: string
}

export interface SaveReceipt {
  success: boolean
  wordCount: number
  savedAt: string
  errorCode?: string
  errorMessage?: string
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
}

export interface StarMapGraph {
  id: string
  title: string
  description: string
  nodes: StarMapNode[]
  edges: StarMapEdge[]
  embeds: StarMapEmbed[]
  links: StarMapLink[]
}

export interface StarMapNode {
  id: string
  label: string
  description?: string
  x: number
  y: number
  type: 'character' | 'location' | 'event' | 'theme' | 'plot' | 'custom'
  color?: string
  metadata?: Record<string, string>
}

export interface StarMapEdge {
  id: string
  sourceId: string
  targetId: string
  label?: string
  type: 'relation' | 'dependency' | 'timeline' | 'custom'
  color?: string
}

export interface StarMapEmbed {
  id: string
  nodeId: string
  chapterId: string
  offset: number
  length: number
  text: string
}

export interface StarMapLink {
  id: string
  sourceNodeId: string
  targetStarmapId: string
  label?: string
}

export interface LocalSettings {
  fontSize: number
  lineHeight: number
  fontFamily: string
  theme: 'light' | 'dark' | 'system'
  autoSave: boolean
  autoSaveInterval: number
  autoIndent: boolean
  showWordCount: boolean
  lastWorkspacePath?: string
}

export interface ResultEnvelope<T> {
  success: boolean
  data?: T
  errorCode?: string
  userMessage?: string
  rawError?: string
  warnings?: string[]
  changedPaths?: string[]
  changedEntities?: string[]
}

export interface ProjectTree {
  project: Project
  volumes: VolumeTree[]
}

export interface VolumeTree {
  volume: Volume
  chapters: Chapter[]
}

export interface WritingStats {
  totalChars: number
  totalWords: number
  totalChapters: number
  todayChars: number
  todayDuration: number
  averageSpeed: number
}

export type ErrorCode =
  | 'WORKSPACE_NOT_FOUND'
  | 'WORKSPACE_LOCKED'
  | 'WORKSPACE_INVALID'
  | 'PROJECT_NOT_FOUND'
  | 'VOLUME_NOT_FOUND'
  | 'CHAPTER_NOT_FOUND'
  | 'EMPTY_OVERWRITE_BLOCKED'
  | 'SYNC_MERGE_CONFLICT'
  | 'PERMISSION_DENIED'
  | 'IO_ERROR'
  | 'STARMAP_NOT_FOUND'
  | 'NODE_NOT_FOUND'
  | 'EDGE_NOT_FOUND'
  | 'UNKNOWN_ERROR'
