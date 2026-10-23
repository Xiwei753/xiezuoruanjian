// ContinuationPayload — 应用接续（Continuation）携带的数据。
// 正文数据仍由作品数据层和同步体系负责，本 payload 只携带当前页面、对象和视图状态。
//
// Issue #988：接续不能只记“哪一章”，还要还原用户点击接续瞬间所在的真实页面层级、
// 星图子图/相机/选中节点、设置分类、搜索词以及编辑器选区/视口。GitHub 仓库身份只做
// 归一化校验，不携带 token；未登录华为账号、未配置 GitHub 仓库都不影响接续。
//
// contentHash 是 Core 落盘内容的稳定身份（ChapterContent.meta.hash /
// ChapterSaveReceipt.contentHash），不是 TextEditSession.revision。未保存正文用
// snapshotId + snapshotHash 标识源设备冻结的那一版内容，由附近设备同步或应用接续
// 快照通道交付；目标设备必须先校验内容身份，再套用 UTF-16 选区/视口。

import { EditorViewportState } from '../../feature/editor/render/EditorViewportState'

// v2：页面级状态。旧 v1 payload 只有章节/选区，仍按写作页兼容读取。
export const CONTINUATION_PAYLOAD_VERSION: number = 2

export type ContinuationScreen =
  'home' | 'workspace' | 'writing' | 'starmapList' | 'starmap' | 'stats' | 'settings' | 'search'

export interface ContinuationPayload {
  version?: number
  // 页面快照由谁、按什么顺序写入。后台常驻页面不能覆盖前台真实导航栈。
  snapshotOwner?: string
  snapshotSequence?: number
  screen?: ContinuationScreen
  tabIndex?: number
  projectId?: string
  volumeId?: string
  chapterId?: string
  selection?: number[]
  cursor?: number
  viewport?: EditorViewportState
  contentHash?: string
  // 未保存正文快照：snapshotId 用于向源设备/附近设备取回，snapshotHash 用于校验。
  snapshotId?: string
  snapshotHash?: string
  // 源设备身份：目标设备可据此向可信连接请求该 snapshotId，而不是只依赖已连接会话。
  sourceDeviceId?: string
  sourceNetworkId?: string
  // 星图：子图路径、相机、当前选中的节点/边。
  starmapId?: string
  starmapTitle?: string
  parentStarmapId?: string
  parentStarmapTitle?: string
  parentPath?: string
  focusKind?: string
  focusId?: string
  focusScenePath?: string
  selectedNodeId?: string
  selectedKind?: string
  cameraScale?: number
  cameraOffsetX?: number
  cameraOffsetY?: number
  // 设置/搜索。
  settingsSection?: string
  settingKey?: string
  statsPeriod?: string
  statsExpandedPanel?: string
  statsHeatmapYear?: number
  searchQuery?: string
  searchOriginScreen?: string
  searchProjectId?: string
  searchVolumeId?: string
  searchChapterId?: string
  searchStarmapId?: string
  searchSettingsSection?: string
  // 云备份仓库身份（不含 token）。目标设备据此判断是否需要先等同一个仓库的同步。
  repoRemoteUrl?: string
  repoBranch?: string
}

// 页面到一级 Tab 的稳定映射：接续恢复不能永远回到 Home。
export function continuationTabIndex(payload: ContinuationPayload): number {
  if (payload.screen === 'starmapList' || payload.screen === 'starmap') {
    return 1
  }
  if (payload.screen === 'stats') {
    return 2
  }
  if (payload.tabIndex === 0 || payload.tabIndex === 1 || payload.tabIndex === 2) {
    return payload.tabIndex
  }
  return 0
}
