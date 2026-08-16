// StylusInputService — 接 HarmonyOS Stylus 事件，转出平台状态。
// 笔压、倾斜、笔键等。不把原始事件传进 Core。
//
// 真实事件源：@kit.InputKit 的 TouchEvent（stylus/touchpen 设备来源）。
// normalize 从真实事件提取 pressure / tiltX / tiltY / isEraser。
// HarmonyOS 触摸事件中 TouchPoint 含 force（压力，0..1）、tiltX、tiltY 字段。
// 笔能力由本 service 统一负责（Issue #629 评论6 第6部分）：
// 三态 unknown/supported/unsupported，不再依赖 InputDeviceService.hasDeviceOfKind('stylus')，
// 也不再从 InputDevice.sources 查 'stylus'（'stylus' 不是真实 SourceType）。

import { TouchEvent, ToolType } from '@kit.InputKit'

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
// - supported：收到过真实笔事件（TouchEvent.toolType === ToolType.TIP / ToolType.ERASER）
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
  // HarmonyOS 没有可靠的主动 API 查询"是否支持笔输入"：Pen 能力从真实 TouchEvent 的
  // toolType 字段识别（toolType === ToolType.TIP / ToolType.ERASER）。
  // 系统只能在真实笔事件到来后确认，所以 detectSupport 不主动置 'supported'，
  // 也不主动置 'unsupported'（没有可靠 API 证明"不支持"）。
  // 若已通过 normalizeStylusEvent 收到过真实笔事件，保持 'supported'；否则保持 'unknown'。
  // CapabilityRegistry 初始化时可 await 此方法再 refresh()。
  async detectSupport(): Promise<StylusCapabilityState> {
    // 不遍历 inputDevice.getDeviceInfo().sources 查 'stylus'（'stylus' 不是真实 SourceType）。
    // 没有 reliable 主动探测 API → 保持当前状态（'unknown' 或 'supported'）。
    return this.capabilityState
  }

  // 接 HarmonyOS 原始 TouchEvent（stylus 来源），归一化为 PlatformStylusEvent。
  // 取第一个 touch point；提取 force / tiltX / tiltY / toolType。
  // 收到真实笔事件时置 capabilityState = 'supported'（设备必然支持笔输入）。
  normalizeStylusEvent(raw: TouchEvent): PlatformStylusEvent {
    const touches: Touch[] = raw.touches ?? []
    const primary: Touch | undefined = touches.length > 0 ? touches[0] : undefined
    const x: number = primary ? primary.x : 0
    const y: number = primary ? primary.y : 0
    // force 字段范围 0..1；缺失时按"无压感"处理为 0。
    const pressure: number = primary && primary.force !== undefined ? primary.force : 0
    // tiltX / tiltY 字段（弧度或角度，按系统定义直传）。
    const tiltX: number = primary && primary.tiltX !== undefined ? primary.tiltX : 0
    const tiltY: number = primary && primary.tiltY !== undefined ? primary.tiltY : 0
    // isEraser 通过官方 ToolType 枚举判断（ToolType.ERASER）。
    const toolType: number = primary && primary.toolType !== undefined ? primary.toolType : 0
    const isEraser: boolean = toolType === ToolType.ERASER
    // 实际收到笔事件，设备必然支持笔输入。
    this.capabilityState = 'supported'
    return {
      x: x,
      y: y,
      pressure: pressure,
      tiltX: tiltX,
      tiltY: tiltY,
      isEraser: isEraser,
      timestamp: Date.now()
    }
  }

  // 兼容旧入口：Record<string, Object> 形式。
  // 实际收到笔事件时也标记 capabilityState = 'supported'。
  normalize(rawStylusEvent: Record<string, Object>): PlatformStylusEvent {
    const toolType: number = (rawStylusEvent['toolType'] ?? 0) as number
    this.capabilityState = 'supported'
    return {
      x: (rawStylusEvent['x'] ?? 0) as number,
      y: (rawStylusEvent['y'] ?? 0) as number,
      pressure: (rawStylusEvent['force'] ?? rawStylusEvent['pressure'] ?? 0) as number,
      tiltX: (rawStylusEvent['tiltX'] ?? 0) as number,
      tiltY: (rawStylusEvent['tiltY'] ?? 0) as number,
      isEraser: toolType === ToolType.ERASER,
      timestamp: Date.now()
    }
  }
}
