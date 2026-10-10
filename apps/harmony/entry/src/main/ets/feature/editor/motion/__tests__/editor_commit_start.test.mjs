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
import { GlyphIdentityTable, WindowInstanceIdAllocator } from '../editor_glyph_identity.ts'
import {
  visibleGlyphPieces, insertRunStartState, deletedRunStartState, retainedMoveStartState,
  retargetRunStarts,
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
  // Issue #879 评论6096421590 问题2：从 clipLeft/clipRight 自动生成 clipRects，
  // 供 clusterVisible/buildPiece 逐岛求交。测试中字形高度默认 20（见 runGeometry）。
  clipRects: [{ x: clipLeft, y: 0, width: Math.max(0, clipRight - clipLeft), height: 20 }],
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

test('问题2：多段可见字形——insertRunStartState 保留两段可见 piece + 间隙 0宽度 piece', () => {
  const ctx = context('甲乙丙丁', 722, [])
  const ids = idsOf(ctx, 0, 4)
  // 簇2（丙）被窗口接管且完全不可见；簇0、簇1、簇3 可见
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 722, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = insertRunStartState(run, withWindow)
  // 修复后：3 个 piece = [0..1 可见] + [2 间隙 0宽度] + [3 可见]
  // 间隙 piece 以 0 宽度进入，动画期间逐步吐出，不会在第一帧"冒出来"
  assert.equal(start.length, 3)
  // 第一段：簇0..簇1，clip [0, 20]
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 1)
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 20)
  // 第二段：簇2，间隙（0 宽度）——丙在第一帧不可见，但 piece 存在以便动画逐步吐出
  // Issue #879 复核评论6080604353 问题2：gap 的 startClipLeft/Right 用 gap 自身在 ownLayout
  // 中的左边界（丙在 [20,30]，左边界=20），而不是整 run 的 ownRect.x（0）
  assert.equal(start[1].firstIndex, 2)
  assert.equal(start[1].lastIndex, 2)
  assert.equal(start[1].startClipLeft, 20)
  assert.equal(start[1].startClipRight, 20)
  assert.deepEqual(start[1].glyphIds, [ids[2]])
  // 第三段：簇3，clip [30, 40]
  assert.equal(start[2].firstIndex, 3)
  assert.equal(start[2].lastIndex, 3)
  assert.equal(start[2].startClipLeft, 30)
  assert.equal(start[2].startClipRight, 40)
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

// ====== 空数组分支修复：无旧可见字形时应覆盖整段新 run ======

test('空数组分支：纯新插入（所有字都不在旧画面）——piece 应覆盖整段', () => {
  const ctx = context('甲乙', 760, [])
  // 3 个全新插入的字，都不在旧画面里
  const run = {
    glyphIds: ['ins-760-0', 'ins-760-1', 'ins-760-2'],
    ownText: 'ABC',
    ownUtf16Start: 0,
    ownUtf16End: 3,
    ownRect: { x: 50, y: 0, width: 30, height: 20 },
    ownLayout: [{
      startUtf16: 0,
      endUtf16: 3,
      left: 50,
      y: 0,
      height: 20,
      breakKind: 'wrap',
      caretStops: [
        { utf16Offset: 0, x: 50 },
        { utf16Offset: 1, x: 60 },
        { utf16Offset: 2, x: 70 },
        { utf16Offset: 3, x: 80 },
      ],
    }],
  }
  const start = insertRunStartState(run, ctx)
  // 应返回单个 piece，覆盖整段
  assert.equal(start.length, 1)
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 2)
  assert.equal(start[0].glyphIds.length, 3)
  assert.deepEqual(start[0].glyphIds, ['ins-760-0', 'ins-760-1', 'ins-760-2'])
  // 零宽度起点
  assert.equal(start[0].startClipLeft, 50)
  assert.equal(start[0].startClipRight, 50)
})

