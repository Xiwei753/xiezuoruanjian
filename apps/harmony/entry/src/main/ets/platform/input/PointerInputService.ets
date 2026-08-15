// PointerInputService — 接 HarmonyOS 原始 MouseEvent/TouchEvent，转出平台状态。
// 不把 MouseEvent/deviceId 传进 Core。

export interface PlatformPointerEvent {
  x: number
  y: number
  button: number  // 0=left, 1=right, 2=middle
  action: 'down' | 'up' | 'move' | 'wheel'
  timestamp: number
}

export class PointerInputService {
  private static instance: PointerInputService

  static getInstance(): PointerInputService {
    if (!PointerInputService.instance) {
      PointerInputService.instance = new PointerInputService()
    }
    return PointerInputService.instance
  }

  normalize(rawMouseEvent: Record<string, Object>): PlatformPointerEvent {
    return {
      x: rawMouseEvent['x'] as number,
      y: rawMouseEvent['y'] as number,
      button: 0,
      action: 'move',
      timestamp: Date.now()
    }
  }
}
