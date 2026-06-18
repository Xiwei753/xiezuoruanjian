// 本文件定义鸿蒙端 DTO，字段与 Core api/types/ 真实 DTO 对齐。
// Core 使用 serde(rename_all = "camelCase")，JSON 输出为 camelCase。
// 如需新增字段，必须先在 Core api/types/ 中定义，不得自行发明。

// ─── Workspace DTOs ───

export interface RecentEdit {
  projectId: string
  volumeId: string
  chapterId: string
  timestamp: string
}

export interface WorkspaceSummary {
  path: string
  isValid: boolean
  projects: Project[]
  recentEdits: RecentEdit[]
}

export interface WorkspaceDiagnostics {
  hasWorkspace: boolean
  workspacePath: string
  coreInitialized: boolean
  pathExists: boolean
  isDir: boolean
  manifestPath: string
  manifestExists: boolean
  projectsPath: string
  projectsDirExists: boolean
  appMetaExists: boolean
  writable: boolean
  writableError: string
  validateWorkspace: boolean
  treeCount: number
  lastWorkspacePath: string
  createProjectAvailable: boolean
}

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

// ─── Settings DTOs ───

export interface LocalSettings {
  themeMode?: string
  locale?: string
  autoSaveEnabled: boolean
  editorFontSize: number
  editorLineSpacingMultiplier: number
  windowWidth: number
  windowHeight: number
  autoSaveDelayMs: number
  autoIndentEnabled: boolean
  autoIndentWidth: number
  editorTypingAnimationEnabled: boolean
  editorSmoothCursorEnabled: boolean
  editorTypingAnimationDurationMs: number
  editorSmoothCursorDurationMs: number
  aiEnabled: boolean
  statsDeviceId?: string
  desktopSidebarWidth: number
  desktopEditorWidth: number
}

export interface SyncableSettings {
  fontSize: number
  themeMode: string
  monetColor: string
}

export type ThemeMode = 'light' | 'dark' | 'system'

// ─── Sync DTOs ───

export interface SyncConfig {
  enabled: boolean
  backendType: string
  remoteUrl: string
  transport: string
  branch: string
  autoSync: boolean
  syncIntervalSeconds: number
  proxyEnabled: boolean
  proxyType: string
  proxyHost: string
  proxyPort: number
  username: string
  androidHasInternetPermission: boolean
  androidHasAccessNetworkStatePermission: boolean
}

export interface SyncState {
  status: string
  remoteUrl?: string
  backendType?: string
  transport?: string
  lastSyncedCommit?: string
  lastSyncTime?: number
  lastError?: string
  lastSuccessfulNetworkMode?: string
  conflicts?: SyncConflict[]
}

export interface SyncConflict {
  localPath: string
  remotePath: string
  localHash: string
  remoteHash: string
  baseHash: string
  createdAt: number
  description: string
}

export interface SyncResult {
  status: string
  uploadedFiles: string[]
  downloadedFiles: string[]
  localDeletes: string[]
  remoteDeletes: string[]
  overwrittenFiles: string[]
  ignoredFiles: string[]
  conflicts: SyncConflict[]
  commitHash?: string
  error?: string
  errorCategory?: string
  firstSyncMode: string
  userMessage?: string
  chosenNetworkMode?: string
  networkProbeSummary?: NetworkProbeResult[]
}

export interface NetworkProbeResult {
  // 待 Core NetworkProbeResultDto 字段稳定后补充
  [key: string]: string | number | boolean
}

export interface SyncDiagnosticsResult {
  success: boolean
  backendType: string
  androidHasInternetPermission: boolean
  androidHasAccessNetworkStatePermission: boolean
  androidNetworkState: string
  tcpProbeOk: boolean
  tcpProbeStatus: string
  httpConnectProbeOk: boolean
  httpConnectProbeStatus: string
  libgit2ProbeOk: boolean
  libgit2ProbeStatus: string
  networkOk: boolean
  authOk: boolean
  repoOk: boolean
  branchOk: boolean
  networkStatus: string
  authStatus: string
  repoStatus: string
  branchStatus: string
  remoteUrlSanitized: string
  transport: string
  errorCategory: string
  userMessage: string
  rawError?: string
  chosenNetworkMode?: string
  networkProbeSummary?: NetworkProbeResult[]
}

// ─── Envelope DTOs ───

export interface ResultEnvelope<T> {
  success: boolean
  data?: T
  errorCode?: string
  userMessage?: string
  rawError?: string
  warnings: string[]
  changedPaths: string[]
  changedEntities: ChangedEntity[]
}

export interface ChangedEntity {
  entityType: string
  entityId?: string
}

// ─── Error Codes ───
// 与 Core WriterError 错误码对齐

