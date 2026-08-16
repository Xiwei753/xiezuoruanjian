// KeyboardInputService — 接 HarmonyOS 原始 KeyEvent，转出平台状态。
// 不把 KeyEvent/deviceId 传进 Core；转成编辑命令后由 EditorInputAdapter 委托 Coordinator。
//
// 真实事件源：@kit.InputKit 的 KeyEvent。normalize 从真实 KeyEvent 提取
// keyCode / keyText / isShiftPressed / isCtrlPressed / isAltPressed。
// translateKeyCode 用真实 keyCode 映射成可读 keyText，不是 String(keyCode)。

import { KeyEvent } from '@kit.InputKit'

export interface PlatformKeyEvent {
  keyCode: number
  keyText: string
  isShiftPressed: boolean
  isCtrlPressed: boolean
  isAltPressed: boolean
  timestamp: number
}

// HarmonyOS KeyCode 常量（@ohos.multimodalInput.KeyCode 节选）。
// 完整列表由系统提供；这里覆盖编辑器常用键。
const KEYCODE_SHIFT_LEFT: number = 2001
const KEYCODE_SHIFT_RIGHT: number = 2002
const KEYCODE_CTRL_LEFT: number = 2003
const KEYCODE_CTRL_RIGHT: number = 2004
const KEYCODE_ALT_LEFT: number = 2005
const KEYCODE_ALT_RIGHT: number = 2006
const KEYCODE_ENTER: number = 2052
const KEYCODE_BACKSPACE: number = 2057
const KEYCODE_TAB: number = 2049
const KEYCODE_SPACE: number = 2012
const KEYCODE_DPAD_LEFT: number = 2013
const KEYCODE_DPAD_RIGHT: number = 2014
const KEYCODE_DPAD_UP: number = 2015
const KEYCODE_DPAD_DOWN: number = 2016
const KEYCODE_ESCAPE: number = 2054
const KEYCODE_DELETE: number = 2058
const KEYCODE_HOME: number = 2073
const KEYCODE_END: number = 2074

export class KeyboardInputService {
  private static instance: KeyboardInputService

  static getInstance(): KeyboardInputService {
    if (!KeyboardInputService.instance) {
      KeyboardInputService.instance = new KeyboardInputService()
    }
    return KeyboardInputService.instance
  }

  // 把 HarmonyOS 真实 KeyCode 转成可读 keyText。
  // 覆盖编辑器常用键；未覆盖的回退为 `key:${keyCode}`，明确表示是 keyCode 而非语义字符。
  translateKeyCode(keyCode: number): string {
    switch (keyCode) {
      case KEYCODE_ENTER: return 'Enter'
      case KEYCODE_BACKSPACE: return 'Backspace'
      case KEYCODE_TAB: return 'Tab'
      case KEYCODE_SPACE: return ' '
      case KEYCODE_DPAD_LEFT: return 'ArrowLeft'
      case KEYCODE_DPAD_RIGHT: return 'ArrowRight'
      case KEYCODE_DPAD_UP: return 'ArrowUp'
      case KEYCODE_DPAD_DOWN: return 'ArrowDown'
      case KEYCODE_ESCAPE: return 'Escape'
      case KEYCODE_DELETE: return 'Delete'
      case KEYCODE_HOME: return 'Home'
      case KEYCODE_END: return 'End'
      case KEYCODE_SHIFT_LEFT:
      case KEYCODE_SHIFT_RIGHT: return 'Shift'
      case KEYCODE_CTRL_LEFT:
      case KEYCODE_CTRL_RIGHT: return 'Control'
      case KEYCODE_ALT_LEFT:
      case KEYCODE_ALT_RIGHT: return 'Alt'
      default: return `key:${keyCode}`
    }
  }

  // 接 HarmonyOS 原始 KeyEvent 并归一化为 PlatformKeyEvent。
  // 从真实 KeyEvent 提取 keyCode / keyText / 修饰键状态。
  normalize(rawKeyEvent: KeyEvent): PlatformKeyEvent {
    const keyCode: number = rawKeyEvent.keyCode
    // keyText 优先用系统给的 keyText（字符键有值），否则回退到 translateKeyCode（控制键）。
    const sysText: string = rawKeyEvent.keyText ?? ''
    const keyText: string = sysText.length > 0 ? sysText : this.translateKeyCode(keyCode)
    return {
      keyCode: keyCode,
      keyText: keyText,
      isShiftPressed: rawKeyEvent.isShiftPressed ?? false,
      isCtrlPressed: rawKeyEvent.isCtrlPressed ?? false,
      isAltPressed: rawKeyEvent.isAltPressed ?? false,
      timestamp: Date.now()
    }
  }
}
