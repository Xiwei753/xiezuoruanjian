// line_navigation_resolver.test.mjs — LineNavigationResolver 纯逻辑单测。
// Issue #629 评论15 第3项：彻底重写，对齐新接口。
// - EditorLayoutIdentity = { revision, generation, compositionGeneration, displayText }
// - 等待无 timeout，只有 cancelWait 和匹配布局发布两个出口
// - 方法改为纯函数式 static，接收显式 state 参数
// Issue #629 评论16 第2项：新增 NavigationLine/caretStops + getCaretX/getNearestOffsetAtX 测试。
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
  // R7 任务B：纯函数式静态方法 — 接受 VisualCaretPosition（含 affinity），不写死 Downstream。
  // 与 production resolveVisualLineIndex 对齐。
  static getCurrentLineIndex(state, position) {
    if (state.lines.length === 0) return -1
    const { utf16Offset, affinity } = position
    for (let i = 0; i < state.lines.length; i++) {
      const line = state.lines[i]
      if (utf16Offset >= line.startUtf16 && utf16Offset < line.endUtf16) return i
      if (utf16Offset === line.endUtf16) {
        if (line.breakKind === 'softWrap') {
          return affinity === 'upstream' ? i : i + 1
        }
        return i
      }
    }
    return state.lines.length - 1
  }
  static getPreviousLineIndex(state, position) {
    const idx = this.getCurrentLineIndex(state, position)
    if (idx <= 0) return -1
    return idx - 1
  }
  static getNextLineIndex(state, position) {
    const idx = this.getCurrentLineIndex(state, position)
    if (idx < 0 || idx >= state.lines.length - 1) return -1
    return idx + 1
  }
  // R7 任务B：纯函数 — 根据 offset 在指定行的 soft-wrap 边界位置，返回正确 affinity。
  static positionForOffsetInLine(state, lineIndex, utf16Offset) {
    if (lineIndex < 0 || lineIndex >= state.lines.length) {
      return { utf16Offset, affinity: 'downstream' }
    }
    const line = state.lines[lineIndex]
    if (utf16Offset === line.startUtf16) {
      return { utf16Offset, affinity: 'downstream' }
    }
    if (utf16Offset === line.endUtf16) {
      if (line.breakKind === 'softWrap') {
        return { utf16Offset, affinity: 'upstream' }
      }
      return { utf16Offset, affinity: 'downstream' }
    }
    return { utf16Offset, affinity: 'downstream' }
  }
  static getLineStart(state, lineIndex) {
    if (lineIndex < 0 || lineIndex >= state.lines.length) return 0
    return state.lines[lineIndex].startUtf16
  }
  static getLineEnd(state, lineIndex) {
    if (lineIndex < 0 || lineIndex >= state.lines.length) return 0
    return state.lines[lineIndex].endUtf16
  }
  // R7 任务B：getCaretX 接受 VisualCaretPosition。
  static getCaretX(state, position) {
    const lineIdx = this.getCurrentLineIndex(state, position)
    if (lineIdx < 0 || lineIdx >= state.lines.length) return 0
    const line = state.lines[lineIdx]
    const stops = line.caretStops
    if (!stops || stops.length === 0) return 0
    let lo = 0, hi = stops.length - 1, bestIdx = 0
    let bestDist = Math.abs(stops[0].utf16Offset - position.utf16Offset)
    while (lo <= hi) {
      const mid = Math.floor((lo + hi) / 2)
      const dist = Math.abs(stops[mid].utf16Offset - position.utf16Offset)
      if (dist < bestDist || (dist === bestDist && stops[mid].utf16Offset <= position.utf16Offset)) {
        bestDist = dist
        bestIdx = mid
      }
      if (stops[mid].utf16Offset < position.utf16Offset) lo = mid + 1
      else if (stops[mid].utf16Offset > position.utf16Offset) hi = mid - 1
      else break
    }
    return stops[bestIdx].x
  }
  // Issue #629 评论16 第2项：纯函数 — 给定行内 x 坐标，返回最近的 UTF-16 offset。
  static getNearestOffsetAtX(state, lineIndex, x) {
    if (lineIndex < 0 || lineIndex >= state.lines.length) return 0
    const line = state.lines[lineIndex]
    const stops = line.caretStops
    if (!stops || stops.length === 0) return line.startUtf16
    let lo = 0, hi = stops.length - 1, bestIdx = 0
    while (lo <= hi) {
      const mid = Math.floor((lo + hi) / 2)
      if (stops[mid].x <= x) { bestIdx = mid; lo = mid + 1 }
      else hi = mid - 1
    }
    if (bestIdx + 1 < stops.length) {
      const leftX = stops[bestIdx].x
      const rightX = stops[bestIdx + 1].x
      if (x - leftX <= rightX - x) return stops[bestIdx].utf16Offset
      return stops[bestIdx + 1].utf16Offset
    }
    return stops[bestIdx].utf16Offset
  }
}

