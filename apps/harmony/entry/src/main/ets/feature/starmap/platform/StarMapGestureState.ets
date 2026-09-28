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

/** 手势模式枚举 */
export type StarMapGestureMode = 'idle' | 'panCanvas' | 'nodeMenu' | 'connect' | 'moveNode' | 'moveEmbed'

/** 触屏状态数据接口 */
export interface GestureState {
  /** 当前手势模式 */
  mode: StarMapGestureMode
  /** 当前操作的节点 ID（空字符串表示无） */
  activeNodeId: string
  /** 手势起点（画布坐标） */
  startPoint: PlatformPoint
  /** 当前点（画布坐标） */
  currentPoint: PlatformPoint
  /** connect 模式下目标节点 ID */
  targetNodeId: string
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
    activeNodeId: '',
    startPoint: { x: 0, y: 0 },
    currentPoint: { x: 0, y: 0 },
    targetNodeId: ''
  }

  /** 回到 idle，清空所有状态 */
  reset(): void {
    this.state = {
      mode: 'idle',
      activeNodeId: '',
      startPoint: { x: 0, y: 0 },
      currentPoint: { x: 0, y: 0 },
      targetNodeId: ''
    }
  }

  /** 进入画布拖动模式 */
  beginPanCanvas(startX: number, startY: number): void {
    this.state = {
      mode: 'panCanvas',
      activeNodeId: '',
      startPoint: { x: startX, y: startY },
      currentPoint: { x: startX, y: startY },
      targetNodeId: ''
    }
  }

  /** 长按节点弹出菜单 */
  beginNodeMenu(nodeId: string, startX: number, startY: number): void {
    this.state = {
      mode: 'nodeMenu',
      activeNodeId: nodeId,
      startPoint: { x: startX, y: startY },
      currentPoint: { x: startX, y: startY },
      targetNodeId: ''
    }
  }

  /** 从菜单态切到连线模式 */
  beginConnect(nodeId: string, startX: number, startY: number): void {
    this.state = {
      mode: 'connect',
      activeNodeId: nodeId,
      startPoint: { x: startX, y: startY },
      currentPoint: { x: startX, y: startY },
      targetNodeId: ''
    }
  }

  /** 从菜单显式进入移动节点模式 */
  beginMoveNode(nodeId: string, startX: number, startY: number): void {
    this.state = {
      mode: 'moveNode',
      activeNodeId: nodeId,
      startPoint: { x: startX, y: startY },
      currentPoint: { x: startX, y: startY },
      targetNodeId: ''
    }
  }

  /** 从菜单显式进入移动 Embed 模式 */
  beginMoveEmbed(embedInstanceId: string, startX: number, startY: number): void {
    this.state = {
      mode: 'moveEmbed',
      activeNodeId: embedInstanceId,
      startPoint: { x: startX, y: startY },
      currentPoint: { x: startX, y: startY },
      targetNodeId: ''
    }
  }

  /** 更新当前点（画布坐标） */
  updateCurrent(x: number, y: number): void {
    this.state.currentPoint = { x, y }
  }

  /** connect 模式下设置目标节点 */
  setTargetNode(nodeId: string): void {
    this.state.targetNodeId = nodeId
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

  /** 获取当前状态快照（返回只读副本，外部不应修改） */
  getState(): GestureState {
    return {
      mode: this.state.mode,
      activeNodeId: this.state.activeNodeId,
      startPoint: { x: this.state.startPoint.x, y: this.state.startPoint.y },
      currentPoint: { x: this.state.currentPoint.x, y: this.state.currentPoint.y },
      targetNodeId: this.state.targetNodeId
    }
  }
}
