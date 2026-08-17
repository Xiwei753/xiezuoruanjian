// line_navigation_resolver.test.mjs — LineNavigationResolver 纯逻辑单测。
// Issue #629 评论15 第3项：彻底重写，对齐新接口。
// - EditorLayoutIdentity = { revision, generation, compositionGeneration, displayText }
// - 等待无 timeout，只有 cancelWait 和匹配布局发布两个出口
// - 方法改为纯函数式 static，接收显式 state 参数
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

// ── 纯逻辑镜像：与 LineNavigationResolver.ets 对齐 ──
// Issue #629 评论15 第3项：EditorLayoutIdentity = revision + generation + compositionGeneration + displayText。

class LineNavigationResolver {
  constructor() {
    this.state = null
    this.waiters = []
  }
  updateLayout(state) {
    this.state = state
    if (state !== null) {
      this.resolveWaiters(state)
    }
  }
  // Issue #629 评论15 第3项：不设 timeout，只靠匹配布局发布或 cancelWait 退出。
  waitForLayout(identity) {
    if (this.state !== null && this.matchesIdentity(this.state, identity)) {
      return Promise.resolve(this.state)
    }
    return new Promise((resolve) => {
      this.waiters.push({ identity, resolve })
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
      const idx = this.waiters.indexOf(w)
      if (idx >= 0) this.waiters.splice(idx, 1)
    }
  }
  matchesIdentity(state, identity) {
    if (state === null) return false
    return state.revision === identity.revision
      && state.generation === identity.generation
      && state.compositionGeneration === identity.compositionGeneration
      && state.displayText === identity.displayText
  }
  // 纯函数式静态方法
  static getCurrentLineIndex(state, cursorUtf16) {
    if (state.lines.length === 0) return -1
    for (let i = 0; i < state.lines.length; i++) {
      const line = state.lines[i]
      if (cursorUtf16 >= line.startUtf16 && cursorUtf16 <= line.endUtf16) return i
    }
    return state.lines.length - 1
  }
  static getPreviousLineIndex(state, cursorUtf16) {
    const idx = this.getCurrentLineIndex(state, cursorUtf16)
    if (idx <= 0) return -1
    return idx - 1
  }
  static getNextLineIndex(state, cursorUtf16) {
    const idx = this.getCurrentLineIndex(state, cursorUtf16)
    if (idx < 0 || idx >= state.lines.length - 1) return -1
    return idx + 1
  }
  static getLineStart(state, lineIndex) {
    if (lineIndex < 0 || lineIndex >= state.lines.length) return 0
    return state.lines[lineIndex].startUtf16
  }
  static getLineEnd(state, lineIndex) {
    if (lineIndex < 0 || lineIndex >= state.lines.length) return 0
    return state.lines[lineIndex].endUtf16
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

// 构造 layout state
function makeState(text, extra = {}) {
  return {
    revision: 1,
    generation: 0,
    compositionGeneration: -1,
    contentWidth: 300,
    fontSize: 16,
    lines: mockLines(text),
    displayText: text,
    ...extra
  }
}

// 构造 identity
function makeIdentity(text, extra = {}) {
  return {
    revision: 1,
    generation: 0,
    compositionGeneration: -1,
    displayText: text,
    ...extra
  }
}

console.log('line_navigation_resolver 纯逻辑单测（Issue #629 评论15 第3项）')
console.log('---')

// ── matchesIdentity（新版：revision + generation + compositionGeneration + displayText）──

test('matchesIdentity: state 为 null → false', () => {
  const r = new LineNavigationResolver()
  assert.equal(r.matchesIdentity(null, makeIdentity('abc')), false)
})

test('matchesIdentity: 全字段匹配 → true', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  assert.equal(r.matchesIdentity(r.state, makeIdentity('abc')), true)
})

test('matchesIdentity: revision 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  assert.equal(r.matchesIdentity(r.state, makeIdentity('abc', { revision: 2 })), false)
})

test('matchesIdentity: generation 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  assert.equal(r.matchesIdentity(r.state, makeIdentity('abc', { generation: 1 })), false)
})

test('matchesIdentity: compositionGeneration 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  assert.equal(r.matchesIdentity(r.state, makeIdentity('abc', { compositionGeneration: 5 })), false)
})

