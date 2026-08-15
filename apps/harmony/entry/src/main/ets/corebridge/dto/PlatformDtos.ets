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
  showPrimaryNavigation: boolean
}

// Harmony 平台在 Core 契约之上叠加自己的平台值后供页面消费。
export interface LayoutPlan {
  shellMode: ShellMode
  workspacePaneMode: WorkspacePaneMode
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

export interface ScreenPolicy {
  screenRole: ScreenRole
  actionSlots: ActionSlot[]
}
