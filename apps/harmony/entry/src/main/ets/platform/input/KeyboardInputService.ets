// KeyboardInputService — 接 HarmonyOS 原始 KeyEvent，转出平台状态。
// 不把 KeyEvent/deviceId 传进 Core；转成编辑命令后由 EditorInputAdapter 委托 Coordinator。

export interface PlatformKeyEvent {
  keyCode: number
  keyText: string
  isShiftPressed: boolean
  isCtrlPressed: boolean
  isAltPressed: boolean
  timestamp: number
}

export class KeyboardInputService {
  private static instance: KeyboardInputService

  static getInstance(): KeyboardInputService {
    if (!KeyboardInputService.instance) {
      KeyboardInputService.instance = new KeyboardInputService()
    }
    return KeyboardInputService.instance
  }

  // 把 HarmonyOS KeyCode 转成可读 keyText（占位，实际由 @ohos.multimodalInput keyCode 映射）。
  translateKeyCode(keyCode: number): string {
    return String(keyCode)
  }

  // 接原始 KeyEvent 并归一化为 PlatformKeyEvent。
  normalize(rawKeyEvent: Record<string, Object>): PlatformKeyEvent {
    return {
      keyCode: rawKeyEvent['keyCode'] as number,
      keyText: this.translateKeyCode(rawKeyEvent['keyCode'] as number),
      isShiftPressed: false,
      isCtrlPressed: false,
      isAltPressed: false,
      timestamp: Date.now()
    }
  }
}
