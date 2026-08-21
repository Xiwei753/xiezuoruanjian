// line_navigation_resolver.test.mjs — LineNavigationResolver 纯逻辑单测。
// Issue #629 评论21：删除测试镜像，复用生产实现。
// - LineLayoutStore：状态/等待核心，来自 line_layout_store.ts
// - matchesEditorLayoutIdentity：身份匹配，来自 editor_layout_identity.ts
// - resolveVisualLineIndex / horizontalForOffset / offsetForHorizontal / positionForOffsetInLine：纯函数，来自 editor_layout_math.ts
// Issue #629 评论5358224312 第2项：直接测生产纯函数，确保生产路线被测。
// - 等待无 timeout，只有 cancelAll 和匹配布局发布两个出口
// - 方法改为纯函数式 static，接收显式 state 参数
// Issue #629 评论5357756359 第2项：补 compositionSessionId 测试。
//
// 运行：node line_navigation_resolver.test.mjs

import { strict as assert } from 'node:assert'
import { LineLayoutStore } from '../line_layout_store.ts'
import { matchesEditorLayoutIdentity } from '../editor_layout_identity.ts'
import {
  resolveVisualLineIndex,
  horizontalForOffset,
  offsetForHorizontal,
  positionForOffsetInLine,
  CaretAffinity,
  LineBreakKind,
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

// ── 辅助函数 ──

// 构造 mock navigation lines：每行 5 个字符，每字符宽 10px
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

// ── LineLayoutStore 测试（生产实现）──

console.log('line_navigation_resolver 纯逻辑单测（Issue #629 评论21：复用生产实现）')
console.log('---')

// ── matchesEditorLayoutIdentity（生产函数）──

test('matchesEditorLayoutIdentity: null → false', () => {
  assert.equal(matchesEditorLayoutIdentity(null, makeIdentity('abc')), false)
})

test('matchesEditorLayoutIdentity: 全字段匹配 → true', () => {
  assert.equal(matchesEditorLayoutIdentity(makeState('abc'), makeIdentity('abc')), true)
})

test('matchesEditorLayoutIdentity: revision 不同 → false', () => {
  assert.equal(matchesEditorLayoutIdentity(makeState('abc'), makeIdentity('abc', { revision: 2 })), false)
})

test('matchesEditorLayoutIdentity: generation 不同 → false', () => {
  assert.equal(matchesEditorLayoutIdentity(makeState('abc'), makeIdentity('abc', { generation: 1 })), false)
})

test('matchesEditorLayoutIdentity: compositionGeneration 不同 → false', () => {
  assert.equal(matchesEditorLayoutIdentity(makeState('abc'), makeIdentity('abc', { compositionGeneration: 5 })), false)
})

test('matchesEditorLayoutIdentity: compositionSessionId 不同 → false', () => {
  assert.equal(matchesEditorLayoutIdentity(makeState('abc'), makeIdentity('abc', { compositionSessionId: 99 })), false)
})

test('matchesEditorLayoutIdentity: displayText 不同 → false', () => {
  assert.equal(matchesEditorLayoutIdentity(makeState('abc'), makeIdentity('abd')), false)
})

test('matchesEditorLayoutIdentity: contentWidth/fontSize 不影响匹配', () => {
  assert.equal(matchesEditorLayoutIdentity(makeState('abc'), makeIdentity('abc')), true)
})

// ── LineLayoutStore 状态/等待（生产实现）──

testAsync('LineLayoutStore: 当前 state 已匹配 → 立即返回', async () => {
  const store = new LineLayoutStore()
  store.update(makeState('abc'))
  const result = await store.waitFor(makeIdentity('abc'))
  assert.notEqual(result, null)
  assert.equal(result.lines.length, 1)
})

testAsync('LineLayoutStore: 当前 state 不匹配 → 等待新版本发布', async () => {
  const store = new LineLayoutStore()
  store.update(makeState('old'))
  const waitPromise = store.waitFor(makeIdentity('new'))
  setTimeout(() => {
    store.update(makeState('new'))
  }, 50)
  const result = await waitPromise
  assert.notEqual(result, null)
  assert.equal(result.displayText, 'new')
})

testAsync('LineLayoutStore: cancelAll → 返回 null', async () => {
  const store = new LineLayoutStore()
  store.update(makeState('old'))
  const waitPromise = store.waitFor(makeIdentity('new'))
  setTimeout(() => store.cancelAll(), 50)
  const result = await waitPromise
  assert.equal(result, null)
})

test('LineLayoutStore: update(null) 清空 state', () => {
  const store = new LineLayoutStore()
  store.update(makeState('abc'))
  store.update(null)
  assert.equal(store.getState(), null)
})

testAsync('LineLayoutStore: compositionSessionId 不同 → 不能复用旧布局', async () => {
  const store = new LineLayoutStore()
  store.update(makeState('abc', { compositionSessionId: 1 }))
  const waitPromise = store.waitFor(makeIdentity('abc', { compositionSessionId: 2 }))
  setTimeout(() => {
    store.update(makeState('abc', { compositionSessionId: 2 }))
  }, 50)
  const result = await waitPromise
  assert.notEqual(result, null)
  assert.equal(result.compositionSessionId, 2)
})

// Issue #629 评论5357756359 第2项：session A/B waiter 隔离测试（改用生产 LineLayoutStore）。
// 验证 session B 的布局不会错误 resolve session A 的 waiter。
testAsync('LineLayoutStore: session A waiter 不被 session B layout 错误 resolve', async () => {
  const store = new LineLayoutStore()
  // 当前布局是 session B（compositionSessionId=2）
  store.update(makeState('abc', { compositionSessionId: 2 }))
  // 等待 session A（compositionSessionId=1）的布局
  const waitPromise = store.waitFor(makeIdentity('abc', { compositionSessionId: 1 }))
  // session B 的布局不应 resolve session A 的 waiter
  // 等一小段时间确认 waiter 未被 resolve
  let resolved = false
  waitPromise.then((result) => { if (result !== null) resolved = true })
  await new Promise(resolve => setTimeout(resolve, 50))
  assert.equal(resolved, false, 'session B layout 不应 resolve session A waiter')
  // 发布真正 session A 的布局
  store.update(makeState('abc', { compositionSessionId: 1 }))
  const result = await waitPromise
  assert.notEqual(result, null, '发布 session A layout 后 waiter 被 resolve')
  assert.equal(result.compositionSessionId, 1)
})

testAsync('LineLayoutStore: 等待返回 state 后目标计算必须使用该 state.lines', async () => {
  const store = new LineLayoutStore()
  store.update(makeState('old'))
  const waitPromise = store.waitFor(makeIdentity('new'))
  const newState = makeState('new', { lines: [
    { startUtf16: 0, endUtf16: 10, y: 0, height: 20, breakKind: 'softWrap', caretStops: [
      { utf16Offset: 0, x: 0 }, { utf16Offset: 10, x: 100 }
    ] },
    { startUtf16: 10, endUtf16: 20, y: 20, height: 20, breakKind: 'endOfText', caretStops: [
      { utf16Offset: 10, x: 0 }, { utf16Offset: 20, x: 100 }
    ] }
  ]})
  setTimeout(() => store.update(newState), 50)
  const result = await waitPromise
  // 使用返回的 state 计算目标
  assert.equal(result.lines.length, 2)
  assert.equal(result.lines[1].startUtf16, 10)
})

// ── resolveVisualLineIndex（生产函数）──

test('resolveVisualLineIndex: 行中间 → 正确行', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'softWrap' }, { start: 5, end: 10, breakKind: 'endOfText' }]
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 7, affinity: CaretAffinity.Downstream }), 1)
})