test('空数组分支：纯新插入多字——不再只返回第一个字形', () => {
  const ctx = context('甲', 761, [])
  // 5 个全新插入的字
  const run = {
    glyphIds: ['ins-761-0', 'ins-761-1', 'ins-761-2', 'ins-761-3', 'ins-761-4'],
    ownText: 'ABCDE',
    ownUtf16Start: 0,
    ownUtf16End: 5,
    ownRect: { x: 100, y: 0, width: 50, height: 20 },
    ownLayout: [{
      startUtf16: 0,
      endUtf16: 5,
      left: 100,
      y: 0,
      height: 20,
      breakKind: 'wrap',
      caretStops: Array.from({ length: 6 }, (_v, i) => ({ utf16Offset: i, x: 100 + i * 10 })),
    }],
  }
  const start = insertRunStartState(run, ctx)
  assert.equal(start.length, 1)
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 4)
  assert.equal(start[0].glyphIds.length, 5)
  assert.deepEqual(start[0].glyphIds, ['ins-761-0', 'ins-761-1', 'ins-761-2', 'ins-761-3', 'ins-761-4'])
})

test('混合 run：部分旧可见+部分新插入——间隙作为 0 宽度 piece', () => {
  // displayed 正文 '甲乙丙丁戊'，run 覆盖 0..5
  // 簇0（甲）、簇1（乙）静态可见，簇2（丙）被窗口接管且不可见，
  // 簇3（丁）是新插入字（不在身份表中），簇4（戊）静态可见
  const ctx = context('甲乙丙丁戊', 762, [])
  const ids = idsOf(ctx, 0, 5)
  // 簇2（丙）被窗口接管且完全不可见
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 762, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  // run 的 glyphIds：簇0、簇1、簇2 是旧字（在身份表中），簇3 是新字，簇4 是旧字
  // 但簇3 不在身份表中——需要构造一个不在身份表中的 glyphId
  const run = {
    glyphIds: [ids[0], ids[1], ids[2], 'ins-762-3', ids[4]],
    ownText: '甲乙丙X戊',
    ownUtf16Start: 0,
    ownUtf16End: 5,
    ownRect: { x: 0, y: 0, width: 50, height: 20 },
    ownLayout: [{
      startUtf16: 0,
      endUtf16: 5,
      left: 0,
      y: 0,
      height: 20,
      breakKind: 'wrap',
      caretStops: Array.from({ length: 6 }, (_v, i) => ({ utf16Offset: i, x: i * 10 })),
    }],
  }
  const start = insertRunStartState(run, withWindow)
  // visibleGlyphPieces 会返回：簇0..簇1（可见），簇4（可见）
  // 簇2 不可见（窗口接管且 clip=0），簇3 不在身份表中（新字）
  // 间隙：index 2（簇2，不可见的旧字）和 index 3（簇3，新字）
  // 应该有 3 个 piece：[0..1 可见], [2..3 间隙 0宽度], [4 可见]
  assert.equal(start.length, 3)
  // 第一段：簇0..簇1，可见
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 1)
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 20)
  assert.deepEqual(start[0].glyphIds, [ids[0], ids[1]])
  // 第二段：簇2..簇3，间隙（0 宽度）
  // Issue #879 复核评论6080604353 问题2：gap 左边界 = 簇2在 ownLayout 中的 x = 20
  assert.equal(start[1].firstIndex, 2)
  assert.equal(start[1].lastIndex, 3)
  assert.equal(start[1].startClipLeft, 20)
  assert.equal(start[1].startClipRight, 20)
  assert.deepEqual(start[1].glyphIds, [ids[2], 'ins-762-3'])
  // 第三段：簇4，可见
  assert.equal(start[2].firstIndex, 4)
  assert.equal(start[2].lastIndex, 4)
  assert.equal(start[2].startClipLeft, 40)
  assert.equal(start[2].startClipRight, 50)
  assert.deepEqual(start[2].glyphIds, [ids[4]])
})

test('混合 run：头部新插入+尾部旧可见——头部间隙作为 0 宽度 piece', () => {
  // run 的前 2 个字是新插入的（不在身份表中），后 2 个字是旧可见
  const ctx = context('甲乙', 763, [])
  const ids = idsOf(ctx, 0, 2)
  const run = {
    glyphIds: ['ins-763-0', 'ins-763-1', ids[0], ids[1]],
    ownText: 'XY甲乙',
    ownUtf16Start: 0,
    ownUtf16End: 4,
    ownRect: { x: 0, y: 0, width: 40, height: 20 },
    ownLayout: [{
      startUtf16: 0,
      endUtf16: 4,
      left: 0,
      y: 0,
      height: 20,
      breakKind: 'wrap',
      caretStops: Array.from({ length: 5 }, (_v, i) => ({ utf16Offset: i, x: i * 10 })),
    }],
  }
  const start = insertRunStartState(run, ctx)
  // visibleGlyphPieces 返回：簇2..簇3（甲乙可见）
  // 间隙：index 0..1（新插入字）
  // 应该有 2 个 piece：[0..1 间隙 0宽度], [2..3 可见]
  assert.equal(start.length, 2)
  // 第一段：簇0..簇1，间隙（0 宽度）
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 1)
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 0)
  assert.deepEqual(start[0].glyphIds, ['ins-763-0', 'ins-763-1'])
  // 第二段：簇2..簇3，可见
  assert.equal(start[1].firstIndex, 2)
  assert.equal(start[1].lastIndex, 3)
  assert.equal(start[1].startClipLeft, 20)
  assert.equal(start[1].startClipRight, 40)
  assert.deepEqual(start[1].glyphIds, [ids[0], ids[1]])
})

