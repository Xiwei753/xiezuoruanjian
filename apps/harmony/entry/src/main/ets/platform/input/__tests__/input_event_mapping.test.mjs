// input_event_mapping.test.mjs — 键盘/鼠标事件映射纯逻辑单测。
//
// 验证 Issue #629 评论 5 第 5 节：
//   1. KeyboardInputService.toSemanticCommand: 字符/Enter/Backspace/Delete/方向/Home/End → 语义命令
//   2. PointerInputService.normalizeMouseEvent: 点击/drag/滚轮 action 映射
//   3. 修饰键（Shift/Ctrl/Alt）处理：Shift 扩展选区，Ctrl/Alt 忽略
//   4. 原始平台事件不进 Rust（只产出语义命令）
//
// 运行：node input_event_mapping.test.mjs

import { strict as assert } from 'node:assert'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// ── HarmonyOS KeyCode 常量（与 KeyboardInputService.ets 对齐）──
const KEYCODE_SHIFT_LEFT = 2001
const KEYCODE_SHIFT_RIGHT = 2002
const KEYCODE_CTRL_LEFT = 2003
const KEYCODE_CTRL_RIGHT = 2004
const KEYCODE_ALT_LEFT = 2005
const KEYCODE_ALT_RIGHT = 2006
const KEYCODE_ENTER = 2052
const KEYCODE_BACKSPACE = 2057
const KEYCODE_TAB = 2049
const KEYCODE_SPACE = 2012
const KEYCODE_DPAD_LEFT = 2013
const KEYCODE_DPAD_RIGHT = 2014
const KEYCODE_DPAD_UP = 2015
const KEYCODE_DPAD_DOWN = 2016
const KEYCODE_ESCAPE = 2054
const KEYCODE_DELETE = 2058
const KEYCODE_HOME = 2073
const KEYCODE_END = 2074

// ── 纯逻辑：与 KeyboardInputService.toSemanticCommand 对齐 ──

