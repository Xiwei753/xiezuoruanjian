// editor_commit_start.test.mjs — 提交瞬间起始状态（按字形重算）的纯逻辑单测。
//
// Issue #879 复核评论6078187962 的 3 个核心问题修复：
// 问题1：部分裁切的字形在交棒时跳字 → clusterVisible 返回精确可见区间
// 问题2：不连续的多段可见字形被压成"最长一段" → visibleGlyphPieces 返回全部可见段
// 问题3：ghost字形（已删除但仍被运动窗口绘制）→ 冻结窗口作为独立可见来源
//
// Issue #879 复核评论6078682695 的 4 个问题修复：
// 问题1：多段可见片段不再合并成连续矩形 → insertRunStartState/deletedRunStartState/retainedMoveStartState
//       返回 RunStartPiece[]，每个 piece 生成独立 MotionGlyphWindow
// 问题2：旧删除 ghost 的源布局按 sourceRevision 查（不按 sourceKind 猜）
// 问题3：窗口实例 ID 引用计数 → addLease/releaseLease
// 问题4：精确可见像素区间跨字体/换行几何时投影到目标局部位置
//
// 运行：
//   node apps/harmony/entry/src/main/ets/feature/editor/motion/__tests__/editor_commit_start.test.mjs

import { strict as assert } from 'node:assert'
import { GlyphIdentityTable } from '../editor_glyph_identity.ts'
import {
  visibleGlyphPieces, insertRunStartState, deletedRunStartState, retainedMoveStartState,
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

/** 构造冻结窗口（含 sourceRevision、glyphUtf16Ranges 和 sourceLayout）。 */
const frozenWindow = (glyphIds, clipLeft, clipRight, offsetX, offsetY, sourceRevision, glyphUtf16Ranges, sourceLayout) => ({
  glyphIds,
  clipLeft,
  clipRight,
  offsetX: offsetX ?? 0,
  offsetY: offsetY ?? 0,
  sourceRevision: sourceRevision ?? 0,
  glyphUtf16Ranges: glyphUtf16Ranges ?? null,
  sourceLayout: sourceLayout ?? null,
})

console.log('editor_commit_start 纯逻辑单测（Issue #879 复核评论6078187962 + 6078682695）')

// ====== 问题1：部分裁切的字形在交棒时跳字 ======

test('问题1：静态在屏帧（无运动窗口）——仍然可见的字形段完整保留在 run 自己的区间里', () => {
  const ctx = context('甲乙丙丁', 700, [])
  const run = runGeometry(ctx, 1, 3, { x: 10, y: 0, width: 20, height: 20 })
  const start = deletedRunStartState(run, ctx, 10)
  // 全部字形静态可见 → 起点＝run 自己布局里的完整区间（10..30），不是旧窗口 clip
  assert.equal(start[0].startClipLeft, 10)
  assert.equal(start[0].startClipRight, 30)
})

test('问题1：冻结窗口只露出一部分——起点必须是精确裁切区间 [15,25]，不能是 [10,30]', () => {
  const ctx = context('甲乙丙丁', 701, [])
  const all = idsOf(ctx, 0, 4)
  // 旧窗口覆盖 0..4，当前 clip 是 [15,25]：
  // 簇0在屏 [0,10]，簇1在屏 [10,20]，簇2在屏 [20,30]，簇3在屏 [30,40]
  // clip [15,25] 覆盖：簇1部分可见 [15,20]，簇2部分可见 [20,25]，簇0和簇3不可见
  // 可见段 = 簇1..簇2
  // 簇1在 run 布局中的完整边界 = [10,20]，但精确裁切后 ownLeft = max(10, 15-0) = 15
  // 簇2在 run 布局中的完整边界 = [20,30]，但精确裁切后 ownRight = min(30, 25-0) = 25
  const frozen = [frozenWindow(all, 15, 25, 0, 0, 701, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 1, 4, { x: 10, y: 0, width: 30, height: 20 })
  const start = deletedRunStartState(run, withWindow, 10)
  // 起点必须是精确裁切区间 [15,25]，不能是完整边界 [10,30]
  assert.equal(start[0].startClipLeft, 15)
  assert.equal(start[0].startClipRight, 25)
})

test('问题1：旧窗口大、目标窗口短——不得让新 run 瞬间多露字（不得超出自身区间）', () => {
  const ctx = context('甲乙丙丁', 702, [])
  const all = idsOf(ctx, 0, 4)
  // 旧窗口覆盖 4 个字且整段可见（clip 0..40），新 run 只覆盖前 2 个字
  const frozen = [frozenWindow(all, 0, 40, 0, 0, 702, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 2, { x: 0, y: 0, width: 20, height: 20 })
  const start = deletedRunStartState(run, withWindow, 0)
  // 起点只能是 run 自己的 0..20，不能变成旧窗口的 0..40
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 20)
})

test('问题1：未被窗口覆盖的字形属于静态主文本——可见段按最长连续可见段取', () => {
  const ctx = context('甲乙丙丁', 703, [])
  const ids = idsOf(ctx, 0, 4)
  // 只有簇2被窗口接管且当前完全不可见；簇0、簇1、簇3 仍是静态主文本
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 703, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  // 簇0、簇1 可见（一段），簇3 可见（另一段）——两段都被保留
  assert.equal(pieces.length, 2)
  // 第一段：簇0..簇1
  assert.equal(pieces[0].firstIndex, 0)
  assert.equal(pieces[0].lastIndex, 1)
  assert.equal(pieces[0].ownLeft, 0)
  assert.equal(pieces[0].ownRight, 20)
  // 第二段：簇3
  assert.equal(pieces[1].firstIndex, 3)
  assert.equal(pieces[1].lastIndex, 3)
  assert.equal(pieces[1].ownLeft, 30)
  assert.equal(pieces[1].ownRight, 40)
})

test('问题1：真正的新字（不在在屏身份表里）——零宽度起点', () => {
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
  assert.equal(start[0].startClipLeft, 50)
  assert.equal(start[0].startClipRight, 50)
})

test('问题1：吞字 run 全不可见——塌到目标边缘（起点＝终点，不二次运动）', () => {
  const ctx = context('甲乙', 705, [])
  const ids = idsOf(ctx, 0, 1)
  const frozen = [frozenWindow(ids, 0, 0, 0, 0, 705, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = deletedRunStartState(run, withWindow, 0)
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 0)
})

test('问题1：保留字平移——起点让第一个仍可见的字形停在此刻在屏的位置', () => {
  const ctx = context('甲乙丙丁', 706, [])
  const all = idsOf(ctx, 0, 4)
  // 旧窗口整体右移 5vp，整段仍可见
  const frozen = [frozenWindow(all, 5, 45, 5, 0, 706, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  assert.equal(start[0].startPositionX, 5)
  assert.equal(start[0].startPositionY, 0)
})

test('问题1：保留字平移——只有后半段可见时，按第一个可见字形做局部校正', () => {
  const ctx = context('甲乙丙丁戊己', 707, [])
  const all = idsOf(ctx, 0, 6)
  // 窗口右移 5，但左侧被裁掉一部分：只剩簇3..簇5 可见
  // 簇3在屏 [35,45]，clip [35,65] → 可见 [35,45]
  // 簇3在 run 布局中 [30,40]，精确裁切 ownLeft = max(30, 35-5) = 30
  const frozen = [frozenWindow(all, 35, 65, 5, 0, 707, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 6, { x: 0, y: 0, width: 60, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces[0].firstIndex, 3)
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 60, startPositionX: 0, startPositionY: 0,
  })
  // 第一个可见字形（簇3，own 左界 30）此刻在屏位置 35 → 起点 = 35 - (30 - 0) = 5
  assert.equal(start[0].startPositionX, 5)
})

test('问题1：保留字平移——一个共享字形都不在屏上 → 用兜底位置，不做凭空平移', () => {
  const ctx = context('甲乙丙丁', 708, [])
  const all = idsOf(ctx, 0, 4)
  const frozen = [frozenWindow(all, 0, 0, 0, 0, 708, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const fallback = {
    startClipLeft: 0, startClipRight: 40, startPositionX: 100, startPositionY: 20,
  }
  const start = retainedMoveStartState(run, withWindow, fallback)
  assert.equal(start[0].startPositionX, 100)
  assert.equal(start[0].startPositionY, 20)
})

test('问题1：簇数与字形身份数不匹配——不猜几何，交给兜底起点', () => {
  const ctx = context('甲乙', 709, [])
  const run = {
    glyphIds: ['orig-709-0'],
    ownText: '甲乙',
    ownUtf16Start: 0,
    ownUtf16End: 2,
    ownRect: { x: 0, y: 0, width: 20, height: 20 },
    ownLayout: ctx.layout,
  }
  assert.equal(visibleGlyphPieces(run, ctx).length, 0)
  const start = insertRunStartState(run, ctx)
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 0)
})

test('问题1：身份表不可信（null）——不认字，全部按不可见处理', () => {
  const ctx = context('甲乙', 710, [])
  const blindContext = { ...ctx, identities: null }
  const run = runGeometry(ctx, 0, 2, { x: 0, y: 0, width: 20, height: 20 })
  assert.equal(visibleGlyphPieces(run, blindContext).length, 0)
})

// ====== 问题2：不连续的多段可见字形被压成"最长一段" ======

test('问题2：多段可见字形——甲乙可见、丙不可见、丁可见 → 两段都被保留', () => {
  const ctx = context('甲乙丙丁', 720, [])
  const ids = idsOf(ctx, 0, 4)
  // 簇2（丙）被窗口接管且完全不可见；簇0、簇1、簇3 可见
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 720, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  // 应该有两段：簇0..簇1 和 簇3
  assert.equal(pieces.length, 2)
  // 第一段：簇0..簇1
  assert.equal(pieces[0].firstIndex, 0)
  assert.equal(pieces[0].lastIndex, 1)
  // 第二段：簇3
  assert.equal(pieces[1].firstIndex, 3)
  assert.equal(pieces[1].lastIndex, 3)
})

test('问题2：多段可见字形——deletedRunStartState 保留两段 piece，不合并并集', () => {
  const ctx = context('甲乙丙丁', 721, [])
  const ids = idsOf(ctx, 0, 4)
  // 簇2（丙）被窗口接管且完全不可见；簇0、簇1、簇3 可见
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 721, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = deletedRunStartState(run, withWindow, 0)
  // Issue #879 复核评论6078682695 问题1：不再合并并集，保留两段独立 piece
  assert.equal(start.length, 2)
  // 第一段：簇0..簇1，clip [0, 20]
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 1)
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 20)
  // 第二段：簇3，clip [30, 40]——丙的空洞绝不填补
  assert.equal(start[1].firstIndex, 3)
  assert.equal(start[1].lastIndex, 3)
  assert.equal(start[1].startClipLeft, 30)
  assert.equal(start[1].startClipRight, 40)
})

test('问题2：多段可见字形——insertRunStartState 保留两段 piece，不合并并集', () => {
  const ctx = context('甲乙丙丁', 722, [])
  const ids = idsOf(ctx, 0, 4)
  // 簇2（丙）被窗口接管且完全不可见；簇0、簇1、簇3 可见
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 722, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = insertRunStartState(run, withWindow)
  // Issue #879 复核评论6078682695 问题1：不再合并并集，保留两段独立 piece
  assert.equal(start.length, 2)
  // 第一段：簇0..簇1，clip [0, 20]
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 1)
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 20)
  // 第二段：簇3，clip [30, 40]
  assert.equal(start[1].firstIndex, 3)
  assert.equal(start[1].lastIndex, 3)
  assert.equal(start[1].startClipLeft, 30)
  assert.equal(start[1].startClipRight, 40)
})

test('问题2：多段可见字形——retainedMoveStartState 保留两段 piece，不合并并集', () => {
  const ctx = context('甲乙丙丁', 723, [])
  const ids = idsOf(ctx, 0, 4)
  // 簇2（丙）被窗口接管且完全不可见；簇0、簇1、簇3 可见
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 723, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  // Issue #879 复核评论6078682695 问题1：不再合并并集，保留两段独立 piece
  assert.equal(start.length, 2)
  // 第一段：簇0..簇1
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 1)
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 20)
  // 簇0是静态可见，在屏位置 = ownLeft = 0 → startPositionX = 0 - (0 - 0) = 0
  assert.equal(start[0].startPositionX, 0)
  // 第二段：簇3
  assert.equal(start[1].firstIndex, 3)
  assert.equal(start[1].lastIndex, 3)
  assert.equal(start[1].startClipLeft, 30)
  assert.equal(start[1].startClipRight, 40)
})

