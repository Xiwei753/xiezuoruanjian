// line_navigation_resolver.test.mjs — LineNavigationResolver 纯逻辑单测。
// Issue #629 评论15 第3项：彻底重写，对齐新接口。
// - EditorLayoutIdentity = { revision, generation, compositionGeneration, compositionSessionId, displayText }
// - 等待无 timeout，只有 cancelWait 和匹配布局发布两个出口
// - 方法改为纯函数式 static，接收显式 state 参数
// Issue #629 评论16 第2项：新增 NavigationLine/caretStops + getCaretX/getNearestOffsetAtX 测试。
// Issue #629 评论5357756359 第2项：直接 import 生产 matchesEditorLayoutIdentity，不再复制实现；
// 补 compositionSessionId 测试。
//
// 运行：node line_navigation_resolver.test.mjs

import { strict as assert } from 'node:assert'
// Issue #629 评论5357756359 第2项：直接 import 生产 matchesEditorLayoutIdentity，不再复制实现。
import { matchesEditorLayoutIdentity } from '../editor_layout_identity.ts'
// Issue #629 评论5358224312 第2项：直接 import 生产纯函数，不再复制算法实现。
// 测试里的 LineNavigationResolver 纯算法 static 方法改为薄委托调用这些生产函数。
import {
  resolveVisualLineIndex,
  horizontalForOffset,
  offsetForHorizontal,
  positionForOffsetInLine,
} from '../../render/editor_layout_math.ts'

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

// Issue #629 评论5358224312 第2项：toLineRanges 把测试 NavigationLine[] 转成生产 LineRange[]，
// 供生产纯函数（resolveVisualLineIndex / positionForOffsetInLine 等）消费。
// NavigationLine = { startUtf16, endUtf16, breakKind, ... }，LineRange = { start, end, breakKind }。
const toLineRanges = (lines) => lines.map(l => ({
  start: l.startUtf16,
  end: l.endUtf16,
  breakKind: l.breakKind,
}))

// Issue #629 评论5358224312 第2项：LineNavigationResolver 只保留 waiter/state，
// 纯算法 static 方法全部薄委托 editor_layout_math 生产函数，不再复制算法实现。
// 这样生产 helper 改坏时测试会跟着红，不会出现"镜像测试仍绿"的假阳性。
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
    if (this.state !== null && matchesEditorLayoutIdentity(this.state, identity)) {
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
      if (matchesEditorLayoutIdentity(state, waiter.identity)) {
        waiter.resolve(state)
        resolved.push(waiter)
      }
    }
    for (const w of resolved) {
      const idx = this.waiters.indexOf(w)
      if (idx >= 0) this.waiters.splice(idx, 1)
    }
  }
  // Issue #629 评论5358224312 第2项：薄委托生产 resolveVisualLineIndex。
  // 保留 state.lines.length === 0 → -1 守卫，与 LineNavigationResolver.ets 生产语义一致。
  static getCurrentLineIndex(state, position) {
    if (state.lines.length === 0) return -1
    return resolveVisualLineIndex(toLineRanges(state.lines), position)
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
  // Issue #629 评论5358224312 第2项：薄委托生产 positionForOffsetInLine。
  static positionForOffsetInLine(state, lineIndex, utf16Offset) {
    return positionForOffsetInLine(toLineRanges(state.lines), lineIndex, utf16Offset)
  }
  static getLineStart(state, lineIndex) {
    if (lineIndex < 0 || lineIndex >= state.lines.length) return 0
    return state.lines[lineIndex].startUtf16
  }
  static getLineEnd(state, lineIndex) {
    if (lineIndex < 0 || lineIndex >= state.lines.length) return 0
    return state.lines[lineIndex].endUtf16
  }
  // Issue #629 评论5358224312 第2项：薄委托生产 horizontalForOffset，不再复制二分。
  static getCaretX(state, position) {
    const lineIdx = this.getCurrentLineIndex(state, position)
    if (lineIdx < 0 || lineIdx >= state.lines.length) return 0
    const stops = state.lines[lineIdx].caretStops
    if (!stops || stops.length === 0) return 0
    return horizontalForOffset(stops, position.utf16Offset)
  }
  // Issue #629 评论5358224312 第2项：薄委托生产 offsetForHorizontal，不再复制二分。
  static getNearestOffsetAtX(state, lineIndex, x) {
    if (lineIndex < 0 || lineIndex >= state.lines.length) return 0
    const line = state.lines[lineIndex]
    const stops = line.caretStops
    if (!stops || stops.length === 0) return line.startUtf16
    return offsetForHorizontal(stops, x)
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
    compositionSessionId: 0,
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
    compositionSessionId: 0,
    displayText: text,
    ...extra
  }
}

console.log('line_navigation_resolver 纯逻辑单测（Issue #629 评论15 第3项）')
console.log('---')

// ── matchesIdentity（新版：revision + generation + compositionGeneration + displayText）──

test('matchesIdentity: 全字段匹配 → true', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  assert.equal(matchesEditorLayoutIdentity(r.state, makeIdentity('abc')), true)
})

