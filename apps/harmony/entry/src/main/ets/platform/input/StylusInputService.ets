// StylusInputService — 接 HarmonyOS Stylus 事件，转出平台状态。
// 笔压、倾斜、笔键等。不把原始事件传进 Core。

export interface PlatformStylusEvent {
  x: number
  y: number
  pressure: number  // 0..1
  tiltX: number
  tiltY: number
  isEraser: boolean
  timestamp: number
}

export class StylusInputService {
  private static instance: StylusInputService

  static getInstance(): StylusInputService {
    if (!StylusInputService.instance) {
      StylusInputService.instance = new StylusInputService()
    }
    return StylusInputService.instance
  }

  normalize(rawStylusEvent: Record<string, Object>): PlatformStylusEvent {
    return {
      x: rawStylusEvent['x'] as number,
      y: rawStylusEvent['y'] as number,
      pressure: 0,
      tiltX: 0,
      tiltY: 0,
      isEraser: false,
      timestamp: Date.now()
    }
  }
}
