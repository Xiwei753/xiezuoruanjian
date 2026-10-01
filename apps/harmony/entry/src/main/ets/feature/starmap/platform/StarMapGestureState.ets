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
 */

import { PlatformPoint } from './StarMapLayout'
import { StarMapPathSegment } from '../../../corebridge/dto/StarMapDtos'

/** 手势模式枚举 */
export type StarMapGestureMode = 'idle' | 'panCanvas' | 'nodeMenu' | 'connect' | 'moveNode' | 'moveEmbed'

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
  /** 手势起点（画布坐标） */
  startPoint: PlatformPoint
  /** 当前点（画布坐标） */
  currentPoint: PlatformPoint
  /** connect 模式下目标 item ID */
  targetItemId: string
}

/**
 * 星图触屏状态管理类
 *
 * 维护触屏交互的完整状态机，通过方法调用驱动状态流转。
 * 外部通过 getState() 获取当前状态快照，通过 isXxx() 判断当前模式。
 */
export class StarMapGestureStateTracker {
  /** 内部状态，外部通过 getState() 读取 */
  private state: GestureState = {
    mode: 'idle',
    activeItemId: '',
    activeItemKind: 'node',
    ownerScenePath: [],
    startPoint: { x: 0, y: 0 },
    currentPoint: { x: 0, y: 0 },
    targetItemId: ''
  }

  /** 回到 idle，清空所有状态 */
  reset(): void {
    this.state = {
      mode: 'idle',
      activeItemId: '',
      activeItemKind: 'node',
      ownerScenePath: [],
      startPoint: { x: 0, y: 0 },
      currentPoint: { x: 0, y: 0 },
      targetItemId: ''
    }
  }

  /** 进入画布拖动模式 */
  beginPanCanvas(ownerScenePath: StarMapPathSegment[], startX: number, startY: number): void {
    this.state = {
      mode: 'panCanvas',
      activeItemId: '',
      activeItemKind: 'node',
      ownerScenePath: [...ownerScenePath],
      startPoint: { x: startX, y: startY },
      currentPoint: { x: startX, y: startY },
      targetItemId: ''
    }
  }

  /** 长按节点弹出菜单 */
  beginNodeMenu(ownerScenePath: StarMapPathSegment[], nodeId: string, startX: number, startY: number): void {
    this.state = {
      mode: 'nodeMenu',
      activeItemId: nodeId,
      activeItemKind: 'node',
      ownerScenePath: [...ownerScenePath],
      startPoint: { x: startX, y: startY },
      currentPoint: { x: startX, y: startY },
      targetItemId: ''
    }
  }

  /** 从菜单态切到连线模式 */
  beginConnect(ownerScenePath: StarMapPathSegment[], nodeId: string, startX: number, startY: number): void {
    this.state = {
      mode: 'connect',
      activeItemId: nodeId,
      activeItemKind: 'node',
      ownerScenePath: [...ownerScenePath],
      startPoint: { x: startX, y: startY },
      currentPoint: { x: startX, y: startY },
      targetItemId: ''
    }
  }

  /** 从菜单显式进入移动节点模式 */
  beginMoveNode(ownerScenePath: StarMapPathSegment[], nodeId: string, startX: number, startY: number): void {
    this.state = {
      mode: 'moveNode',
      activeItemId: nodeId,
      activeItemKind: 'node',
      ownerScenePath: [...ownerScenePath],
      startPoint: { x: startX, y: startY },
      currentPoint: { x: startX, y: startY },
      targetItemId: ''
    }
  }

  /** 从菜单显式进入移动 Embed 模式 */
  beginMoveEmbed(ownerScenePath: StarMapPathSegment[], embedInstanceId: string, startX: number, startY: number): void {
    this.state = {
      mode: 'moveEmbed',
      activeItemId: embedInstanceId,
      activeItemKind: 'embed',
      ownerScenePath: [...ownerScenePath],
      startPoint: { x: startX, y: startY },
      currentPoint: { x: startX, y: startY },
      targetItemId: ''
    }
  }

  /** 更新当前点（画布坐标） */
  updateCurrent(x: number, y: number): void {
    this.state.currentPoint = { x, y }
  }

  /** connect 模式下设置目标 item */
  setTargetItem(itemId: string): void {
    this.state.targetItemId = itemId
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

  /** 判断当前手势是否属于指定 scene（路径深度比较） */
  isOwnedByScene(scenePath: StarMapPathSegment[]): boolean {
    const owner = this.state.ownerScenePath
    if (owner.length !== scenePath.length) {
      return false
    }
    for (let i = 0; i < owner.length; i++) {
      if (owner[i].type !== scenePath[i].type ||
        owner[i].instanceId !== scenePath[i].instanceId ||
        owner[i].nodeId !== scenePath[i].nodeId) {
        return false
      }
    }
    return true
  }

  /** 获取当前状态快照（返回只读副本，外部不应修改） */
  getState(): GestureState {
    return {
      mode: this.state.mode,
      activeItemId: this.state.activeItemId,
      activeItemKind: this.state.activeItemKind,
      ownerScenePath: this.state.ownerScenePath.map((seg: StarMapPathSegment): StarMapPathSegment => ({
        type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId
      })),
      startPoint: { x: this.state.startPoint.x, y: this.state.startPoint.y },
      currentPoint: { x: this.state.currentPoint.x, y: this.state.currentPoint.y },
      targetItemId: this.state.targetItemId
    }
  }
}
