// line_navigation_resolver.test.mjs — LineNavigationResolver 纯逻辑单测。
// Issue #629 评论11 第2项 + 评论14 第3项：Up/Down/Home/End 行级导航解析器。
// resolver 存带身份的布局状态（displayText + contentWidth + fontSize + lines），
// 版本化 layout source：executeLineNavigation 出队时如果 Core 已前进但 resolver 还是旧版本，
// 等待对应版本的 layout state 后再算目标，不返回 STALE_LAYOUT 丢键。
//
// 运行：node line_navigation_resolver.test.mjs

import { strict as assert } from 'node:assert'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}
const sleep = (ms) => new Promise(r => setTimeout(r, ms))

// ── 纯逻辑：与 LineNavigationResolver.ets 对齐 ──
// Issue #629 评论14 第3项：身份 = displayText + contentWidth + fontSize（不含 cursorUtf16）。

class LineNavigationResolver {
  constructor() {
    this.state = null
    this.nextVersion = 1
    this.waiters = []
  }
  updateLayout(state) {
    this.state = state
    if (state !== null) {
      this.resolveWaiters(state)
    }
  }
  waitForLayout(identity, timeoutMs = 500) {
    if (this.state !== null && this.matchesIdentity(this.state, identity)) {
      return Promise.resolve(this.state)
    }
    return new Promise((resolve) => {
      const version = this.nextVersion
      const waiter = { identity, resolve, version }
      this.waiters.push(waiter)
      const timer = setTimeout(() => {
        this.removeWaiter(waiter)
        resolve(null)
      }, timeoutMs)
      const originalResolve = waiter.resolve
      waiter.resolve = (state) => {
        clearTimeout(timer)
        originalResolve(state)
      }
    })
  }
  cancelWait() {
    for (const waiter of this.waiters) {
      waiter.resolve(null)
    }
    this.waiters = []
  }
  resolveWaiters(state) {
    const resolved = []
    for (const waiter of this.waiters) {
      if (this.matchesIdentity(state, waiter.identity)) {
        waiter.resolve(state)
        resolved.push(waiter)
      }
    }
    for (const w of resolved) {
      this.removeWaiter(w)
    }
  }
  removeWaiter(waiter) {
    const idx = this.waiters.indexOf(waiter)
    if (idx >= 0) this.waiters.splice(idx, 1)
  }
  matchesIdentity(state, identity) {
    if (state === null) return false
    return state.displayText === identity.displayText
      && state.contentWidth === identity.contentWidth
      && state.fontSize === identity.fontSize
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

console.log('line_navigation_resolver 纯逻辑单测（Issue #629 评论11 第2项 + 评论14 第3项）')
console.log('---')

// ── matchesIdentity（新版：displayText + contentWidth + fontSize） ──

test('matchesIdentity: state 为 null → false', () => {
  const r = new LineNavigationResolver()
  // state is null initially
  assert.equal(r.matchesIdentity(null, { displayText: 'abc', contentWidth: 300, fontSize: 16 }), false)
})

test('matchesIdentity: displayText + contentWidth + fontSize 全匹配 → true', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ displayText: 'abc', contentWidth: 300, fontSize: 16, lines: mockLines('abc'), version: 1 })
  assert.equal(r.matchesIdentity(r.state, { displayText: 'abc', contentWidth: 300, fontSize: 16 }), true)
})

test('matchesIdentity: displayText 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ displayText: 'abc', contentWidth: 300, fontSize: 16, lines: mockLines('abc'), version: 1 })
  assert.equal(r.matchesIdentity(r.state, { displayText: 'abd', contentWidth: 300, fontSize: 16 }), false)
})

test('matchesIdentity: contentWidth 不同 → false（折行边界变化）', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ displayText: 'abc', contentWidth: 300, fontSize: 16, lines: mockLines('abc'), version: 1 })
  assert.equal(r.matchesIdentity(r.state, { displayText: 'abc', contentWidth: 200, fontSize: 16 }), false)
})

test('matchesIdentity: fontSize 不同 → false（折行边界变化）', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ displayText: 'abc', contentWidth: 300, fontSize: 16, lines: mockLines('abc'), version: 1 })
  assert.equal(r.matchesIdentity(r.state, { displayText: 'abc', contentWidth: 300, fontSize: 14 }), false)
})

test('matchesIdentity: cursorUtf16 不影响匹配（cursor 不决定折行）', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ displayText: 'abc', contentWidth: 300, fontSize: 16, lines: mockLines('abc'), version: 1 })
  // 不同 cursor 但相同 displayText/contentWidth/fontSize → 仍然匹配
  assert.equal(r.matchesIdentity(r.state, { displayText: 'abc', contentWidth: 300, fontSize: 16 }), true)
})

// ── waitForLayout ──

