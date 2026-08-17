// line_navigation_resolver.test.mjs — LineNavigationResolver 纯逻辑单测。
// Issue #629 评论11 第2项：Up/Down/Home/End 行级导航解析器。
// resolver 存带身份的布局状态（revision + displayText + cursorUtf16 + lines），
// executeLineNavigation 出队时验证身份匹配才用 lines 算目标，不匹配返回 STALE_LAYOUT。
//
// 运行：node line_navigation_resolver.test.mjs
//
// 注意：.ets 依赖 ArkUI 无法用 Node 直接测，本测试验证提取的纯逻辑（与
// LineNavigationResolver.ets 对齐）。生产代码需 HarmonyOS SDK 才能端到端编译。

import { strict as assert } from 'node:assert'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// ── 纯逻辑：与 LineNavigationResolver.ets 对齐 ──

class LineNavigationResolver {
  constructor() {
    this.state = null
  }
  updateLayout(state) {
    this.state = state
  }
  matchesIdentity(revision, displayText, cursorUtf16) {
    if (this.state === null) return false
    return this.state.revision === revision
      && this.state.displayText === displayText
      && this.state.cursorUtf16 === cursorUtf16
  }
  getLines() {
    return this.state === null ? [] : this.state.lines
  }
  getCurrentLineIndex(cursorUtf16) {
    const lines = this.getLines()
    if (lines.length === 0) return -1
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i]
      if (cursorUtf16 >= line.startUtf16 && cursorUtf16 <= line.endUtf16) return i
    }
    return lines.length - 1
  }
  getPreviousLineIndex(cursorUtf16) {
    const idx = this.getCurrentLineIndex(cursorUtf16)
    if (idx <= 0) return -1
    return idx - 1
  }
  getNextLineIndex(cursorUtf16) {
    const lines = this.getLines()
    const idx = this.getCurrentLineIndex(cursorUtf16)
    if (idx < 0 || idx >= lines.length - 1) return -1
    return idx + 1
  }
  getLineStart(lineIndex) {
    const lines = this.getLines()
    if (lineIndex < 0 || lineIndex >= lines.length) return 0
    return lines[lineIndex].startUtf16
  }
  getLineEnd(lineIndex) {
    const lines = this.getLines()
    if (lineIndex < 0 || lineIndex >= lines.length) return 0
    return lines[lineIndex].endUtf16
  }
}

// 构造 mock lines：每行 5 个字符
function mockLines(text) {
  const lines = []
  let start = 0
  while (start < text.length) {
    const end = Math.min(start + 5, text.length)
    lines.push({ startUtf16: start, endUtf16: end, y: 0, height: 20 })
    start = end
  }
  return lines
}

console.log('line_navigation_resolver 纯逻辑单测（Issue #629 评论11 第2项）')
console.log('---')

// ── matchesIdentity ──

test('matchesIdentity: state 为 null → false（无布局信息）', () => {
  const r = new LineNavigationResolver()
  assert.equal(r.matchesIdentity(1, 'abc', 0), false)
})

test('matchesIdentity: revision/displayText/cursorUtf16 全匹配 → true', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ revision: 1, displayText: 'abc', cursorUtf16: 2, lines: mockLines('abc') })
  assert.equal(r.matchesIdentity(1, 'abc', 2), true)
})

test('matchesIdentity: revision 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ revision: 1, displayText: 'abc', cursorUtf16: 2, lines: mockLines('abc') })
  assert.equal(r.matchesIdentity(2, 'abc', 2), false)
})

test('matchesIdentity: displayText 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ revision: 1, displayText: 'abc', cursorUtf16: 2, lines: mockLines('abc') })
  assert.equal(r.matchesIdentity(1, 'abd', 2), false)
})

test('matchesIdentity: cursorUtf16 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ revision: 1, displayText: 'abc', cursorUtf16: 2, lines: mockLines('abc') })
  assert.equal(r.matchesIdentity(1, 'abc', 3), false)
})

test('matchesIdentity: composition 活跃时 displayText 含 preedit → 与 committed text 不同 → false', () => {
  const r = new LineNavigationResolver()
  // resolver 存的是含 preedit 的显示文本
  r.updateLayout({ revision: 1, displayText: 'abc你好def', cursorUtf16: 5, lines: mockLines('abc你好def') })
  // executeLineNavigation 用 committed text 'abcdef' 构造 identity → 不匹配
  assert.equal(r.matchesIdentity(1, 'abcdef', 3), false)
  // 用显示文本构造 → 匹配
  assert.equal(r.matchesIdentity(1, 'abc你好def', 5), true)
})

// ── STALE_LAYOUT 场景（executeLineNavigation 出队时验证） ──