function translateKeyCode(keyCode) {
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

function normalizeKeyEvent(rawKeyEvent) {
  const keyCode = rawKeyEvent.keyCode
  const sysText = rawKeyEvent.keyText ?? ''
  const keyText = sysText.length > 0 ? sysText : translateKeyCode(keyCode)
  return {
    keyCode,
    keyText,
    isShiftPressed: rawKeyEvent.isShiftPressed ?? false,
    isCtrlPressed: rawKeyEvent.isCtrlPressed ?? false,
    isAltPressed: rawKeyEvent.isAltPressed ?? false,
    timestamp: Date.now(),
  }
}

function toSemanticCommand(ev) {
  const extend = ev.isShiftPressed
  if (ev.isCtrlPressed || ev.isAltPressed) {
    return { kind: 'ignore' }
  }
  switch (ev.keyCode) {
    case KEYCODE_ENTER: return { kind: 'enter' }
    case KEYCODE_BACKSPACE: return { kind: 'backspace' }
    case KEYCODE_DELETE: return { kind: 'delete' }
    case KEYCODE_TAB: return { kind: 'tab' }
    case KEYCODE_ESCAPE: return { kind: 'escape' }
    case KEYCODE_DPAD_LEFT: return { kind: 'arrowLeft', extend }
    case KEYCODE_DPAD_RIGHT: return { kind: 'arrowRight', extend }
    case KEYCODE_DPAD_UP: return { kind: 'arrowUp', extend }
    case KEYCODE_DPAD_DOWN: return { kind: 'arrowDown', extend }
    case KEYCODE_HOME: return { kind: 'home', extend }
    case KEYCODE_END: return { kind: 'end', extend }
    case KEYCODE_SHIFT_LEFT:
    case KEYCODE_SHIFT_RIGHT:
    case KEYCODE_CTRL_LEFT:
    case KEYCODE_CTRL_RIGHT:
    case KEYCODE_ALT_LEFT:
    case KEYCODE_ALT_RIGHT:
      return { kind: 'ignore' }
    default: break
  }
  if (ev.keyText.length > 0 && !ev.keyText.startsWith('key:')) {
    return { kind: 'character', text: ev.keyText }
  }
  return { kind: 'ignore' }
}

// ── 纯逻辑：与 PointerInputService.normalizeMouseEvent 对齐 ──

const MOUSE_BUTTON_LEFT = 0
const MOUSE_BUTTON_RIGHT = 1
const MOUSE_BUTTON_MIDDLE = 2
const MOUSE_ACTION_HOVER = 0
const MOUSE_ACTION_PRESS = 1
const MOUSE_ACTION_RELEASE = 2
const MOUSE_ACTION_MOVE = 3
const MOUSE_ACTION_WHEEL = 4

function mapMouseButton(rawButton) {
  if (rawButton === MOUSE_BUTTON_LEFT || rawButton === MOUSE_BUTTON_RIGHT || rawButton === MOUSE_BUTTON_MIDDLE) {
    return rawButton
  }
  return MOUSE_BUTTON_LEFT
}

function mapMouseAction(rawAction) {
  if (rawAction === MOUSE_ACTION_PRESS) return 'down'
  if (rawAction === MOUSE_ACTION_RELEASE) return 'up'
  if (rawAction === MOUSE_ACTION_WHEEL) return 'wheel'
  return 'move'
}

function normalizeMouseEvent(raw) {
  const action = mapMouseAction(raw.action ?? MOUSE_ACTION_MOVE)
  return {
    x: raw.x ?? 0,
    y: raw.y ?? 0,
    button: mapMouseButton(raw.button ?? MOUSE_BUTTON_LEFT),
    action,
    wheelDeltaX: 0,
    wheelDeltaY: 0,
    timestamp: Date.now(),
  }
}

console.log('键盘/鼠标事件映射纯逻辑单测')
console.log('---')

// ── 1. 字符键 → character ──
test('toSemanticCommand: 字母键 → character', () => {
  const ev = normalizeKeyEvent({ keyCode: 2048, keyText: 'a' })
  const cmd = toSemanticCommand(ev)
  assert.equal(cmd.kind, 'character')
  assert.equal(cmd.text, 'a')
})

test('toSemanticCommand: 数字键 → character', () => {
  const ev = normalizeKeyEvent({ keyCode: 0, keyText: '5' })  // keyCode 0 = 非控制键
  const cmd = toSemanticCommand(ev)
  assert.equal(cmd.kind, 'character')
  assert.equal(cmd.text, '5')
})

test('toSemanticCommand: 空格键 → character (text=" ")', () => {
  const ev = normalizeKeyEvent({ keyCode: KEYCODE_SPACE })
  const cmd = toSemanticCommand(ev)
  assert.equal(cmd.kind, 'character')
  assert.equal(cmd.text, ' ')
})

test('toSemanticCommand: 中文字符 → character', () => {
  const ev = normalizeKeyEvent({ keyCode: 0, keyText: '你' })
  const cmd = toSemanticCommand(ev)
  assert.equal(cmd.kind, 'character')
  assert.equal(cmd.text, '你')
})

test('toSemanticCommand: emoji → character（多 code unit）', () => {
  const ev = normalizeKeyEvent({ keyCode: 0, keyText: '🎉' })
  const cmd = toSemanticCommand(ev)
  assert.equal(cmd.kind, 'character')
  assert.equal(cmd.text, '🎉')
})

// ── 2. 控制键 → 语义命令 ──
test('toSemanticCommand: Enter → enter', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_ENTER }))
  assert.equal(cmd.kind, 'enter')
})

test('toSemanticCommand: Backspace → backspace', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_BACKSPACE }))
  assert.equal(cmd.kind, 'backspace')
})

test('toSemanticCommand: Delete → delete', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_DELETE }))
  assert.equal(cmd.kind, 'delete')
})

test('toSemanticCommand: Tab → tab', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_TAB }))
  assert.equal(cmd.kind, 'tab')
})

test('toSemanticCommand: Escape → escape', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_ESCAPE }))
  assert.equal(cmd.kind, 'escape')
})

// ── 3. 方向键 → arrowXxx（extend=false）──
test('toSemanticCommand: ArrowLeft → arrowLeft extend=false', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_DPAD_LEFT }))
  assert.equal(cmd.kind, 'arrowLeft')
  assert.equal(cmd.extend, false)
})

test('toSemanticCommand: ArrowRight → arrowRight extend=false', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_DPAD_RIGHT }))
  assert.equal(cmd.kind, 'arrowRight')
  assert.equal(cmd.extend, false)
})