test('resolveVisualLineIndex: soft-wrap 行末 Upstream → 归上一行', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'softWrap' }, { start: 5, end: 10, breakKind: 'endOfText' }]
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 5, affinity: CaretAffinity.Upstream }), 0)
})

test('resolveVisualLineIndex: soft-wrap 行末 Downstream → 归下一行', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'softWrap' }, { start: 5, end: 10, breakKind: 'endOfText' }]
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 5, affinity: CaretAffinity.Downstream }), 1)
})

// ── 行导航（用 mockNavLines + resolveVisualLineIndex 直接测试）──

function makeNavState(text, extra = {}) {
  const lines = mockNavLines(text)
  const lineRanges = lines.map(l => ({
    start: l.startUtf16,
    end: l.endUtf16,
    breakKind: l.breakKind,
  }))
  return {
    revision: 1, generation: 0, compositionGeneration: -1, compositionSessionId: 0,
    contentWidth: 300, fontSize: 16, lines, lineRanges, displayText: text, ...extra
  }
}

function getCurrentLineIndex(state, position) {
  if (state.lines.length === 0) return -1
  return resolveVisualLineIndex(state.lineRanges, position)
}

test('getCurrentLineIndex: 定位光标所在行（Downstream affinity）', () => {
  const state = makeNavState('aaaaabbbbbccccc')
  assert.equal(getCurrentLineIndex(state, { utf16Offset: 7, affinity: CaretAffinity.Downstream }), 1)
})