test('问题2：三段可见——甲可见、乙不可见、丙可见、丁不可见、戊可见', () => {
  const ctx = context('甲乙丙丁戊', 724, [])
  const ids = idsOf(ctx, 0, 5)
  // 簇1（乙）和簇3（丁）不可见；簇0、簇2、簇4 可见
  const frozen = [
    frozenWindow([ids[1]], 0, 0, 0, 0, 724, null),
    frozenWindow([ids[3]], 0, 0, 0, 0, 724, null),
  ]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  // 应该有三段：簇0、簇2、簇4
  assert.equal(pieces.length, 3)
  assert.equal(pieces[0].firstIndex, 0)
  assert.equal(pieces[0].lastIndex, 0)
  assert.equal(pieces[1].firstIndex, 2)
  assert.equal(pieces[1].lastIndex, 2)
  assert.equal(pieces[2].firstIndex, 4)
  assert.equal(pieces[2].lastIndex, 4)
})

// ====== 问题3：ghost字形（已删除但仍被运动窗口绘制） ======

test('问题3：ghost字形——字形已从 displayed 正文删除，但冻结窗口仍在绘制 → 仍可见', () => {
  // displayed 正文是 '甲乙'（丙丁已被删除），但旧窗口仍在绘制丙丁
  const ctx = context('甲乙', 730, [])
  // 旧正文是 '甲乙丙丁'，丙丁的 glyphId 是 orig-730-2 和 orig-730-3
  // 但在 displayed 正文 '甲乙' 中查不到这两个 glyphId
  // 冻结窗口仍在绘制它们，glyphUtf16Ranges 提供它们在源修订正文中的 UTF-16 区间
  const ghostIds = ['orig-730-2', 'orig-730-3']
  const ghostRanges = new Map([
    ['orig-730-2', [2, 3]],  // 丙在源修订正文中的 UTF-16 区间
    ['orig-730-3', [3, 4]],  // 丁在源修订正文中的 UTF-16 区间
  ])
  // sourceLayout 是源修订正文 '甲乙丙丁' 的行布局
  const sourceLayout = [line('甲乙丙丁', 0)]
  // 窗口 clip [20, 40]，offsetX=0：丙在屏 [20,30] 可见，丁在屏 [30,40] 可见
  const frozen = [frozenWindow(ghostIds, 20, 40, 0, 0, 730, ghostRanges, sourceLayout)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  // run 覆盖丙丁（在旧正文中）
  const run = {
    glyphIds: ghostIds,
    ownText: '甲乙丙丁',
    ownUtf16Start: 2,
    ownUtf16End: 4,
    ownRect: { x: 20, y: 0, width: 20, height: 20 },
    ownLayout: [line('甲乙丙丁', 0)],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  // ghost字形应该可见
  assert.equal(pieces.length, 1)
  assert.equal(pieces[0].firstIndex, 0)
  assert.equal(pieces[0].lastIndex, 1)
  // ownLeft/ownRight 应该是精确裁切区间
  // 丙在 run 布局中 [20,30]，clip [20,40] → ownLeft = max(20, 20-0) = 20
  // 丁在 run 布局中 [30,40]，clip [20,40] → ownRight = min(40, 40-0) = 40
  assert.equal(pieces[0].ownLeft, 20)
  assert.equal(pieces[0].ownRight, 40)
})

test('问题3：ghost字形部分裁切——只露出一部分', () => {
  // displayed 正文是 '甲乙'（丙丁已被删除），但旧窗口仍在绘制丙丁
  const ctx = context('甲乙', 731, [])
  const ghostIds = ['orig-731-2', 'orig-731-3']
  const ghostRanges = new Map([
    ['orig-731-2', [2, 3]],
    ['orig-731-3', [3, 4]],
  ])
  const sourceLayout = [line('甲乙丙丁', 0)]
  // 窗口 clip [25, 35]，offsetX=0：
  // 丙在屏 [20,30]，可见 [25,30]（5vp）
  // 丁在屏 [30,40]，可见 [30,35]（5vp）
  const frozen = [frozenWindow(ghostIds, 25, 35, 0, 0, 731, ghostRanges, sourceLayout)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: ghostIds,
    ownText: '甲乙丙丁',
    ownUtf16Start: 2,
    ownUtf16End: 4,
    ownRect: { x: 20, y: 0, width: 20, height: 20 },
    ownLayout: [line('甲乙丙丁', 0)],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 1)
  // ownLeft = max(20, 25-0) = 25（精确裁切，不是完整边界 20）
  assert.equal(pieces[0].ownLeft, 25)
  // ownRight = min(40, 35-0) = 35（精确裁切，不是完整边界 40）
  assert.equal(pieces[0].ownRight, 35)
})

test('问题3：ghost字形完全不可见——窗口 clip 为零', () => {
  const ctx = context('甲乙', 732, [])
  const ghostIds = ['orig-732-2', 'orig-732-3']
  const ghostRanges = new Map([
    ['orig-732-2', [2, 3]],
    ['orig-732-3', [3, 4]],
  ])
  const sourceLayout = [line('甲乙丙丁', 0)]
  // 窗口 clip [0, 0]：完全不可见
  const frozen = [frozenWindow(ghostIds, 0, 0, 0, 0, 732, ghostRanges, sourceLayout)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: ghostIds,
    ownText: '甲乙丙丁',
    ownUtf16Start: 2,
    ownUtf16End: 4,
    ownRect: { x: 20, y: 0, width: 20, height: 20 },
    ownLayout: [line('甲乙丙丁', 0)],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 0)
})

test('问题3：ghost字形与静态字形混合——甲静态可见、乙ghost可见、丙真正新字', () => {
  // displayed 正文是 '甲丙'（乙已被删除），但旧窗口仍在绘制乙
  const ctx = context('甲丙', 733, [])
  const ids = idsOf(ctx, 0, 2)
  // 甲的 glyphId 在 displayed 身份表中 → 静态可见
  // 乙的 glyphId 不在 displayed 身份表中 → ghost字形（冻结窗口仍在绘制）
  // 丙的 glyphId 在 displayed 身份表中 → 静态可见
  // 但这里 run 覆盖的是旧正文 '甲乙丙' 中的乙
  const ghostId = 'orig-733-1'  // 乙在旧正文中的 glyphId
  const ghostRanges = new Map([
    [ghostId, [1, 2]],  // 乙在源修订正文中的 UTF-16 区间
  ])
  const sourceLayout = [line('甲乙丙', 0)]
  // 窗口 clip [10, 20]，offsetX=0：乙在屏 [10,20] 可见
  const frozen = [frozenWindow([ghostId], 10, 20, 0, 0, 733, ghostRanges, sourceLayout)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  // run 覆盖乙（在旧正文中）
  const run = {
    glyphIds: [ghostId],
    ownText: '甲乙丙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙丙', 0)],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  // ghost字形乙应该可见
  assert.equal(pieces.length, 1)
  assert.equal(pieces[0].firstIndex, 0)
  assert.equal(pieces[0].lastIndex, 0)
  // ownLeft = max(10, 10-0) = 10
  assert.equal(pieces[0].ownLeft, 10)
  // ownRight = min(20, 20-0) = 20
  assert.equal(pieces[0].ownRight, 20)
})

test('问题3：ghost字形——glyphUtf16Ranges 为 null 时不识别 ghost', () => {
  // 如果窗口没有提供 glyphUtf16Ranges，则无法识别 ghost字形
  const ctx = context('甲乙', 734, [])
  const ghostIds = ['orig-734-2', 'orig-734-3']
  // glyphUtf16Ranges 为 null → 无法定位 ghost字形
  const frozen = [frozenWindow(ghostIds, 20, 40, 0, 0, 734, null, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: ghostIds,
    ownText: '甲乙丙丁',
    ownUtf16Start: 2,
    ownUtf16End: 4,
    ownRect: { x: 20, y: 0, width: 20, height: 20 },
    ownLayout: [line('甲乙丙丁', 0)],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  // glyphUtf16Ranges 为 null → 无法识别 ghost字形 → 不可见
  assert.equal(pieces.length, 0)
})

// ====== Issue #879 复核评论6078682695 问题4：跨字体/换行几何投影 ======

test('问题4：源布局和目标布局不同——可见比例投影到目标局部位置', () => {
  // 源布局：字形在 [40, 50]（10vp 宽）
  // 目标布局：同一字形在 [10, 20]（10vp 宽）
  // 冻结窗口 clip [45, 50]：只露出右半（5vp）
  // 投影后：目标中右半 = [15, 20]
  const ctx = context('XXXX甲', 740, [])  // 5 字，甲在 [40, 50]
  const glyphId = idsOf(ctx, 4, 5)[0]     // 甲的 glyphId
  const frozen = [frozenWindow([glyphId], 45, 50, 0, 0, 740, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  // run 的 ownLayout 把甲放在 [10, 20]（不同前缀宽度）
  const run = {
    glyphIds: [glyphId],
    ownText: '甲',
    ownUtf16Start: 0,
    ownUtf16End: 1,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [{
      startUtf16: 0,
      endUtf16: 1,
      left: 10,
      y: 0,
      height: 20,
      breakKind: 'wrap',
      caretStops: [
        { utf16Offset: 0, x: 10 },
        { utf16Offset: 1, x: 20 },
      ],
    }],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 1)
  // 投影后 ownLeft = 10 + (45-40)/(50-40) * (20-10) = 15
  assert.equal(pieces[0].ownLeft, 15)
  // 投影后 ownRight = 10 + (50-40)/(50-40) * (20-10) = 20
  assert.equal(pieces[0].ownRight, 20)
})

test('问题4：源布局和目标布局相同——投影是恒等变换', () => {
  // 源和目标布局相同：投影不应改变坐标
  const ctx = context('甲乙丙丁', 741, [])
  const all = idsOf(ctx, 0, 4)
  // clip [15, 25]：簇1部分可见 [15,20]，簇2部分可见 [20,25]
  const frozen = [frozenWindow(all, 15, 25, 0, 0, 741, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 1)
  // 源＝目标 → ownLeft=15, ownRight=25（恒等）
  assert.equal(pieces[0].ownLeft, 15)
  assert.equal(pieces[0].ownRight, 25)
})

test('问题4：多字形 piece 跨字体投影——首尾字形各自投影到自己的目标边界', () => {
  // 源布局：簇0在 [0,10]，簇1在 [10,20]，簇2在 [20,30]
  // 目标布局：簇0在 [0,20]，簇1在 [20,40]，簇2在 [40,60]（每个字形宽 20vp）
  // 冻结窗口 clip [5, 25]：簇0右半 [5,10] 可见，簇1全 [10,20] 可见，簇2左半 [20,25] 可见
  // 投影后：
  //   簇0 ownLeft = 0 + (5-0)/(10-0) * (20-0) = 10
  //   簇2 ownRight = 40 + (25-20)/(30-20) * (60-40) = 50
  const ctx = context('甲乙丙', 742, [])
  const all = idsOf(ctx, 0, 3)
  const frozen = [frozenWindow(all, 5, 25, 0, 0, 742, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  // run 的 ownLayout 每个字形宽 20vp
  const run = {
    glyphIds: all,
    ownText: '甲乙丙',
    ownUtf16Start: 0,
    ownUtf16End: 3,
    ownRect: { x: 0, y: 0, width: 60, height: 20 },
    ownLayout: [{
      startUtf16: 0,
      endUtf16: 3,
      left: 0,
      y: 0,
      height: 20,
      breakKind: 'wrap',
      caretStops: [
        { utf16Offset: 0, x: 0 },
        { utf16Offset: 1, x: 20 },
        { utf16Offset: 2, x: 40 },
        { utf16Offset: 3, x: 60 },
      ],
    }],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 1)
  // 簇0 ownLeft = 0 + (5-0)/(10-0) * (20-0) = 10
  assert.equal(pieces[0].ownLeft, 10)
  // 簇2 ownRight = 40 + (25-20)/(30-20) * (60-40) = 50
  assert.equal(pieces[0].ownRight, 50)
})

// ====== Issue #879 复核评论6078682695 问题2：跨事务 ghost 源布局按 sourceRevision 查 ======

test('问题2：跨事务 ghost——sourceRevision 不同于 displayed revision，源布局仍正确', () => {
  // 场景：rev10 'ab' → rev11 'a'（b 被删除，ghost 仍在动画）→ rev12 'ac'
  // rev12 时，b 的 ghost 仍在消退，其 sourceRevision=10，sourceLayout 是 rev10 的布局
  // displayed 正文是 'ac'（rev12），b 不在 displayed 身份表中
  // 冻结窗口的 sourceRevision=10，sourceLayout 是 'ab' 的布局
  const ctx = context('ac', 750, [])
  const ghostId = 'orig-10-1'  // b 在 rev10 中的 glyphId
  const ghostRanges = new Map([
    [ghostId, [1, 2]],  // b 在 rev10 正文 'ab' 中的 UTF-16 区间
  ])
  // sourceLayout 是 rev10 正文 'ab' 的行布局：a 在 [0,10]，b 在 [10,20]
  const sourceLayout = [line('ab', 0)]
  // 窗口 clip [10, 20]，offsetX=0：b 在屏 [10,20] 可见（仍在消退中）
  const frozen = [frozenWindow([ghostId], 10, 20, 0, 0, 10, ghostRanges, sourceLayout)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  // run 覆盖 b（在 rev10 正文中）
  const run = {
    glyphIds: [ghostId],
    ownText: 'ab',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('ab', 0)],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  // ghost 字形 b 应该可见——源布局按 sourceRevision=10 查到，不按 sourceKind 猜
  assert.equal(pieces.length, 1)
  assert.equal(pieces[0].firstIndex, 0)
  assert.equal(pieces[0].lastIndex, 0)
  // ownLeft = max(10, 10-0) = 10
  assert.equal(pieces[0].ownLeft, 10)
  // ownRight = min(20, 20-0) = 20
  assert.equal(pieces[0].ownRight, 20)
  // sourceGlyphX0/X1 应来自 sourceLayout（rev10 的布局）
  assert.equal(pieces[0].sourceGlyphX0, 10)
  assert.equal(pieces[0].sourceGlyphX1, 20)
})

test('问题2：跨事务 ghost——源布局与目标布局不同时投影正确', () => {
  // 场景：rev10 'ab' → rev11 'a'（b 被删除）→ rev12 'ac'
  // b 的 ghost 源布局（rev10）：b 在 [10,20]
  // run 的目标布局：b 在 [30,40]（不同位置）
  // 冻结窗口 clip [15, 20]：只露出 b 的右半
  // 投影后：目标中右半 = [35, 40]
  const ctx = context('ac', 751, [])
  const ghostId = 'orig-10-1'
  const ghostRanges = new Map([
    [ghostId, [1, 2]],
  ])
  const sourceLayout = [line('ab', 0)]  // b 在 [10,20]
  const frozen = [frozenWindow([ghostId], 15, 20, 0, 0, 10, ghostRanges, sourceLayout)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  // run 的 ownLayout 把 b 放在 [30, 40]
  const run = {
    glyphIds: [ghostId],
    ownText: 'ab',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 30, y: 0, width: 10, height: 20 },
    ownLayout: [{
      startUtf16: 0,
      endUtf16: 2,
      left: 0,
      y: 0,
      height: 20,
      breakKind: 'wrap',
      caretStops: [
        { utf16Offset: 0, x: 0 },
        { utf16Offset: 1, x: 30 },
        { utf16Offset: 2, x: 40 },
      ],
    }],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 1)
  // 投影：ratio = (15-10)/(20-10) = 0.5 → ownLeft = 30 + 0.5 * (40-30) = 35
  assert.equal(pieces[0].ownLeft, 35)
  // 投影：ratio = (20-10)/(20-10) = 1.0 → ownRight = 30 + 1.0 * (40-30) = 40
  assert.equal(pieces[0].ownRight, 40)
})

console.log(`\n✅ editor_commit_start: ${passed} tests passed`)
