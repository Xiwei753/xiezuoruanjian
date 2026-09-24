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

export enum WorkspaceLayoutMode {
  SinglePane = 'SinglePane',
  Workbench = 'Workbench'
}

export enum PrimaryNavigationPlacement {
  Bottom = 'Bottom',
  Side = 'Side'
}

// 共用布局尺寸 — Core LayoutMetricsDto（dp，唯一来源，平台端不得各自硬编码）。
export interface LayoutMetrics {
  listPaneWidthDp: number
  projectCardMinWidthDp: number
  toolPaneWidthDp: number
  toolRailWidthDp: number
  editorMinWidthDp: number
  toolbarHeightDp: number
  toolbarLeadingWidthDp: number
  toolbarTrailingWidthDp: number
  listPaneMinWidthDp: number
  toolPaneMinWidthDp: number
}

// 窗口遮挡 — Core WindowOcclusionDto。
export interface WindowOcclusion {
  leftDp: number
  topDp: number
  rightDp: number
  bottomDp: number
  separating: boolean
}

// resolveLayout 的入参 — Core WindowViewportDto（#753 评论 5809590165）。
// 平台只上报原始窗口尺寸与遮挡，paneCount / 折叠 / 指针类型都是 Core 的决策输出。
export interface WindowViewport {
  widthDp: number
  heightDp: number
  occlusions: WindowOcclusion[]
}

// 布局契约 — Core LayoutContractDto。
export interface LayoutContract {
  shellMode: ShellMode
  workspaceLayoutMode: WorkspaceLayoutMode
  primaryNavigationPlacement: PrimaryNavigationPlacement
  metrics: LayoutMetrics
}

// 布局计划 — Harmony 平台值，在 Core LayoutContractDto 之上叠加纸面宽度/内边距/网格列数。
// 这不是 Core 契约（Core 只发 LayoutContract），平台值由 LayoutPolicyHelper 计算。
export interface LayoutPlan {
  shellMode: ShellMode
  workspaceLayoutMode: WorkspaceLayoutMode
  primaryNavigationPlacement: PrimaryNavigationPlacement
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
  CreateProject = 'CreateProject',
  CreateVolume = 'CreateVolume',
  CreateChapter = 'CreateChapter',
  Delete = 'Delete',
  Rename = 'Rename',
  // #610 评论四：真实存在的顺序动作（上移）。
  MoveEarlier = 'MoveEarlier',
  // #610 评论四：真实存在的顺序动作（下移）。
  MoveLater = 'MoveLater',
  Settings = 'Settings',
  Sync = 'Sync',
  Search = 'Search'
}

// #610 评论二：平台无关的业务目标身份。
export enum ActionTarget {
  App = 'App',
  Project = 'Project',
  Volume = 'Volume',
  Chapter = 'Chapter'
}

export enum ActionRegion {
  HeaderLeading = 'HeaderLeading',
  HeaderTrailing = 'HeaderTrailing',
  // #610 评论四：页面主操作区域（新建作品等）。
  PrimaryAction = 'PrimaryAction',
  ListHeader = 'ListHeader',
  ItemTrailing = 'ItemTrailing',
  Context = 'Context',
  EmptyState = 'EmptyState'
}

export interface ActionSlot {
  role: ActionRole
  target: ActionTarget
  region: ActionRegion
  order: number
  requiresConfirmation: boolean
}

// 页面策略 — Core ScreenPolicyDto。
export interface ScreenPolicy {
  screenRole: ScreenRole
  showPrimaryNavigation: boolean
  actionSlots: ActionSlot[]
}
