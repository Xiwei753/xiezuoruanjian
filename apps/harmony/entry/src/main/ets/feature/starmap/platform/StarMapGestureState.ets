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
 * 归属仲裁的职责。现在把归属按手势类型分开记：pan / connect / 当前操作对象
 * 各有一条独立的 owner 路径，且**手势归属一旦定下就不改**，只有对应的结束事件能清掉。
 *
 * pinch 不再有归属（Issue #818）：
 * 双指只有一台全局相机，"这一层是不是 owner"对缩放已经没有意义——双指落在节点上、
 * 落在子星图上、落在二层子星图上，缩的都是整棵树。因此 pinch 只记一个进行中的标志，
 * 相机状态由 StarMapScreen 持有，不存在"缩放某个子 Scene"这条路径了。
 *
 * 双指 > 所有单指（Issue #818 复审）：
 * 真机上第一根手指落下时，Tap/Pan/LongPress 这些单指识别器已经先进入识别，
 * 第二根手指才让高优先级 Pinch 认领。所以单指回调会晚到、也会在 pinch 期间继续到。
 * 这里给所有单指 begin* 统一加了 pinch 守卫：pinch 进行中不接受单指改写 mode。
 * 反过来 pinch 认领时 beginPinch() 走的是同一条路径（emptyState() 重建整个状态），
 * 原子覆盖掉当时的 pan / 菜单 / 连线 / 移动，不会出现两套状态并存。
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

  /**
   * 是否正在双指缩放。
   *
   * 双指优先级高于所有单指手势（#818 复审）：第一根手指落下时 ArkUI 的
   * Tap/Pan/LongPress 已经在竞争，第二根手指才让 Pinch 认领。那些先行的
   * 单指回调会晚到，所以状态机这一层必须兜住——pinch 进行中不接受任何
   * 单指 begin* 改写 mode，否则会把 pinch 覆盖掉、或者反过来抢走相机控制权。
   */
  private isPinchingState(): boolean {
    return this.state.mode === 'pinch'
  }

  /** 进入画布拖动模式 */
  beginPanCanvas(ownerScenePath: StarMapPathSegment[], startX: number, startY: number): void {
    if (this.isPinchingState()) {
      return
    }
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
    if (this.isPinchingState()) {
      return
    }
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
    if (this.isPinchingState()) {
      return
    }
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
    if (this.isPinchingState()) {
      return
    }
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
    if (this.isPinchingState()) {
      return
    }
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
   * 双指开始缩放整棵星图（#818）。
   *
   * 没有 owner 参数：全局相机只有一台，双指落在哪一层都不改变缩放目标。
   * 这里只负责把 mode 切成 pinch，让"有别的 Scene 正在拖画布/连线"这件事能被观察到。
   */
  beginPinch(centerX: number, centerY: number): void {
    const next = emptyState()
    next.mode = 'pinch'
    // 双指只在根 Scene 的 Stack 上统一接收，归属恒为根（空路径）。
    next.ownerScenePath = []
    next.startPoint = { x: centerX, y: centerY }
    next.currentPoint = { x: centerX, y: centerY }
    this.state = next
  }

  /** 双指缩放结束，回到 idle。 */
  endPinch(): void {
    if (this.state.mode !== 'pinch') {
      return
    }
    this.state = emptyState()
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

  /** 当前是否有双指正在缩放整棵星图（#818：无归属，只有进行中与否） */
  isPinching(): boolean {
    return this.state.mode === 'pinch'
  }

  /**
   * 当前手势是否归属在指定 Scene 的**子树**里（含它自己）。
   *
   * 树里所有 Scene 共用同一个 tracker。父层 child Scene 被卸载（Embed 被删、
   * childGraph 重新加载、loading/error 分支切换）时，aboutToDisappear 只能清
   * 属于自己子树的手势：手势可能实际属于根 Scene 或兄弟子树，无关 Scene 退场
   * 把全树唯一的 tracker 清掉，正在进行的缩放/拖动就断了。
   *
   * 必须比 isOwnedByScene 更宽：父 Scene 退场时真正的 owner 可能是它里面
   * 更深一层的孙 Scene，所以要按路径前缀逐段比较，而不是要求完全相等。
   */
  isOwnedBySceneSubtree(scenePath: StarMapPathSegment[]): boolean {
    const owner = this.state.ownerScenePath
    if (owner.length < scenePath.length) {
      return false
    }
    for (let i = 0; i < scenePath.length; i++) {
      if (owner[i].type !== scenePath[i].type ||
        owner[i].instanceId !== scenePath[i].instanceId ||
        owner[i].nodeId !== scenePath[i].nodeId) {
        return false
      }
    }
    return true
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

  /**
   * 只在"当前全局状态确实还是我发起的那种手势"时才清空（#816）。
   *
   * 单指手势的收尾回调可能晚于新手势的起手到达：
   * 第一根手指已经让某一层认领了 panCanvas，第二根手指落下后根 Scene 会把
   * 全局状态切成 pinch。这时旧 pan 的 onActionEnd / onActionCancel 如果只判断
   * "ownerScenePath 是不是我这层"（beginPinch 会把 ownerScenePath 也写成归属层，
   * 于是同一层的旧 pan 依然判定为 true），就会把刚认领的 pinch 一起 reset 掉，
   * 屏幕上表现为双指刚开始就失效（#816 真机症状）。
   *
   * 所以清空前必须同时核对 mode 和归属，两者都匹配才允许清。
   * 匹配则清空并返回 true；否则什么都不做并返回 false。
   */
  clearIfOwnedBy(expectedMode: StarMapGestureMode, scenePath: StarMapPathSegment[]): boolean {
    if (this.state.mode !== expectedMode) {
      return false
    }
    if (!isSameScenePath(this.state.ownerScenePath, scenePath)) {
      return false
    }
    this.reset()
    return true
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
      connectOwnerScenePath: s.connectOwnerScenePath !== null ? copyPath(s.connectOwnerScenePath) : null,
      connectSourceScenePath: s.connectSourceScenePath !== null ? copyPath(s.connectSourceScenePath) : null,
      startPoint: { x: s.startPoint.x, y: s.startPoint.y },
      currentPoint: { x: s.currentPoint.x, y: s.currentPoint.y },
      targetItemId: s.targetItemId,
      targetItemScenePath: s.targetItemScenePath !== null ? copyPath(s.targetItemScenePath) : null
    }
  }
}