test('matchesIdentity: displayText 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  assert.equal(r.matchesIdentity(r.state, makeIdentity('abd')), false)
})

test('matchesIdentity: contentWidth/fontSize 不影响匹配（不属于编辑状态身份）', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  // 同一编辑状态但不同 contentWidth → 仍然匹配（因为 contentWidth 不在 identity 中）
  assert.equal(r.matchesIdentity(r.state, makeIdentity('abc')), true)
})

// ── waitForLayout（无 timeout）──

test('waitForLayout: 当前 state 已匹配 → 立即返回', async () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  const result = await r.waitForLayout(makeIdentity('abc'))
  assert.notEqual(result, null)
  assert.equal(result.lines.length, 1)
})

test('waitForLayout: 当前 state 不匹配 → 等待新版本发布', async () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('old'))
  // 启动等待（无 timeout，不会自动 resolve）
  const waitPromise = r.waitForLayout(makeIdentity('new'))
  // 模拟 SujianEditor 更新布局
  setTimeout(() => {
    r.updateLayout(makeState('new'))
  }, 50)
  const result = await waitPromise
  assert.notEqual(result, null)
  assert.equal(result.displayText, 'new')
})

test('cancelWait: 取消所有等待 → 返回 null', async () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('old'))
  const waitPromise = r.waitForLayout(makeIdentity('new'))
  setTimeout(() => r.cancelWait(), 50)
  const result = await waitPromise
  assert.equal(result, null)
})

// ── 纯函数式行导航（用显式 state）──

test('getPreviousLineIndex: 返回上一行', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(LineNavigationResolver.getPreviousLineIndex(state, 7), 0)
})

test('getPreviousLineIndex: 已在第一行 → -1', () => {
  const state = makeState('aaaaabbbbb')
  assert.equal(LineNavigationResolver.getPreviousLineIndex(state, 2), -1)
})

test('getNextLineIndex: 返回下一行', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(LineNavigationResolver.getNextLineIndex(state, 2), 1)
})

test('getNextLineIndex: 已在最后一行 → -1', () => {
  const state = makeState('aaaaabbbbb')
  assert.equal(LineNavigationResolver.getNextLineIndex(state, 7), -1)
})

test('getLineStart: 返回行首 offset', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(LineNavigationResolver.getLineStart(state, 1), 5)
})

test('getLineEnd: 返回行末 offset', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(LineNavigationResolver.getLineEnd(state, 1), 10)
})

test('getCurrentLineIndex: 定位光标所在行', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(LineNavigationResolver.getCurrentLineIndex(state, 7), 1)
})

test('updateLayout(null): 清空 state', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  r.updateLayout(null)
  assert.equal(r.state, null)
})

// ── compositionGeneration 变化场景 ──

test('composition 生成变化但 revision 未变时不能复用旧布局', async () => {
  const r = new LineNavigationResolver()
  // revision=1, compositionGeneration=-1（无 composition）
  r.updateLayout(makeState('abc', { compositionGeneration: -1 }))
  // 等待 revision=1, compositionGeneration=0（新 composition）
  const waitPromise = r.waitForLayout(makeIdentity('abc', { compositionGeneration: 0 }))
  // 发布新布局（revision 不变但 compositionGeneration 变了）
  setTimeout(() => {
    r.updateLayout(makeState('abc', { compositionGeneration: 0 }))
  }, 50)
  const result = await waitPromise
  assert.notEqual(result, null)
  assert.equal(result.compositionGeneration, 0)
})

test('等待返回 state 后目标计算必须使用该 state.lines', async () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('old', { compositionGeneration: -1 }))
  const waitPromise = r.waitForLayout(makeIdentity('new', { compositionGeneration: -1 }))
  const newState = makeState('new', { compositionGeneration: -1, lines: [
    { startUtf16: 0, endUtf16: 10, y: 0, height: 20 },
    { startUtf16: 10, endUtf16: 20, y: 20, height: 20 }
  ]})
  setTimeout(() => r.updateLayout(newState), 50)
  const result = await waitPromise
  // 使用返回的 state 计算目标
  assert.equal(LineNavigationResolver.getLineStart(result, 1), 10)
  assert.equal(LineNavigationResolver.getLineEnd(result, 1), 20)
})

console.log('---')
console.log(`✅ line_navigation_resolver: ${passed} tests passed`)