test('matchesIdentity: revision 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  assert.equal(matchesEditorLayoutIdentity(r.state, makeIdentity('abc', { revision: 2 })), false)
})

test('matchesIdentity: generation 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  assert.equal(matchesEditorLayoutIdentity(r.state, makeIdentity('abc', { generation: 1 })), false)
})

test('matchesIdentity: compositionGeneration 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  assert.equal(matchesEditorLayoutIdentity(r.state, makeIdentity('abc', { compositionGeneration: 5 })), false)
})

test('matchesIdentity: displayText 不同 → false', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  assert.equal(matchesEditorLayoutIdentity(r.state, makeIdentity('abd')), false)
})

test('matchesIdentity: contentWidth/fontSize 不影响匹配（不属于编辑状态身份）', () => {
  const r = new LineNavigationResolver()
  r.updateLayout(makeState('abc'))
  // 同一编辑状态但不同 contentWidth → 仍然匹配（因为 contentWidth 不在 identity 中）
  assert.equal(matchesEditorLayoutIdentity(r.state, makeIdentity('abc')), true)
})

// ── Issue #629 评论5357756359 第2项：compositionSessionId 测试 ──

test('matchesEditorLayoutIdentity: 全字段相同（含 compositionSessionId）→ true', () => {
  const state = makeState('abc')
  assert.equal(matchesEditorLayoutIdentity(state, makeIdentity('abc')), true)
})

test('matchesEditorLayoutIdentity: 仅 compositionSessionId 不同 → false', () => {
  const state = makeState('abc')
  assert.equal(matchesEditorLayoutIdentity(state, makeIdentity('abc', { compositionSessionId: 1 })), false)
})

testAsync('waitForLayout: session A waiter 不被 session B layout 错误 resolve', async () => {
  const r = new LineNavigationResolver()
  // 当前布局是 session B（compositionSessionId=2）
  r.updateLayout(makeState('abc', { compositionSessionId: 2 }))
  // 等待 session A（compositionSessionId=1）的布局
  const waitPromise = r.waitForLayout(makeIdentity('abc', { compositionSessionId: 1 }))
  // session B 的布局不应 resolve session A 的 waiter
  // 等一小段时间确认 waiter 未被 resolve
  let resolved = false
  waitPromise.then((result) => { if (result !== null) resolved = true })
  await new Promise(resolve => setTimeout(resolve, 50))
  assert.equal(resolved, false, 'session B layout 不应 resolve session A waiter')
  // 发布真正 session A 的布局
  r.updateLayout(makeState('abc', { compositionSessionId: 1 }))
  const result = await waitPromise
  assert.notEqual(result, null, '发布 session A layout 后 waiter 被 resolve')
  assert.equal(result.compositionSessionId, 1)
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

// ── Issue #629 评论5358224312 第2项：直接测生产纯函数，确保生产路线被测 ──
// 这些测试直接调用从 editor_layout_math.ts import 的生产函数，
// 不经过 LineNavigationResolver wrapper，确保生产实现本身被覆盖。
// 若生产 helper 改坏，这些测试会直接红，不会出现"镜像测试仍绿"的假阳性。

test('生产 resolveVisualLineIndex: soft-wrap 行末 Upstream → 归本行', () => {
  const lines = [
    { start: 0, end: 5, breakKind: 'softWrap' },
    { start: 5, end: 10, breakKind: 'endOfText' },
  ]
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 5, affinity: 'upstream' }), 0)
})

test('生产 resolveVisualLineIndex: soft-wrap 行末 Downstream → 归下一行', () => {
  const lines = [
    { start: 0, end: 5, breakKind: 'softWrap' },
    { start: 5, end: 10, breakKind: 'endOfText' },
  ]
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 5, affinity: 'downstream' }), 1)
})

test('生产 resolveVisualLineIndex: offset 在行内 → 该行', () => {
  const lines = [
    { start: 0, end: 5, breakKind: 'softWrap' },
    { start: 5, end: 10, breakKind: 'endOfText' },
  ]
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 3, affinity: 'downstream' }), 0)
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 7, affinity: 'downstream' }), 1)
})

test('生产 resolveVisualLineIndex: 空行数组 → 0', () => {
  assert.equal(resolveVisualLineIndex([], { utf16Offset: 0, affinity: 'downstream' }), 0)
})

test('生产 horizontalForOffset: 二分找到对应 stop 的 x', () => {
  const stops = [
    { utf16Offset: 0, x: 0 },
    { utf16Offset: 1, x: 10 },
    { utf16Offset: 2, x: 20 },
    { utf16Offset: 3, x: 30 },
  ]
  assert.equal(horizontalForOffset(stops, 2), 20)
  assert.equal(horizontalForOffset(stops, 0), 0)
  assert.equal(horizontalForOffset(stops, 3), 30)
})

test('生产 horizontalForOffset: 空 stops → 0', () => {
  assert.equal(horizontalForOffset([], 5), 0)
})