test('toSemanticCommand: ArrowUp → arrowUp extend=false', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_DPAD_UP }))
  assert.equal(cmd.kind, 'arrowUp')
  assert.equal(cmd.extend, false)
})

test('toSemanticCommand: ArrowDown → arrowDown extend=false', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_DPAD_DOWN }))
  assert.equal(cmd.kind, 'arrowDown')
  assert.equal(cmd.extend, false)
})

// ── 4. Shift+方向键 → arrowXxx（extend=true）──
test('toSemanticCommand: Shift+ArrowLeft → arrowLeft extend=true', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_DPAD_LEFT, isShiftPressed: true }))
  assert.equal(cmd.kind, 'arrowLeft')
  assert.equal(cmd.extend, true)
})

test('toSemanticCommand: Shift+ArrowRight → arrowRight extend=true', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_DPAD_RIGHT, isShiftPressed: true }))
  assert.equal(cmd.kind, 'arrowRight')
  assert.equal(cmd.extend, true)
})

test('toSemanticCommand: Shift+Home → home extend=true', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_HOME, isShiftPressed: true }))
  assert.equal(cmd.kind, 'home')
  assert.equal(cmd.extend, true)
})

test('toSemanticCommand: Shift+End → end extend=true', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_END, isShiftPressed: true }))
  assert.equal(cmd.kind, 'end')
  assert.equal(cmd.extend, true)
})

// ── 5. Home/End ──
test('toSemanticCommand: Home → home extend=false', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_HOME }))
  assert.equal(cmd.kind, 'home')
  assert.equal(cmd.extend, false)
})

test('toSemanticCommand: End → end extend=false', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_END }))
  assert.equal(cmd.kind, 'end')
  assert.equal(cmd.extend, false)
})

// ── 6. 修饰键单独按下 → ignore ──
test('toSemanticCommand: Shift 单独 → ignore', () => {
  assert.equal(toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_SHIFT_LEFT })).kind, 'ignore')
  assert.equal(toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_SHIFT_RIGHT })).kind, 'ignore')
})

test('toSemanticCommand: Ctrl 单独 → ignore', () => {
  assert.equal(toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_CTRL_LEFT })).kind, 'ignore')
  assert.equal(toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_CTRL_RIGHT })).kind, 'ignore')
})

test('toSemanticCommand: Alt 单独 → ignore', () => {
  assert.equal(toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_ALT_LEFT })).kind, 'ignore')
  assert.equal(toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_ALT_RIGHT })).kind, 'ignore')
})

// ── 7. Ctrl/Alt 组合 → ignore（这轮不做快捷键）──
test('toSemanticCommand: Ctrl+a → ignore', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: 2048, keyText: 'a', isCtrlPressed: true }))
  assert.equal(cmd.kind, 'ignore')
})

test('toSemanticCommand: Ctrl+Enter → ignore', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_ENTER, isCtrlPressed: true }))
  assert.equal(cmd.kind, 'ignore')
})

test('toSemanticCommand: Alt+ArrowLeft → ignore', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_DPAD_LEFT, isAltPressed: true }))
  assert.equal(cmd.kind, 'ignore')
})

// ── 8. 未知 keyCode → ignore（key:xxx 回退形式）──
test('toSemanticCommand: 未知 keyCode（无 keyText）→ ignore', () => {
  const cmd = toSemanticCommand(normalizeKeyEvent({ keyCode: 99999 }))
  assert.equal(cmd.kind, 'ignore')
})

// ── 9. 鼠标事件映射 ──
test('normalizeMouseEvent: PRESS → action=down', () => {
  const ev = normalizeMouseEvent({ x: 100, y: 200, button: 0, action: MOUSE_ACTION_PRESS })
  assert.equal(ev.action, 'down')
  assert.equal(ev.x, 100)
  assert.equal(ev.y, 200)
  assert.equal(ev.button, 0)
})

test('normalizeMouseEvent: RELEASE → action=up', () => {
  const ev = normalizeMouseEvent({ x: 100, y: 200, button: 0, action: MOUSE_ACTION_RELEASE })
  assert.equal(ev.action, 'up')
})

test('normalizeMouseEvent: MOVE → action=move', () => {
  const ev = normalizeMouseEvent({ x: 100, y: 200, button: 0, action: MOUSE_ACTION_MOVE })
  assert.equal(ev.action, 'move')
})

