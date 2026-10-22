// Routes — 应用路由枚举与参数类型。
// 页面通过统一 Navigation（AppNavigation）进出，不再用 router 跳转。
//
// Issue #629 评论 5 第 8 节：WritingRouteParams 跨设备校验字段改成稳定 contentHash，
// 不用 TextEditSession.revision；viewport 用强类型 EditorViewportState，不用裸 object。

import { EditorViewportState } from '../../feature/editor/render/EditorViewportState'
import { SearchContext } from '../../corebridge/dto/SearchDtos'

export enum Routes {
  Home = 'Home',
  Workspace = 'Workspace',
  Writing = 'Writing',
  StarMapList = 'StarMapList',
  StarMap = 'StarMap',
  Settings = 'Settings',
  Stats = 'Stats',
  Search = 'Search'
}

// 一级 Tab 索引（与 PrimaryTabShell 的 Tabs 顺序一致）。
export const TAB_HOME: number = 0
export const TAB_STARMAP: number = 1
export const TAB_STATS: number = 2

// 搜索结果在星图里需要精确定位的对象类型。
// link / hyperlink 没有独立位置，由页面解析 source 后选中其所属 node/embed。
export type StarMapFocusKind = 'node' | 'edge' | 'embed' | 'link' | 'hyperlink'

export interface WorkspaceRouteParams {
  projectId: string
  projectName: string
  // 搜索结果跳到卷时展开对应卷卡；普通进入不传。
  focusVolumeId?: string
}

export interface WritingRouteParams {
  chapterId?: string              // 可选：未选章节时不传
  chapterTitle?: string           // 可选：未选章节时不传
  projectId: string
  volumeId?: string               // 可选：未选章节时不传
  // 接续编辑器状态（Issue #629 评论 5306513458 问题4 + 评论 5 第 8 节）。
  // selection/cursor 是 UTF-16 code unit offset（UI 坐标，跨设备平台无关）；
  // 恢复时由 WritingScreen 转成 Core UTF-8 byte offset。
  selection?: number[]              // [anchorUtf16, headUtf16]
  cursor?: number                   // UTF-16 code unit offset
  viewport?: EditorViewportState    // UI 滚动位置（强类型，不写 Core）
  contentHash?: string              // 跨设备稳定内容身份（不用 revision）
  // 全局搜索命中（Issue #982）。searchByteStart/searchByteEnd 是 Core 原文 UTF-8 byte 偏移；
  // searchField='body' 时 WritingScreen 在正文中转 UTF-16 后选中并滚动到该范围。
  searchByteStart?: number
  searchByteEnd?: number
  searchField?: string
}

export interface StarMapRouteParams {
  starmapId: string
  starmapTitle: string
  parentStarmapId?: string
  parentStarmapTitle?: string
  // 搜索结果精确落到节点/边/嵌入/链接；页面加载图后解析几何并选中、居中。
  focusKind?: StarMapFocusKind
  focusId?: string
}

// 全局搜索路由参数：打开搜索页时携带来源上下文，不改变底层页面状态。
export interface SearchRouteParams {
  context: SearchContext
  initialQuery?: string
}

// Settings 页是应用级页面（Issue #784 评论 5856805657）。
// section 仅用于搜索结果跳转到具体设置分类；settingKey（Issue #982）进一步定位到具体设置项。
export interface SettingsRouteParams {
  section?: string
  settingKey?: string
}
