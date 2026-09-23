// PointerInputService — 接 HarmonyOS 原始 MouseEvent/TouchEvent，转出平台状态。
// 不把 MouseEvent/deviceId 传进 Core。
//
// 真实事件源：ArkUI 组件 .onMouse / .onTouch 回调给的 MouseEvent / TouchEvent
// （component/common.d.ts 全局声明，非 @kit.InputKit）。
// normalize 从真实事件提取 x / y / button / action / wheelDelta。
//
// Issue #629 评论 5 第 5 节：SujianEditor 接 onMouse → normalizeMouseEvent →
// 点击命中/drag selection 走 SelectionController；滚轮只改 viewport。
//
// Issue #629 评论6 第6部分：删除手写 button/action 魔法数字（0/1/2/3/4），
// 直接引用 ArkUI 官方 MouseButton / MouseAction 枚举。
//
// Issue #744 评论 5794965395 第 4 节：用 ArkUI MouseEvent/TouchEvent（非 InputKit），
// 删除 Record/cast/typeof 反射式读字段，删除 normalize(raw: Record) 兼容入口，
// 滚轮按 API26 d.ts 明确字段（ArkUI MouseEvent 无滚轮增量字段，固定返回 0）。

export interface PlatformPointerEvent {
  x: number
  y: number
  button: number  // MouseButton 枚举值：Left=0, Right=1, Middle=2
  action: 'down' | 'up' | 'move' | 'wheel'
  // 滚轮偏移（仅 action==='wheel' 有意义）。向下/向右为正（与 scrollTop/scrollLeft 增量方向一致）。
  // ArkUI MouseEvent 无滚轮增量字段，滚轮由 Scroll 组件自动消费；
  // 此字段供 SujianEditor 在需要手动改 viewport 时使用，extractWheelDelta* 固定返回 0。
  wheelDeltaX: number
  wheelDeltaY: number
  timestamp: number
}

// ArkUI 官方 MouseButton / MouseAction / TouchType 枚举由 component/enums.d.ts 全局声明。
// MouseButton: Left=0, Right=1, Middle=2, Back=3, Forward=4, None=5
// MouseAction: Press=0, Release=1, Move=2, Hover=3, ENTER_WINDOW=4, LEAVE_WINDOW=5, CANCEL=13（无 WHEEL）
// TouchType: Down=0, Up=1, Move=2, Cancel=3
// 直接引用官方枚举，不在本地复制数值表（Issue #629 评论6 第6部分）。

export class PointerInputService {
  private static instance: PointerInputService

  static getInstance(): PointerInputService {
    if (!PointerInputService.instance) {
      PointerInputService.instance = new PointerInputService()
    }
    return PointerInputService.instance
  }

  // 接 ArkUI 原始 MouseEvent，归一化为 PlatformPointerEvent。
  // ArkUI MouseEvent 无滚轮增量字段，滚轮由 Scroll 组件自动消费；
  // extractWheelDelta* 固定返回 0，供手动 viewport 调整时占位用。
  normalizeMouseEvent(raw: MouseEvent): PlatformPointerEvent {
    const action: 'down' | 'up' | 'move' | 'wheel' = this.mapMouseAction(raw.action)
    return {
      x: raw.x,
      y: raw.y,
      button: this.mapMouseButton(raw.button),
      action: action,
      wheelDeltaX: this.extractWheelDeltaX(raw),
      wheelDeltaY: this.extractWheelDeltaY(raw),
      timestamp: raw.timestamp
    }
  }

  // 接 ArkUI 原始 TouchEvent（手指/触控），归一化为 PlatformPointerEvent。
  // 取第一个 touch point；button=MouseButton.Left 表示触控即"左键"。
  normalizeTouchEvent(raw: TouchEvent): PlatformPointerEvent {
    const touches: TouchObject[] = raw.touches
    const primary: TouchObject | undefined = touches.length > 0 ? touches[0] : undefined
    const x: number = primary ? primary.x : 0
    const y: number = primary ? primary.y : 0
    return {
      x: x,
      y: y,
      button: MouseButton.Left,
      action: this.mapTouchAction(raw.type),
      wheelDeltaX: 0,
      wheelDeltaY: 0,
      timestamp: raw.timestamp
    }
  }

  // —— private ——
  // 用官方 MouseButton 枚举值比较，不硬编码数字。
  private mapMouseButton(rawButton: MouseButton): number {
    if (rawButton === MouseButton.Left || rawButton === MouseButton.Right || rawButton === MouseButton.Middle) {
      return rawButton
    }
    return MouseButton.Left
  }

  // 用官方 MouseAction 枚举值比较，不硬编码数字。
  // ArkUI MouseAction 无 WHEEL 值（滚轮由 Scroll 组件自动消费），故不返回 'wheel'。
  private mapMouseAction(rawAction: MouseAction): 'down' | 'up' | 'move' {
    if (rawAction === MouseAction.Press) {
      return 'down'
    }
    if (rawAction === MouseAction.Release) {
      return 'up'
    }
    // Hover / Move / ENTER_WINDOW / LEAVE_WINDOW / CANCEL / 未知都归 move。
    return 'move'
  }

  private mapTouchAction(rawAction: TouchType): 'down' | 'up' | 'move' {
    if (rawAction === TouchType.Down) {
      return 'down'
    }
    if (rawAction === TouchType.Up) {
      return 'up'
    }
    if (rawAction === TouchType.Move) {
      return 'move'
    }
    // Cancel 归 move（保守不丢位置）。
    return 'move'
  }

  // ArkUI MouseEvent 无滚轮增量字段；滚轮由 Scroll 组件自动消费。
  private extractWheelDeltaX(_raw: MouseEvent): number {
    return 0
  }

  private extractWheelDeltaY(_raw: MouseEvent): number {
    return 0
  }
}
