// PointerInputService — 接 HarmonyOS 原始 MouseEvent/TouchEvent，转出平台状态。
// 不把 MouseEvent/deviceId 传进 Core。
//
// 真实事件源：@kit.InputKit 的 MouseEvent / TouchEvent。
// normalize 从真实事件提取 x / y / button / action。

import { MouseEvent, TouchEvent, Action } from '@kit.InputKit'

export interface PlatformPointerEvent {
  x: number
  y: number
  button: number  // 0=left, 1=right, 2=middle
  action: 'down' | 'up' | 'move' | 'wheel'
  timestamp: number
}

// HarmonyOS MouseEvent.button 取值（@ohos.multimodalInput.MouseEvent）。
const MOUSE_BUTTON_LEFT: number = 0
const MOUSE_BUTTON_RIGHT: number = 1
const MOUSE_BUTTON_MIDDLE: number = 2

// HarmonyOS MouseAction 取值（@ohos.multimodalInput.MouseAction）。
const MOUSE_ACTION_HOVER: number = 0
const MOUSE_ACTION_PRESS: number = 1
const MOUSE_ACTION_RELEASE: number = 2
const MOUSE_ACTION_MOVE: number = 3
const MOUSE_ACTION_WHEEL: number = 4

export class PointerInputService {
  private static instance: PointerInputService

  static getInstance(): PointerInputService {
    if (!PointerInputService.instance) {
      PointerInputService.instance = new PointerInputService()
    }
    return PointerInputService.instance
  }

  // 接 HarmonyOS 原始 MouseEvent，归一化为 PlatformPointerEvent。
  normalizeMouseEvent(raw: MouseEvent): PlatformPointerEvent {
    return {
      x: raw.x ?? 0,
      y: raw.y ?? 0,
      button: this.mapMouseButton(raw.button ?? MOUSE_BUTTON_LEFT),
      action: this.mapMouseAction(raw.action ?? MOUSE_ACTION_MOVE),
      timestamp: Date.now()
    }
  }

  // 接 HarmonyOS 原始 TouchEvent（手指/触控），归一化为 PlatformPointerEvent。
  // 取第一个 touch point；button=0 表示触控即"左键"。
  normalizeTouchEvent(raw: TouchEvent): PlatformPointerEvent {
    const touches: Touch[] = raw.touches ?? []
    const primary: Touch | undefined = touches.length > 0 ? touches[0] : undefined
    const x: number = primary ? primary.x : 0
    const y: number = primary ? primary.y : 0
    return {
      x: x,
      y: y,
      button: MOUSE_BUTTON_LEFT,
      action: this.mapTouchAction(raw.action ?? Action.DOWN),
      timestamp: Date.now()
    }
  }

  // 兼容旧入口：Record<string, Object> 形式（保留给未类型化调用方）。
  normalize(rawMouseEvent: Record<string, Object>): PlatformPointerEvent {
    return {
      x: (rawMouseEvent['x'] ?? 0) as number,
      y: (rawMouseEvent['y'] ?? 0) as number,
      button: this.mapMouseButton((rawMouseEvent['button'] ?? MOUSE_BUTTON_LEFT) as number),
      action: this.mapMouseAction((rawMouseEvent['action'] ?? MOUSE_ACTION_MOVE) as number),
      timestamp: Date.now()
    }
  }

  // —— private ——
  private mapMouseButton(rawButton: number): number {
    if (rawButton === MOUSE_BUTTON_LEFT || rawButton === MOUSE_BUTTON_RIGHT || rawButton === MOUSE_BUTTON_MIDDLE) {
      return rawButton
    }
    return MOUSE_BUTTON_LEFT
  }

  private mapMouseAction(rawAction: number): 'down' | 'up' | 'move' | 'wheel' {
    if (rawAction === MOUSE_ACTION_PRESS) {
      return 'down'
    }
    if (rawAction === MOUSE_ACTION_RELEASE) {
      return 'up'
    }
    if (rawAction === MOUSE_ACTION_WHEEL) {
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
}
