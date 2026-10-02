/**
 * StarMapGestureState — 星图触屏交互状态机
 *
 * 纯状态管理模块，不依赖 ArkUI 组件，不依赖 corebridge。
 * 所有状态变更通过方法调用驱动，外部不得直接赋值字段。
 *
 * 触屏规则说明：
 * - 空白未长按直接滑动 → panCanvas（画布拖动）
 * - 点节点 → 选中（只改变视觉选中态，不进入手势模式）
 * - 长按节点 → nodeMenu（弹出节点菜单）
 * - 长按后继续移动 → 从 nodeMenu 切到 connect，关闭菜单显示预览线
 * - 松手落到另一节点 → 创建关系
 * - "移动节点"必须从菜单显式进入 moveNode，之后 Pan 才改 node.position
 * - Embed/子星图同样通过显式"移动"进入 moveEmbed
 * - 普通滑动不能误拖节点
 *
 * 递归手势归属（Issue #816）：
 * 之前只有一个 ownerScenePath 跟着"当前 mode"走，于是"这层是不是空闲"实际承担了
 * 归属仲裁的职责——双指落在子图上时，父层会看到自己不是 idle 就退出，但这既不准确
 * （父层本来就没参与）也不彻底（父子可以同时非 idle，归属就在中间漂）。
 * 现在把归属按手势类型分开记：pinch / pan / connect / 当前操作对象各有一条独立的 owner 路径，
 * 且**手势归属一旦定下就不改**，只有对应的结束事件能清掉。
 * 父层看到子层的 pinch 时只能观察（isPinchOwnedByScene 为 false），
 * 不允许顺手改自己的 offset——改了就变成父子一起缩放。
 */

import { PlatformPoint } from './StarMapLayout'
import { StarMapPathSegment } from '../../../corebridge/dto/StarMapDtos'
import { isSameScenePath } from './StarMapGeometry'

/** 手势模式枚举 */
export type StarMapGestureMode = 'idle' | 'panCanvas' | 'nodeMenu' | 'connect' | 'moveNode' | 'moveEmbed' | 'pinch'

/** 触屏状态数据接口 */
export interface GestureState {
  /** 当前手势模式 */
  mode: StarMapGestureMode
  /** 当前操作的 item ID（普通节点 nodeId 或 Embed instanceId，空字符串表示无） */
  activeItemId: string
  /** 当前操作的 item 类型（'node' 或 'embed'，idle/panCanvas 时为 'node'） */
  activeItemKind: 'node' | 'embed'
  /** 当前手势所属的 scene 路径（根 scene 为空数组 []） */
  ownerScenePath: StarMapPathSegment[]
  /** 当前被操作对象所在 scene 的路径；没有操作对象时为 null */
  activeItemScenePath: StarMapPathSegment[] | null
  /** panCanvas 的归属 scene；没有正在拖画布时为 null */
  panOwnerScenePath: StarMapPathSegment[] | null
  /** pinch 的归属 scene；没有正在双指缩放时为 null。定下之后不换人，直到双指抬起 */
  pinchOwnerScenePath: StarMapPathSegment[] | null
  /** connect 的归属 scene；没有正在连线时为 null */
  connectOwnerScenePath: StarMapPathSegment[] | null
  /** connect 源对象所在 scene 的路径（#816 跨层连线的起点依据） */
  connectSourceScenePath: StarMapPathSegment[] | null
  /** 手势起点（画布坐标） */
  startPoint: PlatformPoint
  /** 当前点（画布坐标） */
  currentPoint: PlatformPoint
  /** connect 模式下目标 item ID */
  targetItemId: string
  /** connect 模式下目标所在 scene 的路径（#816 跨层连线的终点依据） */
  targetItemScenePath: StarMapPathSegment[] | null
}

/** 构造一份空状态，避免每个 begin* 手写一遍字段。 */
function emptyState(): GestureState {
  return {
    mode: 'idle',
    activeItemId: '',
    activeItemKind: 'node',
    ownerScenePath: [],
    activeItemScenePath: null,
    panOwnerScenePath: null,
    pinchOwnerScenePath: null,
    connectOwnerScenePath: null,
    connectSourceScenePath: null,
    startPoint: { x: 0, y: 0 },
    currentPoint: { x: 0, y: 0 },
    targetItemId: '',
    targetItemScenePath: null
  }
}

