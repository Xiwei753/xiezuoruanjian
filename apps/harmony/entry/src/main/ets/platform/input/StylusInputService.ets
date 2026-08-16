// StylusInputService — 接 HarmonyOS Stylus 事件，转出平台状态。
// 笔压、倾斜、笔键等。不把原始事件传进 Core。
//
// 真实事件源：@kit.InputKit 的 TouchEvent（stylus/touchpen 设备来源）。
// normalize 从真实事件提取 pressure / tiltX / tiltY / isEraser。
// HarmonyOS 触摸事件中 TouchPoint 含 force（压力，0..1）、tiltX、tiltY 字段。
// 笔能力由本 service 统一负责（Issue #629 评论5 第6部分）：isSupported()/detectSupport()，
// 不再依赖 InputDeviceService.hasDeviceOfKind('stylus')——InputDeviceService 不再产生 stylus kind。

import { TouchEvent } from '@kit.InputKit'
import { inputDevice } from '@kit.InputKit'

export interface PlatformStylusEvent {
  x: number
  y: number
  pressure: number  // 0..1
  tiltX: number
  tiltY: number
  isEraser: boolean
  timestamp: number
}

// #629 评论3：stylus 不由数值 source 猜测，由 toolType 笔事件独立判断。

// HarmonyOS TouchPoint.toolType（@ohos.multimodalInput.ToolType）。
// TIP=0, ERASER=1。
const TOOL_TYPE_ERASER: number = 1

export class StylusInputService {
  private static instance: StylusInputService

  static getInstance(): StylusInputService {
    if (!StylusInputService.instance) {
      StylusInputService.instance = new StylusInputService()
    }
    return StylusInputService.instance
  }

  // 笔能力缓存。由 detectSupport() 异步刷新，或 normalizeStylusEvent 实际收到笔事件时置 true。
  // 初始 false（保守，未探测过不假装能力存在）。
  private supported: boolean = false
  // 是否已执行过 detectSupport（供调用方判断 isSupported 是否已探测）。
  private detected: boolean = false

  // 当前笔能力状态。CapabilityRegistry.refresh() 读此方法，不自己探测 InputDevice。
  isSupported(): boolean {
    return this.supported
  }

  // 是否已执行过 detectSupport。
  isDetected(): boolean {
    return this.detected
  }

  // 异步探测笔能力：查询 inputDevice 设备列表，看是否有 sources 含 'stylus' 的设备。
  // 刷新 supported 缓存并返回。CapabilityRegistry 初始化时可 await 此方法再 refresh()。
  // 不依赖 InputDeviceService.hasDeviceOfKind('stylus')——InputDeviceService 不再产生 stylus kind。
  async detectSupport(): Promise<boolean> {
    try {
      const ids: Array<number> = (await inputDevice.getDeviceList()) as unknown as Array<number>
      let found: boolean = false
      for (const id of ids) {
        try {
          const raw: inputDevice.InputDeviceData = await inputDevice.getDeviceInfo(id)
          const sources: string[] = raw.sources ?? []
          if (sources.includes('stylus')) {
            found = true
            break
          }
        } catch (e) {
          // 单设备查询失败跳过，不伪造状态。
        }
      }
      this.supported = found
      this.detected = true
      return found
    } catch (e) {
      // getDeviceList 失败；保守置 false，不假装能力存在。
      this.supported = false
      this.detected = true
      return false
    }
  }

  // 接 HarmonyOS 原始 TouchEvent（stylus 来源），归一化为 PlatformStylusEvent。
  // 取第一个 touch point；提取 force / tiltX / tiltY / toolType。
  normalizeStylusEvent(raw: TouchEvent): PlatformStylusEvent {
    // 实际收到笔事件，设备必然支持笔输入。
    this.supported = true
    this.detected = true

    const touches: Touch[] = raw.touches ?? []
    const primary: Touch | undefined = touches.length > 0 ? touches[0] : undefined
    const x: number = primary ? primary.x : 0
    const y: number = primary ? primary.y : 0
    // force 字段范围 0..1；缺失时按"无压感"处理为 0。
    const pressure: number = primary && primary.force !== undefined ? primary.force : 0
    // tiltX / tiltY 字段（弧度或角度，按系统定义直传）。
    const tiltX: number = primary && primary.tiltX !== undefined ? primary.tiltX : 0
    const tiltY: number = primary && primary.tiltY !== undefined ? primary.tiltY : 0
    // isEraser 通过 toolType 判断（ERASER=1）。
    const toolType: number = primary && primary.toolType !== undefined ? primary.toolType : 0
    const isEraser: boolean = toolType === TOOL_TYPE_ERASER
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
  // 实际收到笔事件时也标记 supported=true。
  normalize(rawStylusEvent: Record<string, Object>): PlatformStylusEvent {
    this.supported = true
    this.detected = true
    const toolType: number = (rawStylusEvent['toolType'] ?? 0) as number
    return {
      x: (rawStylusEvent['x'] ?? 0) as number,
      y: (rawStylusEvent['y'] ?? 0) as number,
      pressure: (rawStylusEvent['force'] ?? rawStylusEvent['pressure'] ?? 0) as number,
      tiltX: (rawStylusEvent['tiltX'] ?? 0) as number,
      tiltY: (rawStylusEvent['tiltY'] ?? 0) as number,
      isEraser: toolType === TOOL_TYPE_ERASER,
      timestamp: Date.now()
    }
  }
}