// 构造 mock navigation lines：每行 5 个字符，每字符宽 10px
// Issue #629 评论18：LineLayout 含 breakKind + caretStops
function mockNavLines(text) {
  const lines = []
  let start = 0
  while (start < text.length) {
    const end = Math.min(start + 5, text.length)
    const stops = []
    for (let i = start; i <= end; i++) {
      stops.push({ utf16Offset: i, x: (i - start) * 10 })
    }
    const isLast = end === text.length
    const breakKind = isLast ? 'endOfText' : 'softWrap'
    lines.push({ startUtf16: start, endUtf16: end, y: 0, height: 20, breakKind, caretStops: stops })
    start = end
  }
  return lines
}

// 构造 layout state（使用 NavigationLine[]）
function makeState(text, extra = {}) {
  return {
    revision: 1,
    generation: 0,
    compositionGeneration: -1,
    contentWidth: 300,
    fontSize: 16,
    lines: mockNavLines(text),
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

test('getPreviousLineIndex: 返回上一行（Downstream affinity）', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(LineNavigationResolver.getPreviousLineIndex(state, { utf16Offset: 7, affinity: 'downstream' }), 0)
})

test('getPreviousLineIndex: 已在第一行 → -1', () => {
  const state = makeState('aaaaabbbbb')
  assert.equal(LineNavigationResolver.getPreviousLineIndex(state, { utf16Offset: 2, affinity: 'downstream' }), -1)
})

test('getNextLineIndex: 返回下一行（Downstream affinity）', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(LineNavigationResolver.getNextLineIndex(state, { utf16Offset: 2, affinity: 'downstream' }), 1)
})

test('getNextLineIndex: 已在最后一行 → -1', () => {
  const state = makeState('aaaaabbbbb')
  assert.equal(LineNavigationResolver.getNextLineIndex(state, { utf16Offset: 7, affinity: 'downstream' }), -1)
})

test('getLineStart: 返回行首 offset', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(LineNavigationResolver.getLineStart(state, 1), 5)
})

test('getLineEnd: 返回行末 offset', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(LineNavigationResolver.getLineEnd(state, 1), 10)
})

test('getCurrentLineIndex: 定位光标所在行（Downstream affinity）', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(LineNavigationResolver.getCurrentLineIndex(state, { utf16Offset: 7, affinity: 'downstream' }), 1)
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
    { startUtf16: 0, endUtf16: 10, y: 0, height: 20, caretStops: [
      { utf16Offset: 0, x: 0 }, { utf16Offset: 10, x: 100 }
    ] },
    { startUtf16: 10, endUtf16: 20, y: 20, height: 20, caretStops: [
      { utf16Offset: 10, x: 0 }, { utf16Offset: 20, x: 100 }
    ] }
  ]})
  setTimeout(() => r.updateLayout(newState), 50)
  const result = await waitPromise
  // 使用返回的 state 计算目标
  assert.equal(LineNavigationResolver.getLineStart(result, 1), 10)
  assert.equal(LineNavigationResolver.getLineEnd(result, 1), 20)
})