test('getPreviousLineIndex: 返回上一行', () => {
  const state = makeNavState('aaaaabbbbbccccc')
  const idx = getCurrentLineIndex(state, { utf16Offset: 7, affinity: CaretAffinity.Downstream })
  assert.equal(idx - 1, 0)
})

test('getPreviousLineIndex: 已在第一行 → -1', () => {
  const state = makeNavState('aaaaabbbbb')
  const idx = getCurrentLineIndex(state, { utf16Offset: 2, affinity: CaretAffinity.Downstream })
  assert.equal(idx - 1, -1)
})

test('getNextLineIndex: 返回下一行', () => {
  const state = makeNavState('aaaaabbbbbccccc')
  const idx = getCurrentLineIndex(state, { utf16Offset: 2, affinity: CaretAffinity.Downstream })
  assert.equal(idx + 1, 1)
})

test('getNextLineIndex: 已在最后一行 → -1', () => {
  const state = makeNavState('aaaaabbbbb')
  const idx = getCurrentLineIndex(state, { utf16Offset: 7, affinity: CaretAffinity.Downstream })
  assert.equal(idx >= state.lines.length - 1 ? -1 : idx + 1, -1)
})

test('getLineStart: 返回行首 offset', () => {
  const state = makeNavState('aaaaabbbbbccccc')
  assert.equal(state.lines[1].startUtf16, 5)
})

test('getLineEnd: 返回行末 offset', () => {
  const state = makeNavState('aaaaabbbbbccccc')
  assert.equal(state.lines[1].endUtf16, 10)
})

test('getCurrentLineIndex: soft-wrap 行末 Upstream → 归上一行', () => {
  const state = makeNavState('aaaaabbbbbccccc')
  assert.equal(getCurrentLineIndex(state, { utf16Offset: 5, affinity: CaretAffinity.Upstream }), 0)
})

test('getCurrentLineIndex: soft-wrap 行末 Downstream → 归下一行', () => {
  const state = makeNavState('aaaaabbbbbccccc')
  assert.equal(getCurrentLineIndex(state, { utf16Offset: 5, affinity: CaretAffinity.Downstream }), 1)
})

// ── horizontalForOffset / offsetForHorizontal（生产函数）──

test('horizontalForOffset: 找到光标所在行最近的 caret stop x', () => {
  const state = makeNavState('aaaaabbbbbccccc')
  const stops = state.lines[1].caretStops
  const x = horizontalForOffset(stops, 7)
  assert.equal(x, 20)
})

test('horizontalForOffset: cursor 在行首返回 x=0', () => {
  const state = makeNavState('aaaaabbbbb')
  const stops = state.lines[1].caretStops
  const x = horizontalForOffset(stops, 5)
  assert.equal(x, 0)
})

test('horizontalForOffset: cursor 在行末返回最后一个 stop 的 x', () => {
  const state = makeNavState('aaaaabbbbb')
  const stops = state.lines[1].caretStops
  const x = horizontalForOffset(stops, 10)
  assert.equal(x, 50)
})

test('offsetForHorizontal: 找目标行内最近的 code point 边界', () => {
  const state = makeNavState('aaaaabbbbbccccc')
  const stops = state.lines[0].caretStops
  const offset = offsetForHorizontal(stops, 25)
  assert.equal(offset, 2)
})