test('STALE_LAYOUT: state 为 null → 不算目标', () => {
  const r = new LineNavigationResolver()
  // executeLineNavigation 会调 matchesIdentity，null → false → STALE_LAYOUT
  assert.equal(r.matchesIdentity(1, 'abc', 0), false)
  assert.equal(r.getLines().length, 0)
})

test('STALE_LAYOUT: Core snapshot 已前进而 ArkUI 布局还没刷新 → revision 不匹配', () => {
  const r = new LineNavigationResolver()
  // resolver 存的是 revision=1 的布局
  r.updateLayout({ revision: 1, displayText: 'abc', cursorUtf16: 1, lines: mockLines('abc') })
  // Core snapshot 已前进到 revision=2（用户打了字），但 onAreaChange 还没触发 refreshRenderLayout
  assert.equal(r.matchesIdentity(2, 'abc', 1), false)
})

test('STALE_LAYOUT: displayText 变化（用户输入新字）→ 不匹配', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ revision: 1, displayText: 'hello', cursorUtf16: 2, lines: mockLines('hello') })
  // 用户输入 'x' 后 displayText = 'hexllo'，但布局还是 'hello' 的
  assert.equal(r.matchesIdentity(1, 'hexllo', 3), false)
})

// ── 行级导航（identity 匹配时） ──

test('up: identity 匹配 → 返回上一行行首', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbbccccc'
  r.updateLayout({ revision: 1, displayText: text, cursorUtf16: 7, lines: mockLines(text) })
  assert.equal(r.matchesIdentity(1, text, 7), true)
  // cursor 在第 1 行（bbbbb），上一行是第 0 行（aaaaa），行首 = 0
  const targetLine = r.getPreviousLineIndex(7)
  assert.equal(targetLine, 0)
  assert.equal(r.getLineStart(targetLine), 0)
})

test('down: identity 匹配 → 返回下一行行首', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbbccccc'
  r.updateLayout({ revision: 1, displayText: text, cursorUtf16: 2, lines: mockLines(text) })
  assert.equal(r.matchesIdentity(1, text, 2), true)
  // cursor 在第 0 行，下一行是第 1 行，行首 = 5
  const targetLine = r.getNextLineIndex(2)
  assert.equal(targetLine, 1)
  assert.equal(r.getLineStart(targetLine), 5)
})

test('home: identity 匹配 → 返回当前行行首', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbbccccc'
  r.updateLayout({ revision: 1, displayText: text, cursorUtf16: 7, lines: mockLines(text) })
  assert.equal(r.matchesIdentity(1, text, 7), true)
  // cursor 在第 1 行，行首 = 5
  const lineIdx = r.getCurrentLineIndex(7)
  assert.equal(lineIdx, 1)
  assert.equal(r.getLineStart(lineIdx), 5)
})

test('end: identity 匹配 → 返回当前行行末', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbbccccc'
  r.updateLayout({ revision: 1, displayText: text, cursorUtf16: 7, lines: mockLines(text) })
  assert.equal(r.matchesIdentity(1, text, 7), true)
  // cursor 在第 1 行，行末 = 10
  const lineIdx = r.getCurrentLineIndex(7)
  assert.equal(lineIdx, 1)
  assert.equal(r.getLineEnd(lineIdx), 10)
})

test('up: 已在第一行 → getPreviousLineIndex 返回 -1', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbb'
  r.updateLayout({ revision: 1, displayText: text, cursorUtf16: 2, lines: mockLines(text) })
  assert.equal(r.getPreviousLineIndex(2), -1)
})

test('down: 已在最后一行 → getNextLineIndex 返回 -1', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbb'
  r.updateLayout({ revision: 1, displayText: text, cursorUtf16: 7, lines: mockLines(text) })
  assert.equal(r.getNextLineIndex(7), -1)
})

test('updateLayout(null): 清空 state → matchesIdentity false, getLines 空', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ revision: 1, displayText: 'abc', cursorUtf16: 1, lines: mockLines('abc') })
  r.updateLayout(null)
  assert.equal(r.matchesIdentity(1, 'abc', 1), false)
  assert.equal(r.getLines().length, 0)
  assert.equal(r.getCurrentLineIndex(0), -1)
})

test('中文行布局: identity 含中文 displayText → 匹配时正确算行', () => {
  const r = new LineNavigationResolver()
  const text = '你好世界测试'
  const lines = [{ startUtf16: 0, endUtf16: 3, y: 0, height: 20 }, { startUtf16: 3, endUtf16: 6, y: 20, height: 20 }]
  r.updateLayout({ revision: 2, displayText: text, cursorUtf16: 4, lines })
  assert.equal(r.matchesIdentity(2, text, 4), true)
  // cursor 在第 1 行，up → 第 0 行行首 = 0
  assert.equal(r.getPreviousLineIndex(4), 0)
  assert.equal(r.getLineStart(0), 0)
})

console.log('---')
console.log(`✅ line_navigation_resolver: ${passed} tests passed`)
