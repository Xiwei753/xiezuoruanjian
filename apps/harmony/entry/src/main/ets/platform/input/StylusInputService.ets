// StylusInputService — 接 HarmonyOS Stylus 事件，转出平台状态。
// 笔压、倾斜、笔键等。不把原始事件传进 Core。
//
// 真实事件源：ArkUI 组件 TouchEvent（.onTouch 回调传入的全局 TouchEvent）。
// normalizeStylusEvent 从真实事件提取 pressure / tiltX / tiltY / isEraser。
// ArkUI TouchEvent 继承 BaseEvent，事件级含 pressure（0..1）、tiltX、tiltY、sourceTool、timestamp；
// touches[0].x / y 是组件坐标系坐标。
// 笔能力由本 service 统一负责（Issue #629 评论6 第6部分）：
// 三态 unknown/supported/unsupported，不再依赖 InputDeviceService.hasDeviceOfKind('stylus')，
// 也不再从 InputDevice.sources 查 'stylus'（'stylus' 不是真实 SourceType）。

export interface PlatformStylusEvent {
  x: number
  y: number
  pressure: number  // 0..1
  tiltX: number
  tiltY: number
  isEraser: boolean
  timestamp: number
}

// 笔能力三态（Issue #629 评论6 第6部分）：
// - unknown：未探测过，或系统只能在真实笔事件到来后确认（HarmonyOS 无主动 Pen 能力查询 API）
// - supported：收到过真实笔事件（event.sourceTool === SourceTool.Pen；当前 SDK SourceTool 只有 Pen 是笔类值）
// - unsupported：明确确认不支持（这轮无可靠主动 API 设此态，保留给未来 Pen Kit 探测）
//
// 不再从 InputDevice.sources 查 'stylus'——'stylus' 不是真实 SourceType
// （真实的是 keyboard/mouse/touchpad/touchscreen/joystick/trackball）。
export type StylusCapabilityState = 'unknown' | 'supported' | 'unsupported'

export class StylusInputService {
  private static instance: StylusInputService

  static getInstance(): StylusInputService {
    if (!StylusInputService.instance) {
      StylusInputService.instance = new StylusInputService()
    }
    return StylusInputService.instance
  }

  // 笔能力三态缓存。初始 'unknown'（保守，未探测过不假装能力存在也不假装不存在）。
  // normalizeStylusEvent 收到真实笔事件时置 'supported'。
  // detectSupport 不查 sources.includes('stylus')（'stylus' 不是真实 SourceType），
  // HarmonyOS 无主动 Pen 能力查询 API，系统只能在真实笔事件到来后确认，所以保持 'unknown'。
  private capabilityState: StylusCapabilityState = 'unknown'

  // 当前笔能力状态（三态）。CapabilityRegistry.refresh() 读此方法，不自己探测 InputDevice。
  // 不再依赖 InputDeviceService.hasDeviceOfKind('stylus')——InputDeviceService 不再产生 stylus kind。
  getCapabilityState(): StylusCapabilityState {
    return this.capabilityState
  }

  // 兼容旧入口：返回是否已确认支持笔输入。
  // 仅在 getCapabilityState() === 'supported' 时 true；
  // 'unknown' / 'unsupported' 都返回 false（不假装能力存在）。
  isSupported(): boolean {
    return this.capabilityState === 'supported'
  }

  // 是否已脱离 'unknown'（已确认 supported 或 unsupported）。
  // 三态语义下：'supported' / 'unsupported' 都算已确认；'unknown' 算未确认。
  isDetected(): boolean {
    return this.capabilityState !== 'unknown'
  }

  // 异步探测笔能力。
  // 不查 sources.includes('stylus')——'stylus' 不是真实 SourceType（真实的是
  // keyboard/mouse/touchpad/touchscreen/joystick/trackball）。
  // HarmonyOS 没有可靠的主动 API 查询"是否支持笔输入"：Pen 能力从真实 ArkUI TouchEvent 的
  // sourceTool 字段识别（event.sourceTool === SourceTool.Pen；当前 SDK SourceTool 只有 Pen 是笔类值）。
  // 系统只能在真实笔事件到来后确认，所以 detectSupport 不主动置 'supported'，
  // 也不主动置 'unsupported'（没有可靠 API 证明"不支持"）。
  // 若已通过 normalizeStylusEvent 收到过真实笔事件，保持 'supported'；否则保持 'unknown'。
  // CapabilityRegistry 初始化时可 await 此方法再 refresh()。
  async detectSupport(): Promise<StylusCapabilityState> {
    // 不遍历 inputDevice.getDeviceInfo().sources 查 'stylus'（'stylus' 不是真实 SourceType）。
    // 没有 reliable 主动探测 API → 保持当前状态（'unknown' 或 'supported'）。
    return this.capabilityState
  }

  // 唯一的笔工具判断入口。统一按 ArkUI event.sourceTool 判断，不再读 touches[0].toolType，也不写魔法数字。
  // Issue #743 要求使用 ArkUI SourceTool 枚举识别笔类工具：Pen / Rubber / Brush / Pencil / Airbrush。
  // 当前 SDK 的 SourceTool 枚举只有 Pen 是笔类值（Unknown/Finger/MOUSE/TOUCHPAD/JOYSTICK 都不是笔）；
  // Rubber/Brush/Pencil/Airbrush 尚未在 SourceTool 中定义（它们是 @kit.InputKit ToolType 的值，
  // 但本轮已删除 @kit.InputKit 依赖）。若未来 SDK 扩展 SourceTool，需在此扩展判断。
  // 手指等其他来源返回 false。
  isStylusEvent(event: TouchEvent): boolean {
    return event.sourceTool === SourceTool.Pen
  }

  // 接 ArkUI 组件 TouchEvent（.onTouch 传来），归一化为 PlatformStylusEvent。
  // 非笔事件（isStylusEvent false）或没有 touch point 时返回 null。
  // 坐标取 event.touches[0].x/y；压力、倾斜从事件级 event.pressure / event.tiltX / event.tiltY 读取；
  // 时间戳用 event.timestamp（ns）。
  // 只有收到真实笔事件后才把 capabilityState 置为 'supported'。
  normalizeStylusEvent(event: TouchEvent): PlatformStylusEvent | null {
    if (!this.isStylusEvent(event)) {
      return null
    }
    const touches: TouchObject[] = event.touches ?? []
    if (touches.length === 0) {
      return null
    }
    const primary: TouchObject = touches[0]
    // 橡皮擦：Issue #743 要求按 event.sourceTool === SourceTool.Rubber 判断。
    // 当前 SDK 的 SourceTool 枚举没有 Rubber 值（只有 Unknown/Finger/Pen/MOUSE/TOUCHPAD/JOYSTICK），
    // 因此当前无法通过 sourceTool 区分橡皮擦。isEraser 暂为 false，待 SDK 扩展 SourceTool 后补充。
    const isEraser: boolean = false
    // 实际收到笔事件，设备必然支持笔输入。
    this.capabilityState = 'supported'
    return {
      x: primary.x,
      y: primary.y,
      pressure: event.pressure,
      tiltX: event.tiltX,
      tiltY: event.tiltY,
      isEraser: isEraser,
      timestamp: event.timestamp
    }
  }
}
