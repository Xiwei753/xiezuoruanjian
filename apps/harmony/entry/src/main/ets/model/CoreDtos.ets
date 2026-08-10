// 本文件定义鸿蒙端 DTO，字段与 Core api/types/ 真实 DTO 对齐。
// Core struct 字段使用 serde(rename_all = "camelCase")，JSON 输出为 camelCase。
// Core enum 变体使用 PascalCase（如 "Back"、"TopLeading"、"SinglePane"），与 Rust 枚举名一致。
// 如需新增字段，必须先在 Core api/types/ 中定义，不得自行发明。

// ─── App State DTOs ───

export interface RecentEdit {
  projectId: string
  volumeId: string
  chapterId: string
  timestamp: string
}

export interface AppSummary {
  projects: Project[]
  recentEdits: RecentEdit[]
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
  appearanceMode: string
  colorSource: string
  dynamicColorEnabled: boolean
  selectedBuiltinThemeId: string
  selectedPaletteId: string
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
  linuxQtSidebarWidth: number
  linuxQtEditorWidth: number
}

export interface SyncableSettings {
  fontSize: number
  themeMode: string
  monetColor: string
  themePaletteJson: string
}

export type ThemeMode = 'light' | 'dark' | 'system'

// ─── Theme Palette DTOs ───

export interface ThemeColorSchemeDto {
  primary: string
  on_primary: string
  primary_container: string
  on_primary_container: string
  inverse_primary: string
  secondary: string
  on_secondary: string
  secondary_container: string
  on_secondary_container: string
  tertiary: string
  on_tertiary: string
  tertiary_container: string
  on_tertiary_container: string
  background: string
  on_background: string
  surface: string
  on_surface: string
  surface_variant: string
  on_surface_variant: string
  surface_tint: string
  surface_dim: string
  surface_bright: string
  surface_container_lowest: string
  surface_container_low: string
  surface_container: string
  surface_container_high: string
  surface_container_highest: string
  inverse_surface: string
  inverse_on_surface: string
  error: string
  on_error: string
  error_container: string
  on_error_container: string
  outline: string
  outline_variant: string
  scrim: string
}

export interface ThemePaletteRecordDto {
  schema_version: number
  palette_id: string
  palette_fingerprint: string
  source: string
  source_platform: string
  source_device_id: string
  source_device_class: string
  captured_at_ms: number
  variant: string
  light_scheme: ThemeColorSchemeDto
  dark_scheme: ThemeColorSchemeDto
}

export interface BuiltinThemeDto {
  theme_id: string
  name: string
  light_scheme: ThemeColorSchemeDto
  dark_scheme: ThemeColorSchemeDto
}

// ─── Sync DTOs ───

export interface SyncConfig {
  enabled: boolean
  backendType: string
  remoteUrl: string
  transport: string
  branch: string
  autoSync: boolean
  syncIntervalSeconds: number
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
  userMessage?: string
  rawError?: string
  chosenNetworkMode?: string
  networkProbeSummary?: string
}

// ─── Envelope DTOs ───