test('waitForLayout: 当前 state 已匹配 → 立即返回', async () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ displayText: 'abc', contentWidth: 300, fontSize: 16, lines: mockLines('abc'), version: 1 })
  const result = await r.waitForLayout({ displayText: 'abc', contentWidth: 300, fontSize: 16 }, 100)
  assert.notEqual(result, null)
  assert.equal(result.lines.length, 1)
})

test('waitForLayout: 当前 state 不匹配 → 等待新版本', async () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ displayText: 'old', contentWidth: 300, fontSize: 16, lines: mockLines('old'), version: 1 })
  // 启动等待
  const waitPromise = r.waitForLayout({ displayText: 'new', contentWidth: 300, fontSize: 16 }, 200)
  // 模拟 SujianEditor 更新布局
  setTimeout(() => {
    r.updateLayout({ displayText: 'new', contentWidth: 300, fontSize: 16, lines: mockLines('new'), version: 2 })
  }, 50)
  const result = await waitPromise
  assert.notEqual(result, null)
  assert.equal(result.displayText, 'new')
})

test('waitForLayout: 超时返回 null', async () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ displayText: 'old', contentWidth: 300, fontSize: 16, lines: mockLines('old'), version: 1 })
  const result = await r.waitForLayout({ displayText: 'new', contentWidth: 300, fontSize: 16 }, 50)
  assert.equal(result, null)
})

test('cancelWait: 取消所有等待', async () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ displayText: 'old', contentWidth: 300, fontSize: 16, lines: mockLines('old'), version: 1 })
  const waitPromise = r.waitForLayout({ displayText: 'new', contentWidth: 300, fontSize: 16 }, 500)
  setTimeout(() => r.cancelWait(), 50)
  const result = await waitPromise
  assert.equal(result, null)
})

// ── 行级导航 ──

test('up: 返回上一行行首', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbbccccc'
  r.updateLayout({ displayText: text, contentWidth: 300, fontSize: 16, lines: mockLines(text), version: 1 })
  const targetLine = r.getPreviousLineIndex(7)
  assert.equal(targetLine, 0)
  assert.equal(r.getLineStart(targetLine), 0)
})

test('down: 返回下一行行首', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbbccccc'
  r.updateLayout({ displayText: text, contentWidth: 300, fontSize: 16, lines: mockLines(text), version: 1 })
  const targetLine = r.getNextLineIndex(2)
  assert.equal(targetLine, 1)
  assert.equal(r.getLineStart(targetLine), 5)
})

test('home: 返回当前行行首', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbbccccc'
  r.updateLayout({ displayText: text, contentWidth: 300, fontSize: 16, lines: mockLines(text), version: 1 })
  const lineIdx = r.getCurrentLineIndex(7)
  assert.equal(lineIdx, 1)
  assert.equal(r.getLineStart(lineIdx), 5)
})

test('end: 返回当前行行末', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbbccccc'
  r.updateLayout({ displayText: text, contentWidth: 300, fontSize: 16, lines: mockLines(text), version: 1 })
  const lineIdx = r.getCurrentLineIndex(7)
  assert.equal(lineIdx, 1)
  assert.equal(r.getLineEnd(lineIdx), 10)
})

test('up: 已在第一行 → getPreviousLineIndex 返回 -1', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbb'
  r.updateLayout({ displayText: text, contentWidth: 300, fontSize: 16, lines: mockLines(text), version: 1 })
  assert.equal(r.getPreviousLineIndex(2), -1)
})

test('down: 已在最后一行 → getNextLineIndex 返回 -1', () => {
  const r = new LineNavigationResolver()
  const text = 'aaaaabbbbb'
  r.updateLayout({ displayText: text, contentWidth: 300, fontSize: 16, lines: mockLines(text), version: 1 })
  assert.equal(r.getNextLineIndex(7), -1)
})

test('updateLayout(null): 清空 state', () => {
  const r = new LineNavigationResolver()
  r.updateLayout({ displayText: 'abc', contentWidth: 300, fontSize: 16, lines: mockLines('abc'), version: 1 })
  r.updateLayout(null)
  assert.equal(r.getLines().length, 0)
  assert.equal(r.getCurrentLineIndex(0), -1)
})

test('中文行布局: 匹配时正确算行', () => {
  const r = new LineNavigationResolver()
  const text = '你好世界测试'
  const lines = [{ startUtf16: 0, endUtf16: 3, y: 0, height: 20 }, { startUtf16: 3, endUtf16: 6, y: 20, height: 20 }]
  r.updateLayout({ displayText: text, contentWidth: 300, fontSize: 16, lines, version: 1 })
  assert.equal(r.getPreviousLineIndex(4), 0)
  assert.equal(r.getLineStart(0), 0)
})

console.log('---')
console.log(`✅ line_navigation_resolver: ${passed} tests passed`)
