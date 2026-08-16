// KeyboardInputService — 接 HarmonyOS 原始 KeyEvent，转出平台状态。
// 不把 KeyEvent/deviceId 传进 Core；转成编辑命令后由 EditorInputAdapter 委托 Coordinator。
//
// 真实事件源：@kit.InputKit 的 KeyEvent。normalize 从真实 KeyEvent 提取
// keyCode / keyText / isShiftPressed / isCtrlPressed / isAltPressed。
// translateKeyCode 用真实 keyCode 映射成可读 keyText，不是 String(keyCode)。
//
// Issue #629 评论 5 第 5 节：toSemanticCommand 把归一化后的 PlatformKeyEvent 翻译成
// 编辑器语义命令（character/Enter/Backspace/Delete/方向/Home/End 等）。
// SujianEditor 接 ArkUI onKeyEvent → normalize → toSemanticCommand → 调
// EditorInputAdapter/SelectionController。原始 KeyEvent 不进 Rust。

import { KeyEvent } from '@kit.InputKit'

export interface PlatformKeyEvent {
  keyCode: number
  keyText: string
  isShiftPressed: boolean
  isCtrlPressed: boolean
  isAltPressed: boolean
  timestamp: number
}

// 语义命令：编辑器输入语义。原始 KeyEvent 不进 Rust，由 SujianEditor 转成语义命令后
// 调 EditorInputAdapter/SelectionController。
// - character: 可打印字符（含空格），text 是 UTF-16 字符串（可能多 code unit，如 emoji）
// - enter: 回车/换行
// - backspace: 退格删除（光标前）
// - delete: 向前删除（光标后）
// - arrowLeft/arrowRight/arrowUp/arrowDown: 方向键，extend=true 表示 shift+方向键扩展选区
// - home/end: 行首/行末，extend=true 表示 shift+Home/End 扩展选区
// - tab: 制表符（这轮当字符插入，不做焦点切换）
// - escape: ESC（这轮忽略，不做取消/退出）
// - ignore: 修饰键单独按下、Ctrl/Alt 组合、无法识别的键 — SujianEditor 不转发
export type SemanticKeyCommand =
  | { kind: 'character'; text: string }
  | { kind: 'enter' }
  | { kind: 'backspace' }
  | { kind: 'delete' }
  | { kind: 'arrowLeft'; extend: boolean }
  | { kind: 'arrowRight'; extend: boolean }
  | { kind: 'arrowUp'; extend: boolean }
  | { kind: 'arrowDown'; extend: boolean }
  | { kind: 'home'; extend: boolean }
  | { kind: 'end'; extend: boolean }
  | { kind: 'tab' }
  | { kind: 'escape' }
  | { kind: 'ignore' }

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

  // 把归一化后的 PlatformKeyEvent 翻译成语义命令。
  // isShiftPressed 决定方向键/Home/End 是否扩展选区（shift+方向键）。
  // 控制键组合（Ctrl/Cmd + 字符）这轮先忽略，不做快捷键绑定
  // （Ctrl+C/V/X 等由 IME/系统剪贴板处理，Backspace/Delete 已有专门语义命令）。
  // 修饰键单独按下（Shift/Ctrl/Alt）返回 ignore。
  toSemanticCommand(ev: PlatformKeyEvent): SemanticKeyCommand {
    const extend: boolean = ev.isShiftPressed
    // Ctrl/Alt 组合这轮不处理（Ctrl+X/C/V 等由系统/IME 处理）。
    if (ev.isCtrlPressed || ev.isAltPressed) {
      return { kind: 'ignore' }
    }
    switch (ev.keyCode) {
      case KEYCODE_ENTER:
        return { kind: 'enter' }
      case KEYCODE_BACKSPACE:
        return { kind: 'backspace' }
      case KEYCODE_DELETE:
        return { kind: 'delete' }
      case KEYCODE_TAB:
        return { kind: 'tab' }
      case KEYCODE_ESCAPE:
        return { kind: 'escape' }
      case KEYCODE_DPAD_LEFT:
        return { kind: 'arrowLeft', extend: extend }
      case KEYCODE_DPAD_RIGHT:
        return { kind: 'arrowRight', extend: extend }
      case KEYCODE_DPAD_UP:
        return { kind: 'arrowUp', extend: extend }
      case KEYCODE_DPAD_DOWN:
        return { kind: 'arrowDown', extend: extend }
      case KEYCODE_HOME:
        return { kind: 'home', extend: extend }
      case KEYCODE_END:
        return { kind: 'end', extend: extend }
      case KEYCODE_SHIFT_LEFT:
      case KEYCODE_SHIFT_RIGHT:
      case KEYCODE_CTRL_LEFT:
      case KEYCODE_CTRL_RIGHT:
      case KEYCODE_ALT_LEFT:
      case KEYCODE_ALT_RIGHT:
        return { kind: 'ignore' }
      default:
        break
    }
    // 字符键：keyText 非空且不是 `key:xxx` 回退形式。
    // Space（keyCode 2012）的 keyText 是 ' '（translateKeyCode 映射）。
    if (ev.keyText.length > 0 && !ev.keyText.startsWith('key:')) {
      return { kind: 'character', text: ev.keyText }
    }
    return { kind: 'ignore' }
  }
}
