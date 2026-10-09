// editor_commit_start.test.mjs — 提交瞬间起始状态（按字形重算）的纯逻辑单测。
//
// Issue #879 复核评论6075662695问题3：
// 起始 clip/position 必须按「每个字形此刻在屏是否可见」重算，
// 不能抄旧窗口的绝对 clip，也不能把部分匹配塌成零宽度。
//
// 运行：
//   node apps/harmony/entry/src/main/ets/feature/editor/motion/__tests__/editor_commit_start.test.mjs

import { strict as assert } from 'node:assert'
import { GlyphIdentityTable } from '../editor_glyph_identity.ts'
import {
  visibleGlyphRange, insertRunStartState, deletedRunStartState, retainedMoveStartState,
} from '../editor_commit_start.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

/** 构造一行布局：每个字符宽 10vp，行内 caretStops 与字符边界一一对应。 */
const line = (text, y) => ({
  startUtf16: 0,
  endUtf16: text.length,
  left: 0,
  y: y,
  height: 20,
  breakKind: 'wrap',
  caretStops: Array.from({ length: text.length + 1 }, (_v, i) => ({ utf16Offset: i, x: i * 10 })),
})

/** 构造在屏上下文。 */
const context = (text, revision, frozenWindows) => ({
  text: text,
  layout: [line(text, 0)],
  identities: GlyphIdentityTable.create(text, revision),
  frozenWindows: frozenWindows ?? [],
})

/** 取一段文本里所有字符簇身份。 */
const idsOf = (ctx, from, to) => ctx.identities.idsForRange(from, to).map((e) => e.glyphId)

/** 构造 run 几何（own 侧默认与在屏同一份布局）。 */
const runGeometry = (ctx, from, to, rect) => ({
  glyphIds: idsOf(ctx, from, to),
  ownText: ctx.text,
  ownUtf16Start: from,
  ownUtf16End: to,
  ownRect: rect ?? { x: from * 10, y: 0, width: (to - from) * 10, height: 20 },
  ownLayout: ctx.layout,
})

console.log('editor_commit_start 纯逻辑单测（Issue #879 复核评论6075662695问题3）')

test('静态在屏帧（无运动窗口）：仍然可见的字形段完整保留在 run 自己的区间里', () => {
  const ctx = context('甲乙丙丁', 700, [])
  const run = runGeometry(ctx, 1, 3, { x: 10, y: 0, width: 20, height: 20 })
  const start = deletedRunStartState(run, ctx, 10)
  // 全部字形静态可见 → 起点＝run 自己布局里的完整区间（10..30），不是旧窗口 clip
  assert.equal(start.startClipLeft, 10)
  assert.equal(start.startClipRight, 30)
})

test('冻结窗口只露出一部分：起点按可见字形段换算，不抄窗口绝对 clip', () => {
  const ctx = context('甲乙丙丁', 701, [])
  const all = idsOf(ctx, 0, 4)
  // 旧窗口覆盖 0..4，当前 clip 是 [15,25]：簇1 部分可见、簇2 部分可见、簇3 不可见
  const frozen = [{ glyphIds: all, clipLeft: 15, clipRight: 25, offsetX: 0, offsetY: 0 }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 1, 4, { x: 10, y: 0, width: 30, height: 20 })
  const start = deletedRunStartState(run, withWindow, 10)
  // 可见段＝簇1..簇2 → run 坐标系里是 10..30
  assert.equal(start.startClipLeft, 10)
  assert.equal(start.startClipRight, 30)
})

test('旧窗口大、目标窗口短：不得让新 run 瞬间多露字（不得超出自身区间）', () => {
  const ctx = context('甲乙丙丁', 702, [])
  const all = idsOf(ctx, 0, 4)
  // 旧窗口覆盖 4 个字且整段可见（clip 0..40），新 run 只覆盖前 2 个字
  const frozen = [{ glyphIds: all, clipLeft: 0, clipRight: 40, offsetX: 0, offsetY: 0 }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 2, { x: 0, y: 0, width: 20, height: 20 })
  const start = deletedRunStartState(run, withWindow, 0)
  // 起点只能是 run 自己的 0..20，不能变成旧窗口的 0..40
  assert.equal(start.startClipLeft, 0)
  assert.equal(start.startClipRight, 20)
})