/** 深拷贝路径段列表。 */
function copyPath(path: StarMapPathSegment[]): StarMapPathSegment[] {
  return path.map((seg: StarMapPathSegment): StarMapPathSegment => ({
    type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId
  }))
}

/**
 * 星图触屏状态管理类
 *
 * 维护触屏交互的完整状态机，通过方法调用驱动状态流转。
 * 外部通过 getState() 获取当前状态快照，通过 isXxx() 判断当前模式。
 */
export class StarMapGestureStateTracker {
  /** 内部状态，外部通过 getState() 读取 */
  private state: GestureState = emptyState()

  /** 回到 idle，清空所有状态 */
  reset(): void {
    this.state = emptyState()
  }

  /** 进入画布拖动模式 */
  beginPanCanvas(ownerScenePath: StarMapPathSegment[], startX: number, startY: number): void {
    const next = emptyState()
    next.mode = 'panCanvas'
    next.ownerScenePath = copyPath(ownerScenePath)
    next.panOwnerScenePath = copyPath(ownerScenePath)
    next.startPoint = { x: startX, y: startY }
    next.currentPoint = { x: startX, y: startY }
    this.state = next
  }

  /** 长按节点弹出菜单 */
  beginNodeMenu(ownerScenePath: StarMapPathSegment[], nodeId: string, startX: number, startY: number): void {
    const next = emptyState()
    next.mode = 'nodeMenu'
    next.activeItemId = nodeId
    next.activeItemKind = 'node'
    next.ownerScenePath = copyPath(ownerScenePath)
    next.activeItemScenePath = copyPath(ownerScenePath)
    next.startPoint = { x: startX, y: startY }
    next.currentPoint = { x: startX, y: startY }
    this.state = next
  }

  /** 从菜单态切到连线模式 */
  beginConnect(ownerScenePath: StarMapPathSegment[], nodeId: string, startX: number, startY: number): void {
    const next = emptyState()
    next.mode = 'connect'
    next.activeItemId = nodeId
    next.activeItemKind = 'node'
    next.ownerScenePath = copyPath(ownerScenePath)
    next.activeItemScenePath = copyPath(ownerScenePath)
    next.connectOwnerScenePath = copyPath(ownerScenePath)
    next.connectSourceScenePath = copyPath(ownerScenePath)
    next.startPoint = { x: startX, y: startY }
    next.currentPoint = { x: startX, y: startY }
    this.state = next
  }

  /** 从菜单显式进入移动节点模式 */
  beginMoveNode(ownerScenePath: StarMapPathSegment[], nodeId: string, startX: number, startY: number): void {
    const next = emptyState()
    next.mode = 'moveNode'
    next.activeItemId = nodeId
    next.activeItemKind = 'node'
    next.ownerScenePath = copyPath(ownerScenePath)
    next.activeItemScenePath = copyPath(ownerScenePath)
    next.startPoint = { x: startX, y: startY }
    next.currentPoint = { x: startX, y: startY }
    this.state = next
  }

  /** 从菜单显式进入移动 Embed 模式 */
  beginMoveEmbed(ownerScenePath: StarMapPathSegment[], embedInstanceId: string, startX: number, startY: number): void {
    const next = emptyState()
    next.mode = 'moveEmbed'
    next.activeItemId = embedInstanceId
    next.activeItemKind = 'embed'
    next.ownerScenePath = copyPath(ownerScenePath)
    next.activeItemScenePath = copyPath(ownerScenePath)
    next.startPoint = { x: startX, y: startY }
    next.currentPoint = { x: startX, y: startY }
    this.state = next
  }

  /**
   * 双指缩放归属到某层 Scene。
   *
   * 归属只在"还没有人认领"时才能定下（isPinchOwnedByScene / canClaimPinch 做这件事）：
   * 一次双指从落下到抬起之间如果改归属，父子两层会轮流被缩放，
   * 屏幕上表现为缩放抖动、子图突然跳位（#816 真机症状）。
   */
  beginPinch(ownerScenePath: StarMapPathSegment[], centerX: number, centerY: number): void {
    const next = emptyState()
    next.mode = 'pinch'
    next.ownerScenePath = copyPath(ownerScenePath)
    next.pinchOwnerScenePath = copyPath(ownerScenePath)
    next.startPoint = { x: centerX, y: centerY }
    next.currentPoint = { x: centerX, y: centerY }
    this.state = next
  }

  /**
   * 双指缩放结束。
   * 只有归属层会走到这里，父层保持 observe（自己的 offset 一个都不动）。
   */
  endPinch(): void {
    if (this.state.pinchOwnerScenePath === null) {
      return
    }
    const next = emptyState()
    this.state = next
  }