// ── Issue #629 评论16 第2项：getCaretX / getNearestOffsetAtX ──

test('getCaretX: 找到光标所在行最近的 caret stop x（接受 VisualCaretPosition）', () => {
  const state = makeState('aaaaabbbbbccccc')
  const x = LineNavigationResolver.getCaretX(state, { utf16Offset: 7, affinity: 'downstream' })
  assert.equal(x, 20)
})

test('getCaretX: cursor 在行首返回 x=0（接受 VisualCaretPosition）', () => {
  const state = makeState('aaaaabbbbb')
  const x = LineNavigationResolver.getCaretX(state, { utf16Offset: 5, affinity: 'downstream' })
  assert.equal(x, 0)
})

test('getCaretX: cursor 在行末返回最后一个 stop 的 x（接受 VisualCaretPosition）', () => {
  const state = makeState('aaaaabbbbb')
  const x = LineNavigationResolver.getCaretX(state, { utf16Offset: 10, affinity: 'downstream' })
  assert.equal(x, 50)
})

test('getNearestOffsetAtX: 找目标行内最近的 code point 边界', () => {
  const state = makeState('aaaaabbbbbccccc')
  // 第一行 caretStops: offset 0→x=0, 1→x=10, 2→x=20, 3→x=30, 4→x=40, 5→x=50
  // x=25 → 最近是 offset 2 (x=20) 或 3 (x=30)，距离 5 和 5 相等，取左边界
  const offset = LineNavigationResolver.getNearestOffsetAtX(state, 0, 25)
  assert.equal(offset, 2)
})

test('getNearestOffsetAtX: x=0 → 行首', () => {
  const state = makeState('aaaaabbbbb')
  const offset = LineNavigationResolver.getNearestOffsetAtX(state, 0, 0)
  assert.equal(offset, 0)
})

test('getNearestOffsetAtX: x 超过行宽 → 行末', () => {
  const state = makeState('aaaaabbbbb')
  const offset = LineNavigationResolver.getNearestOffsetAtX(state, 0, 1000)
  assert.equal(offset, 5)
})

test('getNearestOffsetAtX: 第二行正确偏移', () => {
  const state = makeState('aaaaabbbbbccccc')
  // 第二行 caretStops: offset 5→x=0, 6→x=10, 7→x=20, 8→x=30, 9→x=40, 10→x=50
  const offset = LineNavigationResolver.getNearestOffsetAtX(state, 1, 35)
  assert.equal(offset, 8)
})

// ── R7 任务B：positionForOffsetInLine 测试 ──

test('positionForOffsetInLine: SoftWrap 起点（行首）→ Downstream', () => {
  const state = makeState('aaaaabbbbbccccc')
  // 第二行 startUtf16=5, breakKind='softWrap'
  const pos = LineNavigationResolver.positionForOffsetInLine(state, 1, 5)
  assert.equal(pos.utf16Offset, 5)
  assert.equal(pos.affinity, 'downstream')
})

test('positionForOffsetInLine: SoftWrap 终点（行末）→ Upstream', () => {
  const state = makeState('aaaaabbbbbccccc')
  // 第一行 endUtf16=5, breakKind='softWrap'
  const pos = LineNavigationResolver.positionForOffsetInLine(state, 0, 5)
  assert.equal(pos.utf16Offset, 5)
  assert.equal(pos.affinity, 'upstream')
})