test('未被窗口覆盖的字形属于静态主文本：可见段按最长连续可见段取', () => {
  const ctx = context('甲乙丙丁', 703, [])
  const ids = idsOf(ctx, 0, 4)
  // 只有簇2被窗口接管且当前完全不可见；簇0、簇1、簇3 仍是静态主文本
  const frozen = [{ glyphIds: [ids[2]], clipLeft: 0, clipRight: 0, offsetX: 0, offsetY: 0 }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const range = visibleGlyphRange(run, withWindow)
  // 最长连续可见段是 0..1（簇3 被不可见的簇2 隔开）
  assert.equal(range.firstIndex, 0)
  assert.equal(range.lastIndex, 1)
  assert.equal(range.ownLeft, 0)
  assert.equal(range.ownRight, 20)
})

test('真正的新字（不在在屏身份表里）：零宽度起点', () => {
  const ctx = context('甲乙', 704, [])
  const run = {
    glyphIds: ['ins-999-0', 'ins-999-1'],
    ownText: 'AB',
    ownUtf16Start: 0,
    ownUtf16End: 2,
    ownRect: { x: 50, y: 0, width: 20, height: 20 },
    ownLayout: [{ ...line('AB', 0), left: 50 }],
  }
  const start = insertRunStartState(run, ctx)
  assert.equal(start.startClipLeft, 50)
  assert.equal(start.startClipRight, 50)
})

test('吞字 run 全不可见：塌到目标边缘（起点＝终点，不二次运动）', () => {
  const ctx = context('甲乙', 705, [])
  const ids = idsOf(ctx, 0, 1)
  const frozen = [{ glyphIds: ids, clipLeft: 0, clipRight: 0, offsetX: 0, offsetY: 0 }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = deletedRunStartState(run, withWindow, 0)
  assert.equal(start.startClipLeft, 0)
  assert.equal(start.startClipRight, 0)
})

test('保留字平移：起点让第一个仍可见的字形停在此刻在屏的位置', () => {
  const ctx = context('甲乙丙丁', 706, [])
  const all = idsOf(ctx, 0, 4)
  // 旧窗口整体右移 5vp，整段仍可见
  const frozen = [{ glyphIds: all, clipLeft: 5, clipRight: 45, offsetX: 5, offsetY: 0 }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  assert.equal(start.startPositionX, 5)
  assert.equal(start.startPositionY, 0)
})

test('保留字平移：只有后半段可见时，按第一个可见字形做局部校正', () => {
  const ctx = context('甲乙丙丁戊己', 707, [])
  const all = idsOf(ctx, 0, 6)
  // 窗口右移 5，但左侧被裁掉一部分：只剩簇3..簇5 可见
  const frozen = [{ glyphIds: all, clipLeft: 35, clipRight: 65, offsetX: 5, offsetY: 0 }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 6, { x: 0, y: 0, width: 60, height: 20 })
  const range = visibleGlyphRange(run, withWindow)
  assert.equal(range.firstIndex, 3)
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 60, startPositionX: 0, startPositionY: 0,
  })
  // 第一个可见字形（簇3，own 左界 30）此刻在屏位置 35 → 起点 = 35 - (30 - 0) = 5
  assert.equal(start.startPositionX, 5)
})

test('保留字平移：一个共享字形都不在屏上 → 用兜底位置，不做凭空平移', () => {
  const ctx = context('甲乙丙丁', 708, [])
  const all = idsOf(ctx, 0, 4)
  const frozen = [{ glyphIds: all, clipLeft: 0, clipRight: 0, offsetX: 0, offsetY: 0 }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const fallback = {
    startClipLeft: 0, startClipRight: 40, startPositionX: 100, startPositionY: 20,
  }
  const start = retainedMoveStartState(run, withWindow, fallback)
  assert.equal(start.startPositionX, 100)
  assert.equal(start.startPositionY, 20)
})

test('簇数与字形身份数不匹配：不猜几何，交给兜底起点', () => {
  const ctx = context('甲乙', 709, [])
  const run = {
    glyphIds: ['orig-709-0'],
    ownText: '甲乙',
    ownUtf16Start: 0,
    ownUtf16End: 2,
    ownRect: { x: 0, y: 0, width: 20, height: 20 },
    ownLayout: ctx.layout,
  }
  assert.equal(visibleGlyphRange(run, ctx), null)
  const start = insertRunStartState(run, ctx)
  assert.equal(start.startClipLeft, 0)
  assert.equal(start.startClipRight, 0)
})

test('身份表不可信（null）：不认字，全部按不可见处理', () => {
  const ctx = context('甲乙', 710, [])
  const blindContext = { ...ctx, identities: null }
  const run = runGeometry(ctx, 0, 2, { x: 0, y: 0, width: 20, height: 20 })
  assert.equal(visibleGlyphRange(run, blindContext), null)
})

console.log(`\n✅ editor_commit_start: ${passed} tests passed`)
