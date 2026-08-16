// PointerInputService — 接 HarmonyOS 原始 MouseEvent/TouchEvent，转出平台状态。
// 不把 MouseEvent/deviceId 传进 Core。
//
// 真实事件源：@kit.InputKit 的 MouseEvent / TouchEvent。
// normalize 从真实事件提取 x / y / button / action / wheelDelta。
//
// Issue #629 评论 5 第 5 节：SujianEditor 接 onMouse → normalizeMouseEvent →
// 点击命中/drag selection 走 SelectionController；滚轮只改 viewport。
//
// Issue #629 评论6 第6部分：删除手写 button/action 魔法数字（0/1/2/3/4），
// 直接引用 Harmony Input Kit 官方 MouseButton / MouseAction 枚举。

import { MouseEvent, TouchEvent, Action, MouseButton, MouseAction } from '@kit.InputKit'

export interface PlatformPointerEvent {
  x: number
  y: number
  button: number  // MouseButton 枚举值：BUTTON_LEFT=0, BUTTON_RIGHT=1, BUTTON_MIDDLE=2
  action: 'down' | 'up' | 'move' | 'wheel'
  // 滚轮偏移（仅 action==='wheel' 有意义）。向下/向右为正（与 scrollTop/scrollLeft 增量方向一致）。
  // HarmonyOS MouseEvent 滚轮事件的增量由 ArkUI 框架消费（Scroll 组件自动滚动），
  // 此字段供 SujianEditor 在需要手动改 viewport 时使用；normalize 从 MouseEvent 提取（缺失时 0）。
  wheelDeltaX: number
  wheelDeltaY: number
  timestamp: number
}

// HarmonyOS 官方 MouseButton / MouseAction 枚举由 @kit.InputKit 提供
// （@ohos.multimodalInput.MouseButton / MouseAction）。
// MouseButton: BUTTON_LEFT=0, BUTTON_RIGHT=1, BUTTON_MIDDLE=2, ...
// MouseAction: HOVER=0, PRESS=1, RELEASE=2, MOVE=3, WHEEL=4
// 直接引用官方枚举，不在本地复制数值表（Issue #629 评论6 第6部分）。

export class PointerInputService {
  private static instance: PointerInputService

  static getInstance(): PointerInputService {
    if (!PointerInputService.instance) {
      PointerInputService.instance = new PointerInputService()
    }
    return PointerInputService.instance
  }

  // 接 HarmonyOS 原始 MouseEvent，归一化为 PlatformPointerEvent。
  // 滚轮增量从 MouseEvent 的 scroll/axis 字段提取（HarmonyOS MouseEvent 在 wheel action 时
  // 通过 ArkUI Scroll 组件自动消费；此处提取供手动 viewport 调整用，缺失时 0）。
  normalizeMouseEvent(raw: MouseEvent): PlatformPointerEvent {
    const action: 'down' | 'up' | 'move' | 'wheel' = this.mapMouseAction(raw.action ?? MouseAction.MOVE)
    return {
      x: raw.x ?? 0,
      y: raw.y ?? 0,
      button: this.mapMouseButton(raw.button ?? MouseButton.BUTTON_LEFT),
      action: action,
      wheelDeltaX: this.extractWheelDeltaX(raw),
      wheelDeltaY: this.extractWheelDeltaY(raw),
      timestamp: Date.now()
    }
  }

  // 接 HarmonyOS 原始 TouchEvent（手指/触控），归一化为 PlatformPointerEvent。
  // 取第一个 touch point；button=MouseButton.BUTTON_LEFT 表示触控即"左键"。
  normalizeTouchEvent(raw: TouchEvent): PlatformPointerEvent {
    const touches: Touch[] = raw.touches ?? []
    const primary: Touch | undefined = touches.length > 0 ? touches[0] : undefined
    const x: number = primary ? primary.x : 0
    const y: number = primary ? primary.y : 0
    return {
      x: x,
      y: y,
      button: MouseButton.BUTTON_LEFT,
      action: this.mapTouchAction(raw.action ?? Action.DOWN),
      wheelDeltaX: 0,
      wheelDeltaY: 0,
      timestamp: Date.now()
    }
  }

  // 兼容旧入口：Record<string, Object> 形式（保留给未类型化调用方）。
  normalize(rawMouseEvent: Record<string, Object>): PlatformPointerEvent {
    return {
      x: (rawMouseEvent['x'] ?? 0) as number,
      y: (rawMouseEvent['y'] ?? 0) as number,
      button: this.mapMouseButton((rawMouseEvent['button'] ?? MouseButton.BUTTON_LEFT) as number),
      action: this.mapMouseAction((rawMouseEvent['action'] ?? MouseAction.MOVE) as number),
      wheelDeltaX: (rawMouseEvent['wheelDeltaX'] ?? 0) as number,
      wheelDeltaY: (rawMouseEvent['wheelDeltaY'] ?? 0) as number,
      timestamp: Date.now()
    }
  }

  // —— private ——
  // 用官方 MouseButton 枚举值比较，不硬编码数字。
  private mapMouseButton(rawButton: number): number {
    if (rawButton === MouseButton.BUTTON_LEFT || rawButton === MouseButton.BUTTON_RIGHT || rawButton === MouseButton.BUTTON_MIDDLE) {
      return rawButton
    }
    return MouseButton.BUTTON_LEFT
  }

  // 用官方 MouseAction 枚举值比较，不硬编码数字。
  private mapMouseAction(rawAction: number): 'down' | 'up' | 'move' | 'wheel' {
    if (rawAction === MouseAction.PRESS) {
      return 'down'
    }
    if (rawAction === MouseAction.RELEASE) {
      return 'up'
    }
    if (rawAction === MouseAction.WHEEL) {
      return 'wheel'
    }
    // HOVER / MOVE / 未知都归 move。
    return 'move'
  }

  private mapTouchAction(rawAction: Action): 'down' | 'up' | 'move' | 'wheel' {
    if (rawAction === Action.DOWN) {
      return 'down'
    }
    if (rawAction === Action.UP) {
      return 'up'
    }
    if (rawAction === Action.MOVE) {
      return 'move'
    }
    // CANCEL / UNKNOWN 等归 move（保守不丢位置）。
    return 'move'
  }

  // 从 MouseEvent 提取滚轮 X 增量。HarmonyOS MouseEvent 滚轮增量字段名因 SDK 版本而异，
  // 此处保守提取：若 raw 含 scrollX/axisX 则用，否则 0（Scroll 组件已自动消费滚轮）。
  private extractWheelDeltaX(raw: MouseEvent): number {
    const v: Object | undefined = (raw as Record<string, Object>)['scrollX']
    if (v !== undefined && typeof v === 'number') {
      return v as number
    }
    return 0
  }

  private extractWheelDeltaY(raw: MouseEvent): number {
    const v: Object | undefined = (raw as Record<string, Object>)['scrollY']
    if (v !== undefined && typeof v === 'number') {
      return v as number
    }
    return 0
  }
}