export interface ResultEnvelope<T> {
  success: boolean
  data?: T
  errorCode?: string
  messageKey?: string
  messageArgs?: Record<string, string>
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
  | 'PROJECT_NOT_FOUND'
  | 'VOLUME_NOT_FOUND'
  | 'CHAPTER_NOT_FOUND'
  | 'STARMAP_NOT_FOUND'
  | 'EMPTY_OVERWRITE_BLOCKED'
  | 'NOT_IMPLEMENTED'
  | 'REFUSE_DELETE_ROOT'
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

// ─── Editor Animation DTOs ─── 与 Core api/types/editor.rs 对齐
// 鸿蒙端动画暂未实现，DTO 定义先行对齐，为后续接入做准备。

export enum EditorAnimationKind {
  Insert = 'Insert',
  Delete = 'Delete',
  Cursor = 'Cursor'
}

export enum AnimationMode {
  GlyphAnimation = 'GlyphAnimation',
  ClusterAnimation = 'ClusterAnimation',
  RunAnimation = 'RunAnimation',
  LineReflowAnimation = 'LineReflowAnimation',
  SnapshotAnimation = 'SnapshotAnimation',
  SystemSuppressed = 'SystemSuppressed'
}

export enum EditorTransactionCause {
  Typing = 'Typing',
  Delete = 'Delete',
  ImeComposition = 'ImeComposition',
  TypingCommit = 'TypingCommit',
  Paste = 'Paste',
  Undo = 'Undo',
  Redo = 'Redo',
  Load = 'Load',
  Format = 'Format',
  Programmatic = 'Programmatic'
}

export enum VisualCoordinateMode {
  Baseline = 'Baseline'
}

export interface CursorRect {
  x: number
  top: number
  bottom: number
  baselineY: number
}

export interface GlyphRect {
  x: number
  y: number
  w: number
  h: number
  char: string
  baselineY: number
  byteStart: number
  byteEnd: number
}

export interface EditorVisualTransaction {
  id: number
  kind: EditorAnimationKind
  cause: EditorTransactionCause
  oldText: string
  newText: string
  oldSelectionAnchor: number
  oldSelectionHead: number
  newSelectionAnchor: number
  newSelectionHead: number
  insertedRangeStart: number
  insertedRangeEnd: number
  animationMode: AnimationMode
  durationMs: number
  coordinateMode: VisualCoordinateMode
}

// ========== 布局契约类型（Core presentation layout_contract，#610） ==========

// Harmony 平台自身的折叠姿态类型（平台输入，不在 Core 契约）。
export enum FoldPosture {
  Unknown = 'Unknown',
  FullyOpened = 'FullyOpened',
  HalfOpened = 'HalfOpened',
  Closed = 'Closed'
}

// Harmony 平台自身的大小分类（平台决策，不在 Core 契约）。
export enum WidthClass {
  Compact = 'Compact',
  Medium = 'Medium',
  Expanded = 'Expanded'
}

export enum ShellMode {
  SinglePane = 'SinglePane',
  SupportingPane = 'SupportingPane',
  TwoPane = 'TwoPane',
  ThreePane = 'ThreePane'
}

export enum WorkspacePaneMode {
  SinglePane = 'SinglePane',
  ListDetail = 'ListDetail',
  ThreePane = 'ThreePane'
}

export interface VisiblePaneRoles {
  showProjectList: boolean
  showChapterTree: boolean
  showEditor: boolean
  showSupporting: boolean
}

export enum PointerClass {
  Unknown = 'Unknown',
  Touch = 'Touch',
  Stylus = 'Stylus',
  Mouse = 'Mouse'
}

// 窗口能力 — 平台端已经根据本平台窗口系统判断好的输入（#610）。
export interface WindowCapabilities {
  availablePaneCount: number
  hasSeparatingFold: boolean
  pointerClass: PointerClass
  keyboardVisible: boolean
}

// 布局契约 — Core resolve_layout 输出（不含 Material 断点 / dp / 导航控件名）。
export interface LayoutContract {
  shellMode: ShellMode
  workspacePaneMode: WorkspacePaneMode
  visiblePaneRoles: VisiblePaneRoles
  showPrimaryNavigation: boolean
}

// Harmony 平台在 Core 契约之上叠加自己的平台值后供页面消费。
export interface LayoutPlan {
  shellMode: ShellMode
  workspacePaneMode: WorkspacePaneMode
  visiblePaneRoles: VisiblePaneRoles
  showPrimaryNavigation: boolean
  contentMaxWidthVp: number
  pagePaddingVp: number
  gridColumns: number
}

// ========== 页面策略类型（Core presentation screen_contract，#610） ==========

export enum ScreenRole {
  Home = 'Home',
  ProjectList = 'ProjectList',
  ProjectWorkspace = 'ProjectWorkspace',
  Writing = 'Writing',
  StarMap = 'StarMap',
  Stats = 'Stats',
  Settings = 'Settings',
  Sync = 'Sync'
}

export enum PaneRole {
  PrimaryList = 'PrimaryList',
  Detail = 'Detail',
  Editor = 'Editor',
  Inspector = 'Inspector',
  Drawer = 'Drawer',
  Supporting = 'Supporting'
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
  Search = 'Search',
  Sort = 'Sort'
}

export enum ActionRegion {
  HeaderLeading = 'HeaderLeading',
  HeaderTrailing = 'HeaderTrailing',
  ListHeader = 'ListHeader',
  ItemTrailing = 'ItemTrailing',
  Context = 'Context',
  EmptyState = 'EmptyState'
}

export interface ActionSlot {
  role: ActionRole
  region: ActionRegion
  order: number
  requiresConfirmation: boolean
}

export interface ScreenPolicy {
  screenRole: ScreenRole
  actionSlots: ActionSlot[]
}