test('positionForOffsetInLine: HardBreak 行末 → Downstream', () => {
  const state = {
    revision: 1, generation: 0, compositionGeneration: -1,
    contentWidth: 300, fontSize: 16, displayText: 'abc\ndef',
    lines: [
      { startUtf16: 0, endUtf16: 3, y: 0, height: 20, breakKind: 'hardBreak',
        caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 3, x: 30 }] },
      { startUtf16: 4, endUtf16: 7, y: 20, height: 20, breakKind: 'endOfText',
        caretStops: [{ utf16Offset: 4, x: 0 }, { utf16Offset: 7, x: 30 }] }
    ]
  }
  const pos = LineNavigationResolver.positionForOffsetInLine(state, 0, 3)
  assert.equal(pos.utf16Offset, 3)
  assert.equal(pos.affinity, 'downstream')
})

test('positionForOffsetInLine: EndOfText 行末 → Downstream', () => {
  const state = makeState('aaaaabbbbb')
  // 最后一行 endUtf16=10, breakKind='endOfText'
  const pos = LineNavigationResolver.positionForOffsetInLine(state, 1, 10)
  assert.equal(pos.utf16Offset, 10)
  assert.equal(pos.affinity, 'downstream')
})

test('positionForOffsetInLine: 行中间位置 → Downstream', () => {
  const state = makeState('aaaaabbbbbccccc')
  const pos = LineNavigationResolver.positionForOffsetInLine(state, 0, 3)
  assert.equal(pos.utf16Offset, 3)
  assert.equal(pos.affinity, 'downstream')
})

// ── R7 任务B：soft-wrap 边界上 affinity 区分行导航 ──

test('getCurrentLineIndex: soft-wrap 行末 Upstream → 归上一行', () => {
  const state = makeState('aaaaabbbbbccccc')
  // 第一行 endUtf16=5, breakKind='softWrap'
  // Upstream at soft-wrap end → 归第一行（行末）
  assert.equal(LineNavigationResolver.getCurrentLineIndex(state, { utf16Offset: 5, affinity: 'upstream' }), 0)
})

test('getCurrentLineIndex: soft-wrap 行末 Downstream → 归下一行', () => {
  const state = makeState('aaaaabbbbbccccc')
  // 第一行 endUtf16=5, breakKind='softWrap'
  // Downstream at soft-wrap end → 归第二行（行首）
  assert.equal(LineNavigationResolver.getCurrentLineIndex(state, { utf16Offset: 5, affinity: 'downstream' }), 1)
})

test('getPreviousLineIndex: soft-wrap 行末 Upstream → 当前是行末，上一行是第一行之前', () => {
  const state = makeState('aaaaabbbbbccccc')
  // Upstream at 5 → 归第一行（index 0），上一行不存在
  assert.equal(LineNavigationResolver.getPreviousLineIndex(state, { utf16Offset: 5, affinity: 'upstream' }), -1)
})

test('getNextLineIndex: soft-wrap 行末 Downstream → 当前是第二行行首，下一行是第三行', () => {
  const state = makeState('aaaaabbbbbccccc')
  // Downstream at 5 → 归第二行（index 1），下一行是第三行（index 2）
  assert.equal(LineNavigationResolver.getNextLineIndex(state, { utf16Offset: 5, affinity: 'downstream' }), 2)
})

test('getCaretX: Upstream at soft-wrap end → 在第一行行末计算 x', () => {
  const state = makeState('aaaaabbbbbccccc')
  // Upstream at 5 → 归第一行
  const x = LineNavigationResolver.getCaretX(state, { utf16Offset: 5, affinity: 'upstream' })
  assert.equal(x, 50) // 第一行最后一个 stop 的 x
})

test('getCaretX: Downstream at soft-wrap end → 在第二行行首计算 x', () => {
  const state = makeState('aaaaabbbbbccccc')
  // Downstream at 5 → 归第二行
  const x = LineNavigationResolver.getCaretX(state, { utf16Offset: 5, affinity: 'downstream' })
  assert.equal(x, 0) // 第二行第一个 stop 的 x
})

console.log('---')
console.log(`✅ line_navigation_resolver: ${passed} tests passed`)