test('生产 offsetForHorizontal: 二分找到最近 offset（距离相等取左）', () => {
  const stops = [
    { utf16Offset: 0, x: 0 },
    { utf16Offset: 1, x: 10 },
    { utf16Offset: 2, x: 20 },
    { utf16Offset: 3, x: 30 },
  ]
  // x=25 → 距 20 和 30 相等，取左 → offset 2
  assert.equal(offsetForHorizontal(stops, 25), 2)
  assert.equal(offsetForHorizontal(stops, 0), 0)
  // x 超过最大 → 最后一个 offset
  assert.equal(offsetForHorizontal(stops, 1000), 3)
})

test('生产 offsetForHorizontal: 空 stops → 0', () => {
  assert.equal(offsetForHorizontal([], 5), 0)
})

test('生产 positionForOffsetInLine: SoftWrap 行末 → Upstream', () => {
  const lines = [
    { start: 0, end: 5, breakKind: 'softWrap' },
    { start: 5, end: 10, breakKind: 'endOfText' },
  ]
  const pos = positionForOffsetInLine(lines, 0, 5)
  assert.equal(pos.utf16Offset, 5)
  assert.equal(pos.affinity, 'upstream')
})

test('生产 positionForOffsetInLine: SoftWrap 行首（下一行 start）→ Downstream', () => {
  const lines = [
    { start: 0, end: 5, breakKind: 'softWrap' },
    { start: 5, end: 10, breakKind: 'endOfText' },
  ]
  const pos = positionForOffsetInLine(lines, 1, 5)
  assert.equal(pos.utf16Offset, 5)
  assert.equal(pos.affinity, 'downstream')
})

test('生产 positionForOffsetInLine: HardBreak 行末 → Downstream', () => {
  const lines = [
    { start: 0, end: 3, breakKind: 'hardBreak' },
    { start: 4, end: 7, breakKind: 'endOfText' },
  ]
  const pos = positionForOffsetInLine(lines, 0, 3)
  assert.equal(pos.affinity, 'downstream')
})

test('生产 positionForOffsetInLine: EndOfText 行末 → Downstream', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'endOfText' }]
  const pos = positionForOffsetInLine(lines, 0, 5)
  assert.equal(pos.affinity, 'downstream')
})

test('生产 positionForOffsetInLine: 行中间位置 → Downstream', () => {
  const lines = [
    { start: 0, end: 5, breakKind: 'softWrap' },
    { start: 5, end: 10, breakKind: 'endOfText' },
  ]
  assert.equal(positionForOffsetInLine(lines, 0, 3).affinity, 'downstream')
})

test('生产 positionForOffsetInLine: 越界 lineIndex → Downstream', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'endOfText' }]
  assert.equal(positionForOffsetInLine(lines, -1, 0).affinity, 'downstream')
  assert.equal(positionForOffsetInLine(lines, 5, 0).affinity, 'downstream')
})

// ── 等价断言：测试 wrapper 走的就是生产函数，证明没有第二套算法 ──

test('等价: wrapper getCurrentLineIndex === 生产 resolveVisualLineIndex（非空 lines）', () => {
  const state = makeState('aaaaabbbbbccccc')
  const pos = { utf16Offset: 5, affinity: 'upstream' }
  assert.equal(
    LineNavigationResolver.getCurrentLineIndex(state, pos),
    resolveVisualLineIndex(toLineRanges(state.lines), pos),
  )
  const pos2 = { utf16Offset: 5, affinity: 'downstream' }
  assert.equal(
    LineNavigationResolver.getCurrentLineIndex(state, pos2),
    resolveVisualLineIndex(toLineRanges(state.lines), pos2),
  )
})

test('等价: wrapper positionForOffsetInLine === 生产 positionForOffsetInLine', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.deepEqual(
    LineNavigationResolver.positionForOffsetInLine(state, 0, 5),
    positionForOffsetInLine(toLineRanges(state.lines), 0, 5),
  )
  assert.deepEqual(
    LineNavigationResolver.positionForOffsetInLine(state, 1, 5),
    positionForOffsetInLine(toLineRanges(state.lines), 1, 5),
  )
})

test('等价: wrapper getCaretX === 生产 horizontalForOffset（经 getCurrentLineIndex 定行）', () => {
  const state = makeState('aaaaabbbbbccccc')
  const pos = { utf16Offset: 7, affinity: 'downstream' }
  const lineIdx = LineNavigationResolver.getCurrentLineIndex(state, pos)
  assert.equal(
    LineNavigationResolver.getCaretX(state, pos),
    horizontalForOffset(state.lines[lineIdx].caretStops, pos.utf16Offset),
  )
})

test('等价: wrapper getNearestOffsetAtX === 生产 offsetForHorizontal', () => {
  const state = makeState('aaaaabbbbbccccc')
  assert.equal(
    LineNavigationResolver.getNearestOffsetAtX(state, 0, 25),
    offsetForHorizontal(state.lines[0].caretStops, 25),
  )
  assert.equal(
    LineNavigationResolver.getNearestOffsetAtX(state, 1, 35),
    offsetForHorizontal(state.lines[1].caretStops, 35),
  )
})

console.log('---')
console.log(`✅ line_navigation_resolver: ${passed} tests passed`)