export type ErrorCode =
  | 'IO_ERROR'
  | 'JSON_ERROR'
  | 'INVALID_WORKSPACE'
  | 'PROJECT_NOT_FOUND'
  | 'VOLUME_NOT_FOUND'
  | 'CHAPTER_NOT_FOUND'
  | 'EMPTY_OVERWRITE_BLOCKED'
  | 'NOT_IMPLEMENTED'
  | 'REFUSE_DELETE_WORKSPACE_ROOT'
  | 'INVALID_DELETE_TARGET'
  | 'SYNC_CONFLICT'
  | 'SYNC_FAILED'
  | 'OTHER'

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

// ─── StarMap DTOs ─── 过渡模型，禁止扩展。
// 等 Core 的 StarMap 合同稳定后整段替换为 typed DTO。
// sourceId/targetId → EndpointPath；parentId/childIds → ChildMapPlacement。

export interface StarMapMeta {
  id: string
  title: string
  description: string
  nodeCount: number
  edgeCount: number
  createdAt: string
  updatedAt: string
}

export interface StarMapGraph {
  schemaVersion: number
  id: string
  starmapId: string
  title: string
  nodes: StarMapNode[]
  edges: StarMapEdge[]
  embeds: StarMapEmbed[]
  links: StarMapLink[]
  layout?: StarMapLayout
  createdAt: number
  updatedAt: number
}

export interface StarMapNode {
  id: string
  title: string
  kind: NodeType
  payload?: Record<string, Object>
  tags?: string[]
  description?: string
  color?: string
  icon?: string
  metadata?: Record<string, string>
  parentId?: string
  childIds?: string[]
}

export type NodeType = 'Character' | 'Event' | 'Location' | 'Item' | 'Concept' | 'Theme' | 'Note' | 'Organization' | 'Timeline' | 'Plot' | 'Foreshadowing' | 'Chapter' | 'Custom'

export interface StarMapEdge {
  id: string
  from?: string
  to?: string
  kind: EdgeKind
  label?: string
  payload?: Record<string, Object>
  createdAt: number
  updatedAt: number
}

export type EdgeKind = 'Contains' | 'References' | 'AppearsIn' | 'Causes' | 'RelatedTo' | 'LocatedAt' | 'CharacterRelation' | 'Timeline' | 'Foreshadows' | 'Resolves' | 'DependsOn' | 'ConflictsWith' | 'Custom'

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
  scale: number
  offsetX: number
  offsetY: number
  width: number
  height: number
}

export interface StarMapLayout {
  kind: 'Freeform' | 'AutoRadial' | 'Custom'
  nodes: StarMapLayoutNode[]
}

export interface StarMapLayoutNode {
  nodeId: string
  x: number
  y: number
  width: number
  height: number
  radius: number
  collapsed: boolean
  zIndex: number
  scale: number
  depth: number
  focusWeight: number
  orbitGroup?: string
}

export interface StarMapEdgeRender {
  edgeId: string
  startX: number
  startY: number
  endX: number
  endY: number
  arrowTipX: number
  arrowTipY: number
  arrowLeftX: number
  arrowLeftY: number
  arrowRightX: number
  arrowRightY: number
  labelX?: number
  labelY?: number
}

export interface StarMapMotionPolicy {
  enabled: boolean
  idleWobbleEnabled: boolean
  idleAmplitudeVp: number
  idlePeriodMs: number
  dragLiftScale: number
  dragShadowBoost: number
  settleDurationMs: number
  reduceMotion: boolean
}

// DO NOT EXPAND StarMap DTOs until Core's StarMap contracts are finalized.

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
  sidePanelWidthVp: number
  primaryPaneWeight: number
  detailPanelMaxWidthVp: number
}

// ========== 页面策略类型（Core screen_policy） ==========

export enum ScreenRole {
  Home = 'Home',
  Workspace = 'Workspace',
  Writing = 'Writing',
  Settings = 'Settings',
  Sync = 'Sync'
}

export enum PaneRole {
  PrimaryList = 'PrimaryList',
  Detail = 'Detail',
  Editor = 'Editor',
  Inspector = 'Inspector',
  Drawer = 'Drawer'
}

export enum ActionRole {
  Back = 'Back',
  Save = 'Save',
  CreateProject = 'CreateProject',
  CreateVolume = 'CreateVolume',
  CreateChapter = 'CreateChapter',
  Delete = 'Delete',
  Rename = 'Rename',
  Settings = 'Settings',
  Sync = 'Sync',
  Search = 'Search'
}

export enum ActionPlacement {
  TopLeading = 'TopLeading',
  TopTrailing = 'TopTrailing',
  Floating = 'Floating',
  BottomBar = 'BottomBar',
  ContextMenu = 'ContextMenu',
  SidePanel = 'SidePanel'
}

export interface ActionSlot {
  actionId: string
  role: ActionRole
  placement: ActionPlacement
  visibleIn: ShellMode[]
  requiresConfirmation: boolean
}

export interface ScreenPolicy {
  screenRole: ScreenRole
  actionSlots: ActionSlot[]
}
