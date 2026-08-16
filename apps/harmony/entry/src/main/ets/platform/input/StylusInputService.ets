// StylusInputService — 接 HarmonyOS Stylus 事件，转出平台状态。
// 笔压、倾斜、笔键等。不把原始事件传进 Core。
//
// 真实事件源：@kit.InputKit 的 TouchEvent（stylus/touchpen 设备来源）。
// normalize 从真实事件提取 pressure / tiltX / tiltY / isEraser。
// HarmonyOS 触摸事件中 TouchPoint 含 force（压力，0..1）、tiltX、tiltY 字段；
// stylus 来源设备 id 在 InputDeviceService 中标 kind='stylus'。

import { TouchEvent, Action, SourceType } from '@kit.InputKit'

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

  // 接 HarmonyOS 原始 TouchEvent（stylus 来源），归一化为 PlatformStylusEvent。
  // 取第一个 touch point；提取 force / tiltX / tiltY / toolType。
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
  normalize(rawStylusEvent: Record<string, Object>): PlatformStylusEvent {
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