test('normalizeMouseEvent: WHEEL → action=wheel', () => {
  const ev = normalizeMouseEvent({ x: 100, y: 200, button: 0, action: MOUSE_ACTION_WHEEL })
  assert.equal(ev.action, 'wheel')
})

test('normalizeMouseEvent: HOVER → action=move', () => {
  const ev = normalizeMouseEvent({ x: 100, y: 200, button: 0, action: MOUSE_ACTION_HOVER })
  assert.equal(ev.action, 'move')
})

test('normalizeMouseEvent: button 映射 left/right/middle', () => {
  assert.equal(normalizeMouseEvent({ button: 0, action: 0 }).button, 0)
  assert.equal(normalizeMouseEvent({ button: 1, action: 0 }).button, 1)
  assert.equal(normalizeMouseEvent({ button: 2, action: 0 }).button, 2)
})

test('normalizeMouseEvent: 未知 button → 归 left(0)', () => {
  assert.equal(normalizeMouseEvent({ button: 99, action: 0 }).button, 0)
})

test('normalizeMouseEvent: 缺省 x/y → 0', () => {
  const ev = normalizeMouseEvent({ action: 0 })
  assert.equal(ev.x, 0)
  assert.equal(ev.y, 0)
})

// ── 10. 原始事件不进 Rust（只产出语义命令/归一化事件）──
test('语义边界: KeyEvent → PlatformKeyEvent → SemanticKeyCommand（无原始 KeyEvent 字段）', () => {
  const raw = { keyCode: 2048, keyText: 'a', deviceId: 1, isShiftPressed: false, isCtrlPressed: false, isAltPressed: false }
  const platformEv = normalizeKeyEvent(raw)
  // PlatformKeyEvent 不含 deviceId 等原始字段
  assert.equal(platformEv.deviceId, undefined)
  assert.equal(platformEv.keyCode, 2048)
  assert.equal(platformEv.keyText, 'a')

  const cmd = toSemanticCommand(platformEv)
  // SemanticKeyCommand 只含 kind + text/extend，不含原始事件字段
  assert.equal(cmd.kind, 'character')
  assert.equal(cmd.text, 'a')
  assert.equal(cmd.deviceId, undefined)
  assert.equal(cmd.keyCode, undefined)
})

test('语义边界: MouseEvent → PlatformPointerEvent（无原始 MouseEvent 字段）', () => {
  const raw = { x: 100, y: 200, button: 0, action: 1, deviceId: 2, target: {} }
  const ev = normalizeMouseEvent(raw)
  // PlatformPointerEvent 不含 deviceId/target 等原始字段
  assert.equal(ev.deviceId, undefined)
  assert.equal(ev.target, undefined)
  assert.equal(ev.x, 100)
  assert.equal(ev.y, 200)
  assert.equal(ev.action, 'down')
})

// ── 11. 完整键盘输入流程模拟 ──
test('完整流程: 输入 "abc" → 3 个 character 命令', () => {
  const cmds = ['a', 'b', 'c'].map(ch =>
    toSemanticCommand(normalizeKeyEvent({ keyCode: 2048, keyText: ch }))
  )
  assert.equal(cmds[0].kind, 'character')
  assert.equal(cmds[0].text, 'a')
  assert.equal(cmds[1].kind, 'character')
  assert.equal(cmds[1].text, 'b')
  assert.equal(cmds[2].kind, 'character')
  assert.equal(cmds[2].text, 'c')
})

test('完整流程: 输入 "a" → Backspace → 删除', () => {
  const charCmd = toSemanticCommand(normalizeKeyEvent({ keyCode: 2048, keyText: 'a' }))
  const bsCmd = toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_BACKSPACE }))
  assert.equal(charCmd.kind, 'character')
  assert.equal(bsCmd.kind, 'backspace')
})

test('完整流程: 选区扩展 Shift+ArrowRight x3', () => {
  const cmds = [0, 1, 2].map(() =>
    toSemanticCommand(normalizeKeyEvent({ keyCode: KEYCODE_DPAD_RIGHT, isShiftPressed: true }))
  )
  for (const cmd of cmds) {
    assert.equal(cmd.kind, 'arrowRight')
    assert.equal(cmd.extend, true)
  }
})

console.log('---')
console.log(`✅ input_event_mapping: ${passed} tests passed`)