test('混合 run：旧可见+尾部新插入——尾部间隙作为 0 宽度 piece', () => {
  // run 的前 2 个字是旧可见，后 2 个字是新插入的
  const ctx = context('甲乙', 764, [])
  const ids = idsOf(ctx, 0, 2)
  const run = {
    glyphIds: [ids[0], ids[1], 'ins-764-2', 'ins-764-3'],
    ownText: '甲乙XY',
    ownUtf16Start: 0,
    ownUtf16End: 4,
    ownRect: { x: 0, y: 0, width: 40, height: 20 },
    ownLayout: [{
      startUtf16: 0,
      endUtf16: 4,
      left: 0,
      y: 0,
      height: 20,
      breakKind: 'wrap',
      caretStops: Array.from({ length: 5 }, (_v, i) => ({ utf16Offset: i, x: i * 10 })),
    }],
  }
  const start = insertRunStartState(run, ctx)
  // visibleGlyphPieces 返回：簇0..簇1（甲乙可见）
  // 尾部间隙：index 2..3（新插入字）
  // 应该有 2 个 piece：[0..1 可见], [2..3 尾部间隙 0宽度]
  assert.equal(start.length, 2)
  // 第一段：簇0..簇1，可见
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 1)
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 20)
  assert.deepEqual(start[0].glyphIds, [ids[0], ids[1]])
  // 第二段：簇2..簇3，尾部间隙（0 宽度）
  // Issue #879 复核评论6080604353 问题2：尾部 gap 左边界 = 簇2在 ownLayout 中的 x = 20
  assert.equal(start[1].firstIndex, 2)
  assert.equal(start[1].lastIndex, 3)
  assert.equal(start[1].startClipLeft, 20)
  assert.equal(start[1].startClipRight, 20)
  assert.deepEqual(start[1].glyphIds, ['ins-764-2', 'ins-764-3'])
})

test('deletedRunStartState 空数组分支——覆盖整段 glyphIds，不只取第一个', () => {
  const ctx = context('甲乙', 765, [])
  const ids = idsOf(ctx, 0, 2)
  // 窗口覆盖甲乙且完全不可见
  const frozen = [frozenWindow(ids, 0, 0, 0, 0, 765, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 2, { x: 0, y: 0, width: 20, height: 20 })
  const start = deletedRunStartState(run, withWindow, 5)
  assert.equal(start.length, 1)
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 1)
  assert.equal(start[0].glyphIds.length, 2)
  assert.deepEqual(start[0].glyphIds, ids)
  // 塌到 collapseX
  assert.equal(start[0].startClipLeft, 5)
  assert.equal(start[0].startClipRight, 5)
})

test('retainedMoveStartState 空数组分支——覆盖整段 glyphIds，不只取第一个', () => {
  const ctx = context('甲乙丙丁', 766, [])
  const all = idsOf(ctx, 0, 4)
  const frozen = [frozenWindow(all, 0, 0, 0, 0, 766, null)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const fallback = {
    startClipLeft: 0, startClipRight: 40, startPositionX: 100, startPositionY: 20,
  }
  const start = retainedMoveStartState(run, withWindow, fallback)
  assert.equal(start.length, 1)
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 3)
  assert.equal(start[0].glyphIds.length, 4)
  assert.deepEqual(start[0].glyphIds, all)
  // fallback 位置
  assert.equal(start[0].startPositionX, 100)
  assert.equal(start[0].startPositionY, 20)
})

// ====== 问题4：retarget 不得因为临时可见分组更换物理 Text key ======