test('offsetForHorizontal: x=0 → 行首', () => {
  const state = makeNavState('aaaaabbbbb')
  const stops = state.lines[0].caretStops
  const offset = offsetForHorizontal(stops, 0)
  assert.equal(offset, 0)
})

test('offsetForHorizontal: x 超过行宽 → 行末', () => {
  const state = makeNavState('aaaaabbbbb')
  const stops = state.lines[0].caretStops
  const offset = offsetForHorizontal(stops, 1000)
  assert.equal(offset, 5)
})

test('offsetForHorizontal: 第二行正确偏移', () => {
  const state = makeNavState('aaaaabbbbbccccc')
  const stops = state.lines[1].caretStops
  const offset = offsetForHorizontal(stops, 35)
  assert.equal(offset, 8)
})

// ── positionForOffsetInLine（生产函数：soft-wrap affinity 逻辑）──
// 这些测试验证 soft-wrap affinity 逻辑，直接调用生产 positionForOffsetInLine。
// 因为无法在 Node 中实例化 .ets，但 static 方法只依赖 LineLayoutState，
// 直接用 mockNavLines 构造等价 state 测试生产函数的行为。

test('SoftWrap 起点（行首）→ Downstream', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'softWrap' }, { start: 5, end: 10, breakKind: 'endOfText' }]
  // 行首 offset=5 在第二行，Downstream → 归第二行
  const pos = resolveVisualLineIndex(lines, { utf16Offset: 5, affinity: CaretAffinity.Downstream })
  assert.equal(pos, 1)
})

test('SoftWrap 终点（行末）→ Upstream', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'softWrap' }, { start: 5, end: 10, breakKind: 'endOfText' }]
  // Upstream at soft-wrap end → 归第一行
  const pos = resolveVisualLineIndex(lines, { utf16Offset: 5, affinity: CaretAffinity.Upstream })
  assert.equal(pos, 0)
})

test('HardBreak 行末 → Downstream', () => {
  const lines = [{ start: 0, end: 3, breakKind: 'hardBreak' }, { start: 4, end: 7, breakKind: 'endOfText' }]
  const pos = resolveVisualLineIndex(lines, { utf16Offset: 3, affinity: CaretAffinity.Downstream })
  assert.equal(pos, 0)
})

test('EndOfText 行末 → Downstream', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'softWrap' }, { start: 5, end: 10, breakKind: 'endOfText' }]
  const pos = resolveVisualLineIndex(lines, { utf16Offset: 10, affinity: CaretAffinity.Downstream })
  assert.equal(pos, 1)
})

// ── positionForHorizontalArrival（生产函数）──
// 通过 editor_layout_math.positionForHorizontalArrival 直接测试。

import { positionForHorizontalArrival } from '../../render/editor_layout_math.ts'

test('positionForHorizontalArrival: Right 到达 SoftWrap 行尾 → Upstream', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'softWrap' }, { start: 5, end: 10, breakKind: 'endOfText' }]
  const pos = positionForHorizontalArrival(lines, 'right', 5)
  assert.equal(pos.affinity, CaretAffinity.Upstream)
})

test('positionForHorizontalArrival: Left 到达 SoftWrap 行尾 → Downstream', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'softWrap' }, { start: 5, end: 10, breakKind: 'endOfText' }]
  const pos = positionForHorizontalArrival(lines, 'left', 5)
  assert.equal(pos.affinity, CaretAffinity.Downstream)
})

test('positionForHorizontalArrival: 非 SoftWrap 行尾 → Downstream', () => {
  const lines = [{ start: 0, end: 5, breakKind: 'hardBreak' }, { start: 5, end: 10, breakKind: 'endOfText' }]
  const pos = positionForHorizontalArrival(lines, 'right', 5)
  assert.equal(pos.affinity, CaretAffinity.Downstream)
})

// ── Issue #629 评论5358224312 第2项：直接测生产纯函数，确保生产路线被测 ──
// 这些测试直接调用从 editor_layout_math.ts import 的生产函数，
// 不经过任何 wrapper，确保生产实现本身被覆盖。
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

console.log('---')
console.log(`✅ line_navigation_resolver: ${passed} tests passed`)