  /** 更新当前点（画布坐标） */
  updateCurrent(x: number, y: number): void {
    this.state.currentPoint = { x, y }
  }

  /** connect 模式下设置目标 item */
  setTargetItem(itemId: string): void {
    this.state.targetItemId = itemId
  }

  /** connect 模式下设置目标所在 scene 的路径 */
  setTargetItemScenePath(scenePath: StarMapPathSegment[]): void {
    this.state.targetItemScenePath = copyPath(scenePath)
  }

  /** 当前是否处于 idle 状态 */
  isIdle(): boolean {
    return this.state.mode === 'idle'
  }

  /** 当前是否正在拖动节点（moveNode 或 moveEmbed 模式） */
  isDraggingNode(): boolean {
    return this.state.mode === 'moveNode' || this.state.mode === 'moveEmbed'
  }

  /** 当前是否处于连线模式 */
  isConnecting(): boolean {
    return this.state.mode === 'connect'
  }

  /** 当前是否有任何一层 Scene 正在被双指缩放（无论归属是谁） */
  isPinching(): boolean {
    return this.state.pinchOwnerScenePath !== null
  }

  /**
   * 双指缩放是否归属指定 Scene。
   * 归属层返回 true 才可以改自己的视口；其余层一律只能观察。
   */
  isPinchOwnedByScene(scenePath: StarMapPathSegment[]): boolean {
    if (this.state.pinchOwnerScenePath === null) {
      return false
    }
    return isSameScenePath(this.state.pinchOwnerScenePath, scenePath)
  }

  /**
   * 指定 Scene 能否认领双指缩放。
   * 还没人认领时才能认领；已归属别人时返回 false，归属就不会中途漂移。
   */
  canClaimPinch(scenePath: StarMapPathSegment[]): boolean {
    if (this.state.pinchOwnerScenePath === null) {
      return true
    }
    return isSameScenePath(this.state.pinchOwnerScenePath, scenePath)
  }

  /** pan 是否归属指定 Scene */
  isPanOwnedByScene(scenePath: StarMapPathSegment[]): boolean {
    if (this.state.panOwnerScenePath === null) {
      return false
    }
    return isSameScenePath(this.state.panOwnerScenePath, scenePath)
  }

  /** connect 是否归属指定 Scene */
  isConnectOwnedByScene(scenePath: StarMapPathSegment[]): boolean {
    if (this.state.connectOwnerScenePath === null) {
      return false
    }
    return isSameScenePath(this.state.connectOwnerScenePath, scenePath)
  }

  /** 当前操作对象是否在指定 Scene */
  isActiveItemInScene(scenePath: StarMapPathSegment[]): boolean {
    if (this.state.activeItemScenePath === null) {
      return false
    }
    return isSameScenePath(this.state.activeItemScenePath, scenePath)
  }

  /** 判断当前手势是否属于指定 scene（路径深度比较） */
  isOwnedByScene(scenePath: StarMapPathSegment[]): boolean {
    return isSameScenePath(this.state.ownerScenePath, scenePath)
  }

  /** 获取当前状态快照（返回只读副本，外部不应修改） */
  getState(): GestureState {
    const s: GestureState = this.state
    return {
      mode: s.mode,
      activeItemId: s.activeItemId,
      activeItemKind: s.activeItemKind,
      ownerScenePath: copyPath(s.ownerScenePath),
      activeItemScenePath: s.activeItemScenePath !== null ? copyPath(s.activeItemScenePath) : null,
      panOwnerScenePath: s.panOwnerScenePath !== null ? copyPath(s.panOwnerScenePath) : null,
      pinchOwnerScenePath: s.pinchOwnerScenePath !== null ? copyPath(s.pinchOwnerScenePath) : null,
      connectOwnerScenePath: s.connectOwnerScenePath !== null ? copyPath(s.connectOwnerScenePath) : null,
      connectSourceScenePath: s.connectSourceScenePath !== null ? copyPath(s.connectSourceScenePath) : null,
      startPoint: { x: s.startPoint.x, y: s.startPoint.y },
      currentPoint: { x: s.currentPoint.x, y: s.currentPoint.y },
      targetItemId: s.targetItemId,
      targetItemScenePath: s.targetItemScenePath !== null ? copyPath(s.targetItemScenePath) : null
    }
  }
}