/** 分区（boundaries + glyphIds）必须逐 piece 完全一致。 */
const assertSamePartition = (before, after) => {
  assert.equal(after.length, before.length, 'piece 数量不得变化')
  for (let i = 0; i < before.length; i++) {
    assert.equal(after[i].firstIndex, before[i].firstIndex, `piece${i} firstIndex 不得变化`)
    assert.equal(after[i].lastIndex, before[i].lastIndex, `piece${i} lastIndex 不得变化`)
    assert.deepEqual(after[i].glyphIds, before[i].glyphIds, `piece${i} glyphIds 不得变化`)
  }
}

test('问题4：可见性变化后 retarget——piece 分区与 glyphIds 完全不变，只重算 clip', () => {
  const ctx = context('甲乙丙丁戊', 780, [])
  const all = idsOf(ctx, 0, 5)
  const run = runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  // prepare：整段被旧窗口覆盖且完整可见（clip 0..50）→ 单个 piece 覆盖 0..4
  const prepared = insertRunStartState({ ...run, ownLayout: ctx.layout }, {
    ...ctx,
    frozenWindows: [frozenWindow(all, 0, 50, 0, 0, 780, null)],
  })
  assert.equal(prepared.length, 1)
  // retarget：同一个窗口缩到 clip 20..30（簇2 完整可见、簇1/簇3 部分可见）——可见分组变了
  const retargeted = retargetRunStarts(
    { ...run, ownLayout: ctx.layout },
    prepared,
    { ...ctx, frozenWindows: [frozenWindow(all, 20, 30, 0, 0, 780, null)] },
    'insert',
    0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  // 分区必须原样保留——否则 glyphIds 变化会让 allocator 分配出新的 win-N，
  // 交棒前刚准备完的物理 Text 节点作废。
  assertSamePartition(prepared, retargeted)
  // clip 却被重算到提交瞬间的真实可见区间
  assert.equal(retargeted[0].startClipLeft, 20)
  assert.equal(retargeted[0].startClipRight, 30)
})

test('问题4：两段可见合成一段——prepare 的两个 piece 仍保留为两个（不合并）', () => {
  const ctx = context('甲乙丙丁戊', 781, [])
  const ids = idsOf(ctx, 0, 5)
  // 簇2 完全不可见（clip 0..0），其余静态可见 → 两段 piece：[0..1] 与 [3..4]
  const prepared = deletedRunStartState(runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 }), {
    ...ctx,
    frozenWindows: [frozenWindow([ids[2]], 0, 0, 0, 0, 781, null)],
  }, 0)
  assert.equal(prepared.length, 2, 'prepare 应有两段可见 piece')
  // retarget：旧窗口已完全消失（空 frozenWindows），整段静态可见 → 可见性变成一整段
  const retargeted = retargetRunStarts(
    runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 }),
    prepared,
    { ...ctx, frozenWindows: [] },
    'deleted',
    0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  // 分区不因「两段合成一段」而改变——两个物理节点各画各自的一段
  assertSamePartition(prepared, retargeted)
  assert.equal(retargeted[0].startClipLeft, 0)
  assert.equal(retargeted[0].startClipRight, 20)
  assert.equal(retargeted[1].startClipLeft, 30)
  assert.equal(retargeted[1].startClipRight, 50)
})

