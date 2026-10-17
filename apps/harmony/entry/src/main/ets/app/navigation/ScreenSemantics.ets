// ScreenSemantics — Harmony 平台自己的页面语义与动作表。
//
// Issue #920 D（以 owner 第二条评论为准）：布局与控件摆放完全由平台 UI 层决定，
// WriterCore 不再返回布局模式 / 栏宽 / 矩形 bounds / 控件区域 / 导航摆放位置，
// 也不再提供 ScreenPolicy。这里把原先从 Core `resolve_screen_policy` 取到的
// 「页面角色 → 页头 / 上下文动作 + 是否显示一级导航」收回到鸿蒙 UI 层，作为
// 平台自己的产品动作表，避免再引入跨端共享布局真相。
//
// 说明：
// - 本文件是纯平台 UI 语义（页面角色、动作区域、动作顺序、是否需要确认），不含布局几何。
// - 动作表与旧 Core `presentation/screen` 的产品顺序保持一致，行为等价。

/** 页面角色 — 供导航 chrome（一级导航显隐、页头动作）区分当前页。 */
export enum ScreenRole {
  Home = 'Home',
  ProjectList = 'ProjectList',
  ProjectWorkspace = 'ProjectWorkspace',
  Writing = 'Writing',
  StarMap = 'StarMap',
  StarMapDetail = 'StarMapDetail',
  Stats = 'Stats',
  Settings = 'Settings',
  Sync = 'Sync'
}

/** 动作角色 — 平台侧真实存在的产品动作。 */
export enum ActionRole {
  Back = 'Back',
  CreateProject = 'CreateProject',
  CreateVolume = 'CreateVolume',
  CreateChapter = 'CreateChapter',
  // Issue #932：星图页「+」进入统一动作表（PrimaryAction）；动作执行仍由页面 requestCreateStarMap() 负责。
  CreateStarMap = 'CreateStarMap',
  Delete = 'Delete',
  Rename = 'Rename',
  MoveEarlier = 'MoveEarlier',
  MoveLater = 'MoveLater',
  Settings = 'Settings',
  Sync = 'Sync',
  Search = 'Search'
}

/** 动作的业务目标身份。 */
export enum ActionTarget {
  App = 'App',
  Project = 'Project',
  Volume = 'Volume',
  Chapter = 'Chapter'
}

/** 动作所属的页面区域。 */
export enum ActionRegion {
  HeaderLeading = 'HeaderLeading',
  HeaderTrailing = 'HeaderTrailing',
  PrimaryAction = 'PrimaryAction',
  ListHeader = 'ListHeader',
  ItemTrailing = 'ItemTrailing',
  Context = 'Context',
  EmptyState = 'EmptyState'
}

/** 单个动作槽位 — 区域、顺序与业务目标。 */
export interface ActionSlot {
  role: ActionRole
  target: ActionTarget
  region: ActionRegion
  order: number
  requiresConfirmation: boolean
}

/** 页面策略 — 页面角色对应的动作槽位与一级导航显隐。 */
export interface ScreenPolicy {
  screenRole: ScreenRole
  actionSlots: ActionSlot[]
  showPrimaryNavigation: boolean
}

function slot(role: ActionRole, target: ActionTarget, region: ActionRegion,
  order: number, requiresConfirmation: boolean): ActionSlot {
  return {
    role: role,
    target: target,
    region: region,
    order: order,
    requiresConfirmation: requiresConfirmation
  }
}

/** 全局页头右侧动作 — 同步 / 搜索 / 设置（所有需要这三个动作的页面共用一份）。 */
function globalHeaderTrailingActions(): ActionSlot[] {
  return [
    slot(ActionRole.Sync, ActionTarget.App, ActionRegion.HeaderTrailing, 10, false),
    slot(ActionRole.Search, ActionTarget.App, ActionRegion.HeaderTrailing, 20, false),
    slot(ActionRole.Settings, ActionTarget.App, ActionRegion.HeaderTrailing, 30, false)
  ]
}