test('问题4：piece 内字形全部不可见——按通道语义塌缩，分区仍不变', () => {
  const ctx = context('甲乙丙丁', 782, [])
  const all = idsOf(ctx, 0, 4)
  const hidden = { ...ctx, frozenWindows: [frozenWindow(all, 0, 0, 0, 0, 782, null)] }
  const run = runGeometry(hidden, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const prepared = insertRunStartState(run, hidden)
  // insert：该 piece 自身左边界零宽度（不是整 run 左边界，也不是别的 run 的位置）
  const insertRetarget = retargetRunStarts(run, prepared, hidden, 'insert', 0,
    { startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0 })
  assertSamePartition(prepared, insertRetarget)
  assert.equal(insertRetarget[0].startClipLeft, 0)
  assert.equal(insertRetarget[0].startClipRight, 0)
  // deleted：塌到 collapseX
  const deletedRetarget = retargetRunStarts(run, prepared, hidden, 'deleted', 7,
    { startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0 })
  assertSamePartition(prepared, deletedRetarget)
  assert.equal(deletedRetarget[0].startClipLeft, 7)
  assert.equal(deletedRetarget[0].startClipRight, 7)
  // retained：用 fallback 起始状态
  const fallback = { startClipLeft: 1, startClipRight: 2, startPositionX: 3, startPositionY: 4 }
  const retainedRetarget = retargetRunStarts(run, prepared, hidden, 'retained', 0, fallback)
  assertSamePartition(prepared, retainedRetarget)
  assert.equal(retainedRetarget[0].startClipLeft, 1)
  assert.equal(retainedRetarget[0].startClipRight, 2)
  assert.equal(retainedRetarget[0].startPositionX, 3)
  assert.equal(retainedRetarget[0].startPositionY, 4)
})

test('问题4：retarget 前后 piece 的 glyphIds 决定同一个 windowInstanceId（物理 key 稳定）', () => {
  const allocator = new WindowInstanceIdAllocator()
  const ctx = context('甲乙丙丁戊己', 783, [])
  const all = idsOf(ctx, 0, 6)
  const run = runGeometry(ctx, 0, 6, { x: 0, y: 0, width: 60, height: 20 })
  // prepare：簇2、簇3 被吞（clip 0..0）→ 两段 piece
  const prepared = insertRunStartState(run, {
    ...ctx,
    frozenWindows: [frozenWindow([all[2], all[3]], 0, 0, 0, 0, 783, null)],
  })
  const beforeKeys = prepared.map((p) => allocator.allocate(p.glyphIds))
  // retarget：可见性大幅变化（只剩第一段的一小部分可见）
  const retargeted = retargetRunStarts(run, prepared, {
    ...ctx,
    frozenWindows: [frozenWindow(all, 0, 5, 0, 0, 783, null)],
  }, 'insert', 0, { startClipLeft: 0, startClipRight: 60, startPositionX: 0, startPositionY: 0 })
  const afterKeys = retargeted.map((p) => allocator.allocate(p.glyphIds))
  // 同一个 piece 的 glyphIds 未变 → 分配器返回同一个 win-N → renderNodeKey 不变
  assert.deepEqual(afterKeys, beforeKeys)
})

test('问题4：insert 间隙 piece 在 retarget 后仍是零宽度，不填补空洞', () => {
  const ctx = context('甲乙丙丁戊', 784, [])
  const ids = idsOf(ctx, 0, 5)
  // 整段都被同一个旧窗口接管，但 clip 只露出簇2（20..30）→ 其余字形是被裁掉的新字
  const prepareCtx = {
    ...ctx,
    frozenWindows: [frozenWindow(ids, 20, 30, 0, 0, 784, null)],
  }
  const run = runGeometry(prepareCtx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  const prepared = insertRunStartState(run, prepareCtx)
  assert.equal(prepared.length, 3)
  assert.equal(prepared[0].firstIndex, 0)
  assert.equal(prepared[1].firstIndex, 2)
  assert.equal(prepared[2].firstIndex, 3)
  const retargeted = retargetRunStarts(run, prepared, prepareCtx, 'insert', 0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 })
  assertSamePartition(prepared, retargeted)
  // 尾间隙 piece 的起始位置仍在该 gap 自己的左边界（簇3 的 x=30），且零宽度
  assert.equal(retargeted[2].startClipLeft, 30)
  assert.equal(retargeted[2].startClipRight, 30)
  // 前间隙同理停在簇0 的左边界
  assert.equal(retargeted[0].startClipLeft, 0)
  assert.equal(retargeted[0].startClipRight, 0)
})

// ====== 问题2（高优先级）：固定 piece 分区下，交棒瞬间被冻结窗口局部裁切而断开成多段可见岛 ======
// 旧实现用 longestVisibleIsland 只保留最长一段，其余可见岛会被裁切层丢弃而永远无法归还
// 静态层——真实丢字。修复后 retargetRunStarts 必须返回该 piece 内所有不连续可见岛。

test('问题2：固定 piece 被冻结窗口局部裁切断开——retarget 返回所有不连续可见岛', () => {
  const ctx = context('甲乙丙丁戊', 790, [])
  const all = idsOf(ctx, 0, 5)
  const run = runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见 → 单个 piece 覆盖 0..4
  const prepared = insertRunStartState(run, ctx)
  assert.equal(prepared.length, 1, 'prepare 应为单个 piece')
  assert.equal(prepared[0].firstIndex, 0)
  assert.equal(prepared[0].lastIndex, 4)
  // retarget：一个冻结窗口把中段簇2（丙，x=20..30）彻底吞掉（clip 0..0），
  // 簇0/1/3/4 静态可见 → 该 piece 断成两段不连续可见岛 [0,1] 与 [3,4]。
  const retargeted = retargetRunStarts(
    run,
    prepared,
    { ...ctx, frozenWindows: [frozenWindow([all[2]], 0, 0, 0, 0, 790, null)] },
    'insert',
    0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  // 分区不变——物理节点身份（glyphIds）不被临时可见分组改变
  assertSamePartition(prepared, retargeted)
  // 必须返回两段可见岛，而不是只保留最长一段
  assert.ok(retargeted[0].intervals !== undefined, 'piece 应携带 intervals')
  assert.equal(retargeted[0].intervals.length, 2, '应有两个不连续可见岛')
  const ivs = retargeted[0].intervals
  assert.deepEqual([ivs[0].firstIndex, ivs[0].lastIndex], [0, 1])
  assert.deepEqual([ivs[1].firstIndex, ivs[1].lastIndex], [3, 4])
  // 被裁掉的簇2 不应出现在任何可见岛里（它此刻仍不可见，留在静态层）
  const covered = new Set()
  for (const iv of ivs) {
    for (let i = iv.firstIndex; i <= iv.lastIndex; i++) { covered.add(i) }
  }
  assert.ok(!covered.has(2), '被冻结窗口裁掉的簇2 不应进入可见岛')
  assert.ok(covered.has(0) && covered.has(1) && covered.has(3) && covered.has(4),
    '所有静态可见的簇都应被某个可见岛覆盖，不能丢字')
  // 每个岛携带自己在 run 自己正文里的 UTF-16 区间，供静态层精确扣除
  assert.equal(ivs[0].utf16Start, 0)
  assert.equal(ivs[0].utf16End, 2)
  assert.equal(ivs[1].utf16Start, 3)
  assert.equal(ivs[1].utf16End, 5)
})

test('问题2：多个可见岛时 legacy 单段 clip 取最长岛（等长取靠前的），不影响兼容读', () => {
  const ctx = context('甲乙丙丁戊', 791, [])
  const all = idsOf(ctx, 0, 5)
  const run = runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  const prepared = insertRunStartState(run, ctx)
  const retargeted = retargetRunStarts(
    run,
    prepared,
    { ...ctx, frozenWindows: [frozenWindow([all[2]], 0, 0, 0, 0, 791, null)] },
    'insert',
    0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  // 两段岛等长（各 2 簇）→ legacy 取靠前第一段 [0,1]，其 ownLeft/ownRight = 0..20
  assert.equal(retargeted[0].startClipLeft, 0)
  assert.equal(retargeted[0].startClipRight, 20)
})

test('问题2：retained 通道——同一固定 piece 的多段可见岛各自停在各自在屏位置', () => {
  const ctx = context('甲乙丙丁戊', 792, [])
  const all = idsOf(ctx, 0, 5)
  const run = runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  const prepared = retainedMoveStartState(run, ctx,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 })
  assert.equal(prepared.length, 1)
  const retargeted = retargetRunStarts(
    run,
    prepared,
    { ...ctx, frozenWindows: [frozenWindow([all[2]], 0, 0, 0, 0, 792, null)] },
    'retained',
    0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  assertSamePartition(prepared, retargeted)
  assert.ok(retargeted[0].intervals !== undefined)
  assert.equal(retargeted[0].intervals.length, 2)
  // 两个岛都应各自携带起始位置（retained：停在各自在屏位置）。本例 own 布局与在屏布局
  // 一致，故起始位置退化为 0——重点是每个岛都有独立且合法的起始状态，而非共享单段 clip。
  const ivs = retargeted[0].intervals
  assert.equal(ivs[0].firstIndex, 0)
  assert.equal(ivs[0].lastIndex, 1)
  assert.equal(ivs[1].firstIndex, 3)
  assert.equal(ivs[1].lastIndex, 4)
  assert.strictEqual(typeof ivs[0].startPositionX, 'number')
  assert.strictEqual(typeof ivs[1].startPositionX, 'number')
  assert.strictEqual(typeof ivs[0].startPositionY, 'number')
  assert.strictEqual(typeof ivs[1].startPositionY, 'number')
})


console.log(`\n✅ editor_commit_start: ${passed} tests passed`)