/** 按页面角色解析动作槽位。纯函数，无副作用。 */
export function resolveActionSlots(screenRole: ScreenRole): ActionSlot[] {
  if (screenRole === ScreenRole.Home) {
    return globalHeaderTrailingActions()
  }
  if (screenRole === ScreenRole.ProjectList) {
    const actions: ActionSlot[] = globalHeaderTrailingActions()
    actions.push(slot(ActionRole.CreateProject, ActionTarget.Project, ActionRegion.PrimaryAction, 10, false))
    actions.push(slot(ActionRole.Delete, ActionTarget.Project, ActionRegion.Context, 10, true))
    actions.push(slot(ActionRole.Rename, ActionTarget.Project, ActionRegion.Context, 20, false))
    return actions
  }
  if (screenRole === ScreenRole.ProjectWorkspace) {
    const actions: ActionSlot[] = [
      slot(ActionRole.Back, ActionTarget.App, ActionRegion.HeaderLeading, 10, false)
    ]
    for (const global of globalHeaderTrailingActions()) {
      actions.push(global)
    }
    actions.push(slot(ActionRole.CreateVolume, ActionTarget.Project, ActionRegion.PrimaryAction, 10, false))
    actions.push(slot(ActionRole.CreateChapter, ActionTarget.Volume, ActionRegion.ItemTrailing, 10, false))
    actions.push(slot(ActionRole.CreateChapter, ActionTarget.Volume, ActionRegion.EmptyState, 10, false))
    actions.push(slot(ActionRole.Delete, ActionTarget.Volume, ActionRegion.Context, 10, true))
    actions.push(slot(ActionRole.Delete, ActionTarget.Chapter, ActionRegion.Context, 20, true))
    actions.push(slot(ActionRole.Rename, ActionTarget.Volume, ActionRegion.Context, 30, false))
    actions.push(slot(ActionRole.Rename, ActionTarget.Chapter, ActionRegion.Context, 40, false))
    actions.push(slot(ActionRole.MoveEarlier, ActionTarget.Volume, ActionRegion.Context, 50, false))
    actions.push(slot(ActionRole.MoveLater, ActionTarget.Volume, ActionRegion.Context, 60, false))
    actions.push(slot(ActionRole.MoveEarlier, ActionTarget.Chapter, ActionRegion.Context, 70, false))
    actions.push(slot(ActionRole.MoveLater, ActionTarget.Chapter, ActionRegion.Context, 80, false))
    return actions
  }
  if (screenRole === ScreenRole.Writing) {
    const actions: ActionSlot[] = [
      slot(ActionRole.Back, ActionTarget.App, ActionRegion.HeaderLeading, 10, false)
    ]
    for (const global of globalHeaderTrailingActions()) {
      actions.push(global)
    }
    return actions
  }
  if (screenRole === ScreenRole.StarMap) {
    // Issue #932：星图「+」并入统一动作表，与 CreateProject 同为 PrimaryAction；
    // 页面消费该 slot 的点击仍走自己的 requestCreateStarMap()，dispatcher 不加新业务分支。
    const actions: ActionSlot[] = globalHeaderTrailingActions()
    actions.push(slot(ActionRole.CreateStarMap, ActionTarget.App, ActionRegion.PrimaryAction, 10, false))
    return actions
  }
  if (screenRole === ScreenRole.StarMapDetail) {
    const actions: ActionSlot[] = [
      slot(ActionRole.Back, ActionTarget.App, ActionRegion.HeaderLeading, 10, false)
    ]
    for (const global of globalHeaderTrailingActions()) {
      actions.push(global)
    }
    return actions
  }
  if (screenRole === ScreenRole.Stats) {
    return globalHeaderTrailingActions()
  }
  if (screenRole === ScreenRole.Settings) {
    return [slot(ActionRole.Back, ActionTarget.App, ActionRegion.HeaderLeading, 10, false)]
  }
  // Sync
  return [
    slot(ActionRole.Back, ActionTarget.App, ActionRegion.HeaderLeading, 10, false),
    slot(ActionRole.Sync, ActionTarget.App, ActionRegion.HeaderTrailing, 10, false)
  ]
}

/** 该页面是否显示一级导航。Writing / Settings / StarMapDetail 是沉浸二级页，隐藏底栏。 */
export function resolveShowPrimaryNavigation(screenRole: ScreenRole): boolean {
  return !(screenRole === ScreenRole.Writing ||
    screenRole === ScreenRole.Settings ||
    screenRole === ScreenRole.StarMapDetail)
}

/** 按页面角色解析完整页面策略。纯函数，无副作用。 */
export function resolveScreenPolicy(screenRole: ScreenRole): ScreenPolicy {
  return {
    screenRole: screenRole,
    actionSlots: resolveActionSlots(screenRole),
    showPrimaryNavigation: resolveShowPrimaryNavigation(screenRole)
  }
}
