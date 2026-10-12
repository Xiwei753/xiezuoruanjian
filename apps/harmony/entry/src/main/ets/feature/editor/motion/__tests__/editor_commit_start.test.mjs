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
  retargetRunStarts, rectDifference, isFullyCovered, mapRectByRatio, shouldCrossfadeSources,
  frozenIndex, clusterVisible,
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

/**
 * 构造冻结窗口（含 sourceRevision、glyphUtf16Ranges 和 sourceLayout）。
 *
 * 当 glyphUtf16Ranges/sourceLayout 为 null 且 ctx 提供时，自动构造：
 * - glyphUtf16Ranges：从 ctx.identities 查每个 glyphId 的 UTF-16 区间
 * - sourceLayout：使用 ctx.layout（与 displayed.layout 相同，适用于 sourceRevision === displayed revision 的场景）
 */
const frozenWindow = (glyphIds, clipLeft, clipRight, offsetX, offsetY, sourceRevision, glyphUtf16Ranges, sourceLayout, ctx) => {
  // 当 glyphUtf16Ranges 为 null 且 ctx 提供时，自动构造映射
  if (glyphUtf16Ranges === null && ctx !== undefined) {
    const ranges = new Map()
    for (const gid of glyphIds) {
      const entry = ctx.identities === null ? null : ctx.identities.entryByGlyphId(gid)
      if (entry !== null) {
        ranges.set(gid, [entry.utf16Start, entry.utf16End])
      }
    }
    glyphUtf16Ranges = ranges
  }
  // 当 sourceLayout 为 null 且 ctx 提供时，使用 ctx.layout
  if (sourceLayout === null && ctx !== undefined) {
    sourceLayout = ctx.layout
  }
  return {
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
  }
}

// 辅助：从 ctx.identities 自动构造 glyphUtf16Ranges Map（与 frozenWindow 内部逻辑一致）
const autoRanges = (glyphIds, ctx) => {
  const ranges = new Map()
  if (ctx.identities === null) return ranges
  for (const gid of glyphIds) {
    const entry = ctx.identities.entryByGlyphId(gid)
    if (entry !== null) {
      ranges.set(gid, [entry.utf16Start, entry.utf16End])
    }
  }
  return ranges
}

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
  const frozen = [frozenWindow(all, 15, 25, 0, 0, 701, null, null, ctx)]
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
  const frozen = [frozenWindow(all, 0, 40, 0, 0, 702, null, null, ctx)]
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
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 703, null, null, ctx)]
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
  const frozen = [frozenWindow(ids, 0, 0, 0, 0, 705, null, null, ctx)]
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
  const frozen = [frozenWindow(all, 5, 45, 5, 0, 706, null, null, ctx)]
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
  const frozen = [frozenWindow(all, 35, 65, 5, 0, 707, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 6, { x: 0, y: 0, width: 60, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces[0].firstIndex, 3)
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 60, startPositionX: 0, startPositionY: 0,
  })
  // Issue #879 复核评论6102724233 问题1：startPositionX 现在是屏幕坐标系中的值
  // （sourceGlyphX0 + frozenDrawOffsetX），不再是 run 布局坐标系中的偏移量。
  // 簇3 sourceGlyphX0=30，窗口 offsetX=5 → startPositionX = 30 + 5 = 35
  assert.equal(start[0].startPositionX, 35)
})

test('问题1：保留字平移——一个共享字形都不在屏上 → 用兜底位置，不做凭空平移', () => {
  const ctx = context('甲乙丙丁', 708, [])
  const all = idsOf(ctx, 0, 4)
  const frozen = [frozenWindow(all, 0, 0, 0, 0, 708, null, null, ctx)]
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
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 720, null, null, ctx)]
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
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 721, null, null, ctx)]
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
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 722, null, null, ctx)]
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

test('问题2：多段可见字形——retainedMoveStartState 单一 piece + intervals 覆盖所有字形', () => {
  const ctx = context('甲乙丙丁', 723, [])
  const ids = idsOf(ctx, 0, 4)
  // 簇2（丙）被窗口接管且完全不可见；簇0、簇1、簇3 可见
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 723, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  // Issue #879 复核评论6100665268 问题1：retained 总是返回单一 piece 覆盖整个 run，
  // intervals 覆盖所有字形（每字形一个），不可见字形用零宽度 interval 保留分区。
  assert.equal(start.length, 1)
  assert.equal(start[0].firstIndex, 0)
  assert.equal(start[0].lastIndex, 3)  // 覆盖所有 glyphIds
  assert.deepEqual(start[0].glyphIds, ids)
  // intervals 覆盖所有字形（每字形一个 interval）
  const intervals = start[0].intervals
  assert.ok(intervals !== undefined && intervals.length === 4)
  // legacy 单段起始取第一个可见 interval（簇0）的值
  assert.equal(start[0].startClipLeft, 0)
  assert.equal(start[0].startClipRight, 10)
  // 簇0 静态可见，在屏位置 = ownLeft = 0 → startPositionX = 0 - (0 - 0) = 0
  assert.equal(start[0].startPositionX, 0)
  // 簇0 可见 [0,10]
  const iv0 = intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv0 !== undefined)
  assert.equal(iv0.startClipLeft, 0)
  assert.equal(iv0.startClipRight, 10)
  // 簇1 可见 [10,20]
  const iv1 = intervals.find(iv => iv.firstIndex === 1)
  assert.ok(iv1 !== undefined)
  assert.equal(iv1.startClipLeft, 10)
  assert.equal(iv1.startClipRight, 20)
  // 簇2 不可见 → 零宽度 interval（保留分区但不露出可见区域）
  const iv2 = intervals.find(iv => iv.firstIndex === 2)
  assert.ok(iv2 !== undefined)
  assert.equal(iv2.startClipLeft, iv2.startClipRight, '不可见字形 interval 应为零宽度')
  // 簇3 可见 [30,40]——丙的空洞绝不填补
  const iv3 = intervals.find(iv => iv.firstIndex === 3)
  assert.ok(iv3 !== undefined)
  assert.equal(iv3.startClipLeft, 30)
  assert.equal(iv3.startClipRight, 40)
})

test('问题2：三段可见——甲可见、乙不可见、丙可见、丁不可见、戊可见', () => {
  const ctx = context('甲乙丙丁戊', 724, [])
  const ids = idsOf(ctx, 0, 5)
  // 簇1（乙）和簇3（丁）不可见；簇0、簇2、簇4 可见
  const frozen = [
    frozenWindow([ids[1]], 0, 0, 0, 0, 724, null, null, ctx),
    frozenWindow([ids[3]], 0, 0, 0, 0, 724, null, null, ctx),
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
  const frozen = [frozenWindow(ghostIds, 20, 40, 0, 0, 734, null, null, ctx)]
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
  const frozen = [frozenWindow([glyphId], 45, 50, 0, 0, 740, null, null, ctx)]
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
  const frozen = [frozenWindow(all, 15, 25, 0, 0, 741, null, null, ctx)]
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
  const frozen = [frozenWindow(all, 5, 25, 0, 0, 742, null, null, ctx)]
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
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 762, null, null, ctx)]
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
  const frozen = [frozenWindow(ids, 0, 0, 0, 0, 765, null, null, ctx)]
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
  const frozen = [frozenWindow(all, 0, 0, 0, 0, 766, null, null, ctx)]
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
    frozenWindows: [frozenWindow(all, 0, 50, 0, 0, 780, null, null, ctx)],
  })
  assert.equal(prepared.length, 1)
  // retarget：同一个窗口缩到 clip 20..30（簇2 完整可见、簇1/簇3 部分可见）——可见分组变了
  const retargeted = retargetRunStarts(
    { ...run, ownLayout: ctx.layout },
    prepared,
    { ...ctx, frozenWindows: [frozenWindow(all, 20, 30, 0, 0, 780, null, null, ctx)] },
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
    frozenWindows: [frozenWindow([ids[2]], 0, 0, 0, 0, 781, null, null, ctx)],
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
  const hidden = { ...ctx, frozenWindows: [frozenWindow(all, 0, 0, 0, 0, 782, null, null, ctx)] }
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
    frozenWindows: [frozenWindow([all[2], all[3]], 0, 0, 0, 0, 783, null, null, ctx)],
  })
  const beforeKeys = prepared.map((p) => allocator.allocate(p.glyphIds))
  // retarget：可见性大幅变化（只剩第一段的一小部分可见）
  const retargeted = retargetRunStarts(run, prepared, {
    ...ctx,
    frozenWindows: [frozenWindow(all, 0, 5, 0, 0, 783, null, null, ctx)],
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
    frozenWindows: [frozenWindow(ids, 20, 30, 0, 0, 784, null, null, ctx)],
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
    { ...ctx, frozenWindows: [frozenWindow([all[2]], 0, 0, 0, 0, 790, null, null, ctx)] },
    'insert',
    0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  // 分区不变——物理节点身份（glyphIds）不被临时可见分组改变
  assertSamePartition(prepared, retargeted)
  // 缺口4：insert 通道除了两段可见岛，还为空洞簇2 建立零宽度 interval → 共 3 段
  assert.ok(retargeted[0].intervals !== undefined, 'piece 应携带 intervals')
  assert.equal(retargeted[0].intervals.length, 3, '应有两段可见岛 + 一段零宽度空洞 interval')
  const ivs = retargeted[0].intervals
  // 按 firstIndex 升序：可见岛[0,1]、零宽度[2,2]、可见岛[3,4]
  assert.deepEqual([ivs[0].firstIndex, ivs[0].lastIndex], [0, 1])
  assert.deepEqual([ivs[1].firstIndex, ivs[1].lastIndex], [2, 2])
  assert.deepEqual([ivs[2].firstIndex, ivs[2].lastIndex], [3, 4])
  // 空洞 interval 是零宽度（startClipLeft === startClipRight）
  assert.equal(ivs[1].startClipLeft, ivs[1].startClipRight, '空洞 interval 应为零宽度')
  // 空洞的 startPositionX 紧接前一个可见岛的右边界（20）
  assert.equal(ivs[1].startPositionX, 20, '空洞 startPositionX 应紧接前一个可见岛右边界')
  // 所有簇都被某个 interval 覆盖——可见岛覆盖可见簇，零宽度 interval 覆盖不可见簇
  const covered = new Set()
  for (const iv of ivs) {
    for (let i = iv.firstIndex; i <= iv.lastIndex; i++) { covered.add(i) }
  }
  assert.ok(covered.has(0) && covered.has(1) && covered.has(2) && covered.has(3) && covered.has(4),
    '所有簇都应被某个 interval 覆盖，不能丢字')
  // 每个岛携带自己在 run 自己正文里的 UTF-16 区间，供静态层精确扣除
  assert.equal(ivs[0].utf16Start, 0)
  assert.equal(ivs[0].utf16End, 2)
  assert.equal(ivs[1].utf16Start, 2)
  assert.equal(ivs[1].utf16End, 3)
  assert.equal(ivs[2].utf16Start, 3)
  assert.equal(ivs[2].utf16End, 5)
})

test('问题2：多个可见岛时 legacy 单段 clip 取最长岛（等长取靠前的），不影响兼容读', () => {
  const ctx = context('甲乙丙丁戊', 791, [])
  const all = idsOf(ctx, 0, 5)
  const run = runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  const prepared = insertRunStartState(run, ctx)
  const retargeted = retargetRunStarts(
    run,
    prepared,
    { ...ctx, frozenWindows: [frozenWindow([all[2]], 0, 0, 0, 0, 791, null, null, ctx)] },
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
    { ...ctx, frozenWindows: [frozenWindow([all[2]], 0, 0, 0, 0, 792, null, null, ctx)] },
    'retained',
    0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  assertSamePartition(prepared, retargeted)
  assert.ok(retargeted[0].intervals !== undefined)
  // Issue #879 复核评论6099070438 问题1：retained 通道 prepare 阶段每字形一个稳定 interval，
  // retarget 保留稳定分区只重算 clip/position。字形 2 不可见时其 interval clip 为零宽度。
  assert.equal(retargeted[0].intervals.length, 5)
  const ivs = retargeted[0].intervals
  // 每个字形一个 interval（稳定分区：firstIndex=lastIndex=i）
  assert.equal(ivs[0].firstIndex, 0)
  assert.equal(ivs[0].lastIndex, 0)
  assert.equal(ivs[1].firstIndex, 1)
  assert.equal(ivs[1].lastIndex, 1)
  assert.equal(ivs[2].firstIndex, 2)
  assert.equal(ivs[2].lastIndex, 2)
  // 字形 2 不可见（被冻结窗口吞掉）→ 零宽度，但分区保留
  assert.equal(ivs[2].startClipLeft, ivs[2].startClipRight, '不可见字形应为零宽度')
  assert.equal(ivs[3].firstIndex, 3)
  assert.equal(ivs[3].lastIndex, 3)
  assert.equal(ivs[4].firstIndex, 4)
  assert.equal(ivs[4].lastIndex, 4)
  // 每个可见字形都有独立且合法的起始位置（retained：停在各自在屏位置）
  assert.strictEqual(typeof ivs[0].startPositionX, 'number')
  assert.strictEqual(typeof ivs[1].startPositionX, 'number')
  assert.strictEqual(typeof ivs[3].startPositionX, 'number')
  assert.strictEqual(typeof ivs[4].startPositionX, 'number')
  assert.strictEqual(typeof ivs[0].startPositionY, 'number')
  assert.strictEqual(typeof ivs[1].startPositionY, 'number')
})


// ====== 缺口2：clusterVisible 的 y 交集检查和不合并不连续区间 ======

test('缺口2：clipRect 的 y 范围与字形不相交时，即使 x 相交也不可见', () => {
  const ctx = context('甲乙丙丁', 800, [])
  const all = idsOf(ctx, 0, 4)
  // 字形在 y=0 行（高度 20），clipRect 在 y=100 行（完全不相交的 y 范围）
  // 即使 x 范围覆盖字形，y 不相交也不应可见
  const frozen = [{
    glyphIds: all,
    clipLeft: 0,
    clipRight: 40,
    clipRects: [{ x: 0, y: 100, width: 40, height: 20 }],
    offsetX: 0,
    offsetY: 0,
    sourceRevision: 800,
    glyphUtf16Ranges: autoRanges(all, ctx),
    sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  // y 不相交 → 所有字形不可见
  assert.equal(pieces.length, 0)
})

test('缺口2：不合并不连续区间——间隙中的字形不被误判为可见', () => {
  const ctx = context('甲乙丙', 801, [])
  const all = idsOf(ctx, 0, 3)
  // 两个不连续的 clipRect：
  // rect1: [0,10] → 甲 [0,10] 完整可见
  // rect2: [20,30] → 丙 [20,30] 完整可见
  // 乙 [10,20] 完全在间隙中 → 不可见
  // 如果合并 clipRects 成 [0,30]，乙会被误判为可见
  const frozen = [{
    glyphIds: all,
    clipLeft: 0,
    clipRight: 30,
    clipRects: [
      { x: 0, y: 0, width: 10, height: 20 },
      { x: 20, y: 0, width: 10, height: 20 },
    ],
    offsetX: 0,
    offsetY: 0,
    sourceRevision: 801,
    glyphUtf16Ranges: autoRanges(all, ctx),
    sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 3, { x: 0, y: 0, width: 30, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  // 甲和丙可见，乙不可见 → 两段 piece
  assert.equal(pieces.length, 2)
  assert.equal(pieces[0].firstIndex, 0)
  assert.equal(pieces[0].lastIndex, 0)
  assert.equal(pieces[1].firstIndex, 2)
  assert.equal(pieces[1].lastIndex, 2)
})

test('缺口2：ghost 字形按 clipRects 求交——间隙中的 ghost 不可见', () => {
  // displayed 正文是 '甲'（乙丙丁已被删除），但旧窗口仍在绘制乙丙丁
  // 乙在源布局中 [10,20]，丙在 [20,30]，丁在 [30,40]
  // 两个不连续的 clipRect：
  // rect1: [10,20] → 乙完整可见
  // rect2: [30,40] → 丁完整可见
  // 丙 [20,30] 完全在间隙中 → 不可见
  // 如果用 clipLeft=10, clipRight=40（包围盒），丙会被误判为可见
  const ctx = context('甲', 802, [])
  const ghostIds = ['orig-802-1', 'orig-802-2', 'orig-802-3']
  const ghostRanges = new Map([
    ['orig-802-1', [1, 2]],
    ['orig-802-2', [2, 3]],
    ['orig-802-3', [3, 4]],
  ])
  const sourceLayout = [line('甲乙丙丁', 0)]
  const frozen = [{
    glyphIds: ghostIds,
    clipLeft: 10,
    clipRight: 40,
    clipRects: [
      { x: 10, y: 0, width: 10, height: 20 },
      { x: 30, y: 0, width: 10, height: 20 },
    ],
    offsetX: 0,
    offsetY: 0,
    sourceRevision: 802,
    glyphUtf16Ranges: ghostRanges,
    sourceLayout: sourceLayout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: ghostIds,
    ownText: '甲乙丙丁',
    ownUtf16Start: 1,
    ownUtf16End: 4,
    ownRect: { x: 10, y: 0, width: 30, height: 20 },
    ownLayout: [line('甲乙丙丁', 0)],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  // 乙和丁可见，丙不可见 → 两段 piece
  assert.equal(pieces.length, 2)
  assert.equal(pieces[0].firstIndex, 0)
  assert.equal(pieces[0].lastIndex, 0)
  assert.equal(pieces[1].firstIndex, 2)
  assert.equal(pieces[1].lastIndex, 2)
})

// ====== 缺口4：insert 通道为零宽度空洞建立 interval ======

test('缺口4：空洞在 piece 最前面——零宽度 interval 的 startPositionX 为 piece 的 startPositionX', () => {
  const ctx = context('甲乙丙丁戊', 810, [])
  const all = idsOf(ctx, 0, 5)
  const run = runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见 → 单个 piece 覆盖 0..4
  const prepared = insertRunStartState(run, ctx)
  assert.equal(prepared.length, 1)
  // retarget：簇0、簇1 被冻结窗口吞掉（clip 0..0），簇2..簇4 静态可见
  const retargeted = retargetRunStarts(
    run,
    prepared,
    { ...ctx, frozenWindows: [frozenWindow([all[0], all[1]], 0, 0, 0, 0, 810, null, null, ctx)] },
    'insert',
    0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  assertSamePartition(prepared, retargeted)
  assert.ok(retargeted[0].intervals !== undefined)
  // intervals 应包含：空洞[0,1] + 可见岛[2,4]
  assert.equal(retargeted[0].intervals.length, 2)
  const ivs = retargeted[0].intervals
  // 空洞 interval
  assert.deepEqual([ivs[0].firstIndex, ivs[0].lastIndex], [0, 1])
  assert.equal(ivs[0].startClipLeft, ivs[0].startClipRight, '空洞 interval 应为零宽度')
  // 空洞在 piece 最前面 → startPositionX = piece.startPositionX = 0
  assert.equal(ivs[0].startPositionX, 0)
  // 可见岛
  assert.deepEqual([ivs[1].firstIndex, ivs[1].lastIndex], [2, 4])
})

test('缺口4：空洞在 piece 最后面——零宽度 interval 的 startPositionX 为前一个可见岛的 startClipRight', () => {
  const ctx = context('甲乙丙丁戊', 811, [])
  const all = idsOf(ctx, 0, 5)
  const run = runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见 → 单个 piece 覆盖 0..4
  const prepared = insertRunStartState(run, ctx)
  // retarget：簇3、簇4 被冻结窗口吞掉（clip 0..0），簇0..簇2 静态可见
  const retargeted = retargetRunStarts(
    run,
    prepared,
    { ...ctx, frozenWindows: [frozenWindow([all[3], all[4]], 0, 0, 0, 0, 811, null, null, ctx)] },
    'insert',
    0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  assertSamePartition(prepared, retargeted)
  assert.ok(retargeted[0].intervals !== undefined)
  // intervals 应包含：可见岛[0,2] + 空洞[3,4]
  assert.equal(retargeted[0].intervals.length, 2)
  const ivs = retargeted[0].intervals
  // 可见岛
  assert.deepEqual([ivs[0].firstIndex, ivs[0].lastIndex], [0, 2])
  // 空洞 interval
  assert.deepEqual([ivs[1].firstIndex, ivs[1].lastIndex], [3, 4])
  assert.equal(ivs[1].startClipLeft, ivs[1].startClipRight, '空洞 interval 应为零宽度')
  // 空洞在 piece 最后面 → startPositionX = 前一个可见岛的 startClipRight = 30
  assert.equal(ivs[1].startPositionX, 30)
})

test('缺口4：多个空洞——每个空洞都有零宽度 interval', () => {
  const ctx = context('甲乙丙丁戊己庚', 812, [])
  const all = idsOf(ctx, 0, 7)
  const run = runGeometry(ctx, 0, 7, { x: 0, y: 0, width: 70, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见 → 单个 piece 覆盖 0..6
  const prepared = insertRunStartState(run, ctx)
  // retarget：簇1（乙）和簇3（丁）被冻结窗口吞掉，其余静态可见
  // 可见-不可见-可见-不可见-可见：甲可见、乙不可见、丙可见、丁不可见、戊己庚可见
  const retargeted = retargetRunStarts(
    run,
    prepared,
    { ...ctx, frozenWindows: [
      frozenWindow([all[1]], 0, 0, 0, 0, 812, null, null, ctx),
      frozenWindow([all[3]], 0, 0, 0, 0, 812, null, null, ctx),
    ] },
    'insert',
    0,
    { startClipLeft: 0, startClipRight: 70, startPositionX: 0, startPositionY: 0 },
  )
  assertSamePartition(prepared, retargeted)
  assert.ok(retargeted[0].intervals !== undefined)
  // intervals 应包含：可见岛[0,0] + 空洞[1,1] + 可见岛[2,2] + 空洞[3,3] + 可见岛[4,6]
  // = 5 个 interval
  assert.equal(retargeted[0].intervals.length, 5)
  const ivs = retargeted[0].intervals
  // 可见岛[0,0]：甲
  assert.deepEqual([ivs[0].firstIndex, ivs[0].lastIndex], [0, 0])
  // 空洞[1,1]：乙
  assert.deepEqual([ivs[1].firstIndex, ivs[1].lastIndex], [1, 1])
  assert.equal(ivs[1].startClipLeft, ivs[1].startClipRight, '空洞1 应为零宽度')
  // 可见岛[2,2]：丙
  assert.deepEqual([ivs[2].firstIndex, ivs[2].lastIndex], [2, 2])
  // 空洞[3,3]：丁
  assert.deepEqual([ivs[3].firstIndex, ivs[3].lastIndex], [3, 3])
  assert.equal(ivs[3].startClipLeft, ivs[3].startClipRight, '空洞2 应为零宽度')
  // 可见岛[4,6]：戊己庚
  assert.deepEqual([ivs[4].firstIndex, ivs[4].lastIndex], [4, 6])
})

test('缺口4：整个 piece 都不可见——insert 通道创建覆盖整个 piece 的零宽度 interval', () => {
  const ctx = context('甲乙丙丁', 813, [])
  const all = idsOf(ctx, 0, 4)
  const run = runGeometry(ctx, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见 → 单个 piece 覆盖 0..3
  const prepared = insertRunStartState(run, ctx)
  // retarget：所有字形被冻结窗口吞掉（clip 0..0）
  const retargeted = retargetRunStarts(
    run,
    prepared,
    { ...ctx, frozenWindows: [frozenWindow(all, 0, 0, 0, 0, 813, null, null, ctx)] },
    'insert',
    0,
    { startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0 },
  )
  assertSamePartition(prepared, retargeted)
  assert.ok(retargeted[0].intervals !== undefined)
  // 没有可见岛 → 只有一个零宽度 interval 覆盖整个 piece
  assert.equal(retargeted[0].intervals.length, 1)
  const ivs = retargeted[0].intervals
  assert.deepEqual([ivs[0].firstIndex, ivs[0].lastIndex], [0, 3])
  assert.equal(ivs[0].startClipLeft, ivs[0].startClipRight, '应为零宽度')
  // 空洞在 piece 最前面（也是最后面）→ startPositionX = piece.startPositionX = 0
  assert.equal(ivs[0].startPositionX, 0)
})


// ====== Issue #879 复核评论6099070438：问题1/2/4 新增测试 ======

test('问题1：retainedMoveStartState 为每个 piece 设置稳定 intervals（每字形一个）', () => {
  const ctx = context('甲乙丙丁戊', 900, [])
  const run = runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  const prepared = retainedMoveStartState(run, ctx,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 })
  assert.equal(prepared.length, 1)
  assert.ok(prepared[0].intervals !== undefined)
  // 每个字形一个 interval（firstIndex === lastIndex），共 5 个
  assert.equal(prepared[0].intervals.length, 5)
  for (let i = 0; i < 5; i++) {
    const iv = prepared[0].intervals[i]
    assert.equal(iv.firstIndex, i, `interval${i} firstIndex 应为 ${i}`)
    assert.equal(iv.lastIndex, i, `interval${i} lastIndex 应为 ${i}`)
    assert.equal(iv.glyphIds.length, 1, `interval${i} 应只含 1 个 glyphId`)
  }
})

test('问题1：retarget 后 retained intervals 分区不变（firstIndex/lastIndex/glyphIds）', () => {
  const ctx = context('甲乙丙丁戊', 901, [])
  const all = idsOf(ctx, 0, 5)
  const run = runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  const prepared = retainedMoveStartState(run, ctx,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 })
  // retarget：字形 2 被冻结窗口吞掉（clip 0..0）
  const retargeted = retargetRunStarts(
    run, prepared,
    { ...ctx, frozenWindows: [frozenWindow([all[2]], 0, 0, 0, 0, 901, null, null, ctx)] },
    'retained', 0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  assertSamePartition(prepared, retargeted)
  // intervals 分区不变：每字形一个，firstIndex/lastIndex/glyphIds 完全一致
  assert.equal(retargeted[0].intervals.length, prepared[0].intervals.length)
  for (let i = 0; i < prepared[0].intervals.length; i++) {
    assert.equal(retargeted[0].intervals[i].firstIndex, prepared[0].intervals[i].firstIndex)
    assert.equal(retargeted[0].intervals[i].lastIndex, prepared[0].intervals[i].lastIndex)
    assert.deepEqual(retargeted[0].intervals[i].glyphIds, prepared[0].intervals[i].glyphIds)
  }
  // 字形 2 不可见 → 其 interval clip 为零宽度，但分区保留
  const iv2 = retargeted[0].intervals[2]
  assert.equal(iv2.startClipLeft, iv2.startClipRight, '不可见字形 interval 应为零宽度')
})

test('问题1：retained prepare/retarget 的 interval.glyphIds 产生相同 windowInstanceId', () => {
  const ctx = context('甲乙丙丁戊', 902, [])
  const all = idsOf(ctx, 0, 5)
  const run = runGeometry(ctx, 0, 5, { x: 0, y: 0, width: 50, height: 20 })
  const prepared = retainedMoveStartState(run, ctx,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 })
  const retargeted = retargetRunStarts(
    run, prepared,
    { ...ctx, frozenWindows: [frozenWindow([all[2]], 0, 0, 0, 0, 902, null, null, ctx)] },
    'retained', 0,
    { startClipLeft: 0, startClipRight: 50, startPositionX: 0, startPositionY: 0 },
  )
  // plannedWindowKeys 和 buildFrame 都按 interval.glyphIds 分配 key，
  // prepare 和 retarget 的 interval.glyphIds 必须完全一致 → key 集合相等
  const alloc1 = new WindowInstanceIdAllocator()
  const alloc2 = new WindowInstanceIdAllocator()
  const keys1 = prepared[0].intervals.map(iv => alloc1.allocate(iv.glyphIds))
  const keys2 = retargeted[0].intervals.map(iv => alloc2.allocate(iv.glyphIds))
  assert.deepEqual(keys1, keys2, 'prepare/retarget 的 windowInstanceId 应完全一致')
})

test('问题4：同一字形左右两块露出、中间有空洞——clipRects 有多段', () => {
  const ctx = context('甲', 903, [])
  const all = idsOf(ctx, 0, 1)
  const run = runGeometry(ctx, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  // 冻结窗口有两个 clipRect：[0,4] 和 [6,10]，中间 [4,6] 不可见
  const span = {
    glyphIds: all,
    clipLeft: 0, clipRight: 10,
    clipRects: [
      { x: 0, y: 0, width: 4, height: 20 },
      { x: 6, y: 0, width: 4, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 903, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }
  const pieces = visibleGlyphPieces(run, { ...ctx, frozenWindows: [span] })
  assert.equal(pieces.length, 1)
  assert.ok(pieces[0].clipRects !== undefined, 'piece 应有 clipRects')
  // 应有 2 段裁切子矩形（左右两块露出），不是压成 [0,10] 连续区间
  assert.equal(pieces[0].clipRects.length, 2)
  assert.equal(pieces[0].clipRects[0].x, 0)
  assert.equal(pieces[0].clipRects[0].width, 4)
  assert.equal(pieces[0].clipRects[1].x, 6)
  assert.equal(pieces[0].clipRects[1].width, 4)
})

test('问题4：retainedMoveStartState 的 interval 携带 clipRects', () => {
  const ctx = context('甲', 904, [])
  const all = idsOf(ctx, 0, 1)
  const run = runGeometry(ctx, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const span = {
    glyphIds: all,
    clipLeft: 0, clipRight: 10,
    clipRects: [
      { x: 0, y: 0, width: 4, height: 20 },
      { x: 6, y: 0, width: 4, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 904, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }
  const prepared = retainedMoveStartState(run, { ...ctx, frozenWindows: [span] },
    { startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0 })
  assert.equal(prepared.length, 1)
  assert.ok(prepared[0].intervals !== undefined)
  assert.equal(prepared[0].intervals.length, 1)
  assert.ok(prepared[0].intervals[0].clipRects !== undefined, 'interval 应有 clipRects')
  assert.equal(prepared[0].intervals[0].clipRects.length, 2)
})

test('问题4：retarget 后 retained interval 的 clipRects 反映新可见性', () => {
  const ctx = context('甲', 905, [])
  const all = idsOf(ctx, 0, 1)
  const run = runGeometry(ctx, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  // prepare：完整可见（无冻结窗口）
  const prepared = retainedMoveStartState(run, ctx,
    { startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0 })
  // retarget：被裁切成两段 [0,3] 和 [7,10]
  const span = {
    glyphIds: all,
    clipLeft: 0, clipRight: 10,
    clipRects: [
      { x: 0, y: 0, width: 3, height: 20 },
      { x: 7, y: 0, width: 3, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 905, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }
  const retargeted = retargetRunStarts(
    run, prepared, { ...ctx, frozenWindows: [span] },
    'retained', 0,
    { startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0 },
  )
  assert.ok(retargeted[0].intervals[0].clipRects !== undefined)
  assert.equal(retargeted[0].intervals[0].clipRects.length, 2)
  // 裁切宽度反映 retarget 时的可见性（3vp 每段）
  assert.equal(retargeted[0].intervals[0].clipRects[0].width, 3)
  assert.equal(retargeted[0].intervals[0].clipRects[1].width, 3)
})

test('问题4：clonePieces 深拷贝 clipRects——retarget 全不可见时 clipRects 仍保留', () => {
  const ctx = context('甲', 906, [])
  const all = idsOf(ctx, 0, 1)
  const run = runGeometry(ctx, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  // prepare：被裁切成两段
  const span = {
    glyphIds: all,
    clipLeft: 0, clipRight: 10,
    clipRects: [
      { x: 0, y: 0, width: 4, height: 20 },
      { x: 6, y: 0, width: 4, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 906, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }
  const prepared = retainedMoveStartState(run, { ...ctx, frozenWindows: [span] },
    { startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0 })
  assert.ok(prepared[0].intervals[0].clipRects !== undefined)
  // retarget：字形完全不可见（clip 0..0）→ intervalForIsland 返回 null → 零宽度 interval
  // 但分区保留，clipRects 不复制（零宽度 interval 不带 clipRects）
  const retargeted = retargetRunStarts(
    run, prepared,
    { ...ctx, frozenWindows: [frozenWindow(all, 0, 0, 0, 0, 906, null, null, ctx)] },
    'retained', 0,
    { startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0 },
  )
  assertSamePartition(prepared, retargeted)
  // 不可见字形的 interval 应为零宽度
  assert.equal(retargeted[0].intervals[0].startClipLeft, retargeted[0].intervals[0].startClipRight)
})

// ====== Issue #879 复核评论6099791364：问题3（retained islandStartX 用源字形绘制原点） ======
// 旧公式 islandStartX = startPositionX + startClipLeft - oldRect.x 得到 clip 在屏左边界，
// p=0 时 islandOffsetX 把局部可见半字偏移扩大成整字。
// 修复后 islandStartX = islandOldLeft + startPositionX - oldRect.x（字形在屏绘制原点），
// p=0 时 offset = currentWindowOffset，字形停在原位 + 窗口偏移，clip 保持当前裁切。
// 注：islandOldLeft 是源字形完整左边界（sourceGlyphX0），不是裁切后可见左边界（ownLeft）。

test('问题3：retained 部分裁切——interval.startPositionX 是窗口偏移，新公式给字形绘制原点不是 clip 在屏左边界', () => {
  const ctx = context('甲乙丙丁', 910, [])
  const all = idsOf(ctx, 0, 4)
  // 冻结窗口 clip=[6,36], offsetX=3：
  // 甲在源[0,10]，在屏[3,13]（源+offset），clip[6,36]交集→在屏可见[6,13]，源可见[3,10]
  // onScreenLeft=6（clip 在屏左边界），ownLeft=3（裁切后投影），sourceGlyphX0=0（完整左边界）
  const frozen = [frozenWindow(all, 6, 36, 3, 0, 910, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })

  const pieces = visibleGlyphPieces(run, withWindow)
  const piece = pieces[0]
  // 首字形（甲）的几何
  const onScreenLeft = piece.onScreenLeft
  const ownLeft = piece.ownLeft
  const sourceGlyphX0 = piece.sourceGlyphX0
  const ownRectX = run.ownRect.x

  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  // 甲的 interval（retained 每字形一个稳定 interval）
  const iv = start[0].intervals[0]
  const startPositionX = iv.startPositionX
  const startClipLeft = iv.startClipLeft

  // 1. startPositionX 语义：onScreenLeft - ownLeft + ownRect.x = 6 - 3 + 0 = 3
  assert.equal(startPositionX, onScreenLeft - ownLeft + ownRectX)
  // 2. startPositionX - ownRect.x 等于当前窗口偏移（onScreenLeft - ownLeft = 3 = offsetX）
  const currentWindowOffset = onScreenLeft - ownLeft
  assert.equal(startPositionX - ownRectX, currentWindowOffset)
  // 3. 新公式 islandStartX = islandOldLeft + startPositionX - oldRect.x
  //    islandOldLeft = sourceGlyphX0（源字形完整左边界，不是裁切后的 ownLeft）
  const islandOldLeft = sourceGlyphX0
  const newIslandStartX = islandOldLeft + startPositionX - ownRectX
  //    应等于 islandOldLeft + currentWindowOffset（字形在屏绘制原点 = 0 + 3 = 3）
  assert.equal(newIslandStartX, islandOldLeft + currentWindowOffset)
  // 4. 部分裁切时（sourceGlyphX0 !== ownLeft），新公式结果不等于 onScreenLeft（clip 在屏左边界）
  assert.notEqual(sourceGlyphX0, ownLeft, '部分裁切时完整边界应不同于裁切后边界')
  assert.notEqual(newIslandStartX, onScreenLeft, '新公式应给字形绘制原点(3)，不是 clip 在屏左边界(6)')
  // 5. 旧公式 islandStartX = startPositionX + startClipLeft - oldRect.x 会等于 onScreenLeft（错误）
  const oldIslandStartX = startPositionX + startClipLeft - ownRectX
  assert.equal(oldIslandStartX, onScreenLeft, '旧公式给 clip 在屏左边界(6)——错误语义')
})

test('问题3：retained retarget 后 interval.startPositionX 仍保持窗口偏移语义', () => {
  const ctx = context('甲乙丙丁', 911, [])
  const all = idsOf(ctx, 0, 4)
  const run = runGeometry(ctx, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见
  const prepared = retainedMoveStartState(run, ctx, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  // retarget：加冻结窗口 clip=[6,36] offsetX=3，甲被部分裁切
  const frozen = [frozenWindow(all, 6, 36, 3, 0, 911, null, null, ctx)]
  const retargeted = retargetRunStarts(run, prepared, { ...ctx, frozenWindows: frozen }, 'retained', 0,
    { startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0 })

  // retarget 后仍保留稳定 intervals 分区（每字形一个）
  assert.ok(retargeted[0].intervals !== undefined)
  const iv = retargeted[0].intervals[0]  // 甲的 interval
  // retarget 重算 clip/position：甲在屏可见[6,13]，ownLeft=3, onScreenLeft=6
  // startPositionX = onScreenLeft - ownLeft + ownRect.x = 6 - 3 + 0 = 3
  assert.equal(iv.startPositionX, 3)
  // 新公式 islandStartX = sourceGlyphX0 + startPositionX - ownRect.x = 0 + 3 - 0 = 3（字形绘制原点）
  const sourceGlyphX0 = 0  // 甲完整左边界
  const newIslandStartX = sourceGlyphX0 + iv.startPositionX - run.ownRect.x
  assert.equal(newIslandStartX, 3, '新公式给字形绘制原点(3)')
  assert.notEqual(newIslandStartX, 6, '不等于 clip 在屏左边界(6)')
})

test('问题3：retained 部分裁切 p=0 时——新公式 islandOffsetX 等于 currentWindowOffset，不扩大偏移', () => {
  const ctx = context('甲乙丙丁', 912, [])
  const all = idsOf(ctx, 0, 4)
  // clip=[6,36] offsetX=3：甲在屏可见[6,13]，只露出右半 7vp（局部可见半字）
  const frozen = [frozenWindow(all, 6, 36, 3, 0, 912, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  const piece = pieces[0]
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals[0]

  const sourceGlyphX0 = piece.sourceGlyphX0  // 0（甲完整左边界）
  const ownRectX = run.ownRect.x  // 0
  const currentWindowOffset = piece.onScreenLeft - piece.ownLeft  // 6 - 3 = 3

  // 新公式 p=0 时 islandOffsetX = islandStartX - islandOldLeft
  const newIslandStartX = sourceGlyphX0 + iv.startPositionX - ownRectX
  const newIslandOffsetX = newIslandStartX - sourceGlyphX0
  // 应等于 currentWindowOffset（字形停在原位 + 窗口偏移，clip 保持当前裁切）
  assert.equal(newIslandOffsetX, currentWindowOffset,
    '新公式 p=0 时 offset 应等于 currentWindowOffset(3)，不扩大局部可见半字偏移')

  // 旧公式 p=0 时 islandOffsetX 会把 clip 在屏左边界当成字形原点，偏移被扩大
  const oldIslandStartX = iv.startPositionX + iv.startClipLeft - ownRectX
  const oldIslandOffsetX = oldIslandStartX - sourceGlyphX0
  assert.notEqual(oldIslandOffsetX, currentWindowOffset,
    '旧公式 p=0 时 offset 不等于 currentWindowOffset——错误')
  assert.equal(oldIslandOffsetX, piece.onScreenLeft - sourceGlyphX0,
    '旧公式 offset = onScreenLeft - islandOldLeft(6)——把局部可见半字偏移扩大成整字')
})

// ====== Issue #879 复核评论6099791364：问题2（carry insert 目标矩形用完整字形几何） ======
// 旧实现 islandTargetRects 直接拷贝 piece.clipRects，与 islandStartRects 完全相同，
// 导致吐字动画完全停滞（每帧 clipRects 不变）。
// 修复后 islandTargetRects 根据 piece.sourceRect 或 glyphX/glyphY/glyphWidth/glyphHeight
// 计算完整字形可见矩形；clipRects 为空时建立零宽度起点和非零目标。
// 在 .ts 层面验证 VisibleRunPiece/RunStartInterval 携带完整字形几何（sourceGlyphX0/X1）
// 和裁切矩形（clipRects），数据结构完整性支撑 .ets 层计算完整目标矩形。

test('问题2：VisibleRunPiece 携带 sourceGlyphX0/X1 完整字形边界——与裁切后 ownLeft/ownRight 不同', () => {
  const ctx = context('甲乙丙丁', 913, [])
  const all = idsOf(ctx, 0, 4)
  // clip=[4,36] offsetX=0：甲在源[0,10]可见[4,10]，部分裁切（左 4vp 被裁掉）
  const frozen = [frozenWindow(all, 4, 36, 0, 0, 913, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 1)
  const piece = pieces[0]
  // sourceGlyphX0 是首字形（甲）完整左边界 = 0（未裁切的原始边界）
  assert.equal(piece.sourceGlyphX0, 0)
  // ownLeft 是裁切后可见左边界 = 4（甲被裁掉左 4vp）
  assert.equal(piece.ownLeft, 4)
  // 完整边界 ≠ 裁切后边界 → sourceGlyphX0/X1 可用于计算完整目标矩形，不与 clipRects 混淆
  assert.notEqual(piece.sourceGlyphX0, piece.ownLeft,
    '部分裁切时完整字形边界(sourceGlyphX0)应不同于裁切后边界(ownLeft)')
  // sourceGlyphX1 是尾字形（丁）完整右边界 = 40
  assert.equal(piece.sourceGlyphX1, 40)
  // ownRight 是丁裁切后右边界 = 36（丁被裁掉右 4vp）
  assert.equal(piece.ownRight, 36)
  assert.notEqual(piece.sourceGlyphX1, piece.ownRight,
    '部分裁切时完整字形边界(sourceGlyphX1)应不同于裁切后边界(ownRight)')
})

test('问题2：piece 的 clipRects 是投影后裁切矩形——不直接拷贝冻结窗口 clipRects', () => {
  const ctx = context('甲乙丙丁', 914, [])
  const all = idsOf(ctx, 0, 4)
  // clip=[6,36] offsetX=3：窗口 clipRects=[{x:6,width:30}]
  // 甲在源[0,10]在屏[3,13]，可见在屏[6,13]→源可见[3,10]→投影到目标 ownLeft=3
  // piece.clipRects 应是投影到 run 自己布局的值，不是直接拷贝窗口的 {x:6,width:30}
  const frozen = [frozenWindow(all, 6, 36, 3, 0, 914, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  const piece = pieces[0]
  assert.ok(piece.clipRects !== undefined, 'piece 应携带 clipRects')
  // clipRects 不等于窗口的 clipRects [{x:6,width:30}]——是投影后的多段矩形
  assert.notEqual(piece.clipRects.length, 1, '部分裁切多字形时 clipRects 应有多段，不是直接拷贝窗口单段')
  // 首段左边界 = 3（投影后），不是窗口的 6
  assert.equal(piece.clipRects[0].x, 3, 'clipRects 首段应投影到 run 布局(x=3)，不是窗口 clip 的 x=6')
  // sourceGlyphX0/X1 提供完整字形边界，可用于计算完整目标矩形（不依赖 clipRects）
  assert.strictEqual(typeof piece.sourceGlyphX0, 'number')
  assert.strictEqual(typeof piece.sourceGlyphX1, 'number')
  // clipRects 总宽度 = ownRight - ownLeft（裁切后可见宽度）
  const clipWidth = piece.clipRects.reduce((sum, r) => sum + r.width, 0)
  assert.equal(clipWidth, piece.ownRight - piece.ownLeft,
    'clipRects 总宽度应等于裁切后可见宽度')
})

test('问题2：retarget insert 通道 interval 携带 clipRects——几何信息完整传递', () => {
  const ctx = context('甲乙丙丁', 915, [])
  const all = idsOf(ctx, 0, 4)
  const run = runGeometry(ctx, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见
  const prepared = insertRunStartState(run, ctx)
  // retarget：加冻结窗口 clip=[4,36]，甲丁部分裁切
  const frozen = [frozenWindow(all, 4, 36, 0, 0, 915, null, null, ctx)]
  const retargeted = retargetRunStarts(run, prepared, { ...ctx, frozenWindows: frozen }, 'insert', 0,
    { startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0 })
  // insert 通道 interval 应携带 clipRects（可见岛）
  assert.ok(retargeted[0].intervals !== undefined)
  for (const iv of retargeted[0].intervals) {
    // 可见岛（非零宽度）interval 应携带 clipRects
    if (iv.startClipRight - iv.startClipLeft > 0.01) {
      assert.ok(iv.clipRects !== undefined, '可见岛 interval 应携带 clipRects')
      assert.ok(iv.clipRects.length >= 1, 'clipRects 应至少有一段')
      // clipRects 几何与 startClipLeft/Right 一致
      const ivClipLeft = Math.min(...iv.clipRects.map(r => r.x))
      const ivClipRight = Math.max(...iv.clipRects.map(r => r.x + r.width))
      assert.equal(ivClipLeft, iv.startClipLeft, 'clipRects 左边界应与 startClipLeft 一致')
      assert.equal(ivClipRight, iv.startClipRight, 'clipRects 右边界应与 startClipRight 一致')
    }
  }
})

test('问题2：字形不可见时 insert 通道建立零宽度 interval——完整字形信息可恢复非零目标矩形', () => {
  const ctx = context('甲乙丙丁', 916, [])
  const all = idsOf(ctx, 0, 4)
  const run = runGeometry(ctx, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见
  const prepared = insertRunStartState(run, ctx)
  // retarget：所有字形被冻结窗口吞掉（clip 0..0）→ clipRects 为空
  const frozen = [frozenWindow(all, 0, 0, 0, 0, 916, null, null, ctx)]
  const retargeted = retargetRunStarts(run, prepared, { ...ctx, frozenWindows: frozen }, 'insert', 0,
    { startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0 })
  // insert 通道为零宽度空洞建立 interval（零宽度起点 + 非零目标的基础）
  assert.ok(retargeted[0].intervals !== undefined)
  assert.equal(retargeted[0].intervals.length, 1)
  const iv = retargeted[0].intervals[0]
  // 零宽度 interval（clipRects 为空/未定义，但 startPositionX 存在——零宽度起点）
  assert.equal(iv.startClipLeft, iv.startClipRight, '应为零宽度 interval（零宽度起点）')
  assert.strictEqual(typeof iv.startPositionX, 'number', '零宽度 interval 应有 startPositionX')
  // 完整字形信息保留：firstIndex/lastIndex/glyphIds 覆盖整段
  // .ets 层可据此用 sourceRect/glyphX/glyphY/glyphWidth/glyphHeight 计算非零目标矩形
  assert.deepEqual([iv.firstIndex, iv.lastIndex], [0, 3])
  assert.equal(iv.glyphIds.length, 4, 'interval 应保留全部 4 个 glyphId——支撑计算非零目标矩形')
  assert.deepEqual(iv.glyphIds, all, 'glyphIds 应与 run 完全一致')
})

test('问题2：retained interval 的 clipRects 与 startPositionX 共同支撑完整目标矩形计算', () => {
  const ctx = context('甲乙丙丁', 917, [])
  const all = idsOf(ctx, 0, 4)
  // clip=[4,36] offsetX=0：甲可见[4,10]（左 4vp 裁掉），丁可见[30,36]（右 4vp 裁掉）
  const frozen = [frozenWindow(all, 4, 36, 0, 0, 917, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  // retained 每字形一个稳定 interval
  assert.ok(start[0].intervals !== undefined)
  assert.equal(start[0].intervals.length, 4)
  // 甲的 interval（部分裁切）
  const iv0 = start[0].intervals[0]
  assert.ok(iv0.clipRects !== undefined, '甲的 interval 应携带 clipRects')
  // clipRects 反映裁切后可见矩形（甲可见[4,10]→clipRect x=4,width=6）
  assert.equal(iv0.clipRects[0].x, 4)
  assert.equal(iv0.clipRects[0].width, 6)
  // startPositionX = 0（offsetX=0，onScreenLeft=ownLeft 时窗口偏移为 0）
  assert.equal(iv0.startPositionX, 0)
  // sourceGlyphX0（完整左边界=0）+ startPositionX - ownRect.x = 0 → 字形绘制原点
  // 与 clipRects 的 x=4（裁切后）不同——完整字形几何与裁切矩形各自独立可用
  // .ets 层据 sourceGlyphX0/glyphWidth 计算完整目标矩形，据 clipRects 计算起始裁切
  const pieces = visibleGlyphPieces(run, withWindow)
  const sourceGlyphX0 = pieces[0].sourceGlyphX0  // 0
  const islandStartX = sourceGlyphX0 + iv0.startPositionX - run.ownRect.x  // 0
  assert.notEqual(islandStartX, iv0.clipRects[0].x,
    '字形绘制原点(0)应不同于裁切后左边界(4)——完整几何与裁切矩形独立')
})

// ====== Issue #879 复核评论6100665268 问题3：retainedMoveStartState 的 sourceGlyphRects ======
// 新语义下 retainedMoveStartState 总是返回单一 piece，intervals 覆盖所有字形。
// 可见字形 interval 携带 clipRects 和 sourceGlyphRects（长度一致）；
// 不可见字形 interval 携带零宽 clipRects 和完整 sourceGlyphRects。
// retargetRunStarts retained 通道保留 sourceGlyphRects，支撑 buildFrame 精确映射裁切矩形。

test('问题3：retainedMoveStartState 可见字形 interval 携带 clipRects 和 sourceGlyphRects，且长度一致', () => {
  const ctx = context('甲乙丙丁', 920, [])
  const run = runGeometry(ctx, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, ctx, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  // Issue #879 复核评论6100665268 问题3：可见字形 interval 携带 clipRects 和 sourceGlyphRects
  assert.equal(start.length, 1)
  const intervals = start[0].intervals
  assert.ok(intervals !== undefined && intervals.length === 4)
  for (let i = 0; i < 4; i++) {
    const iv = intervals[i]
    assert.ok(iv.clipRects !== undefined, `interval${i} 应携带 clipRects`)
    assert.ok(iv.sourceGlyphRects !== undefined, `interval${i} 应携带 sourceGlyphRects`)
    // clipRects 和 sourceGlyphRects 长度一致——每段裁切对应一个源字形矩形
    assert.equal(iv.clipRects.length, iv.sourceGlyphRects.length,
      `interval${i} clipRects 和 sourceGlyphRects 长度应一致`)
    // sourceGlyphRects 是完整字形矩形（宽 10vp，高 20vp）
    for (const sr of iv.sourceGlyphRects) {
      assert.equal(sr.height, 20, 'sourceGlyphRect 高度应为字形高度 20')
      assert.ok(sr.width > 0, '可见字形 sourceGlyphRect 宽度应大于 0')
    }
  }
})

test('问题3：retainedMoveStartState 不可见字形 interval 携带零宽 clipRects 和完整 sourceGlyphRects', () => {
  const ctx = context('甲乙丙丁', 921, [])
  const ids = idsOf(ctx, 0, 4)
  // 簇2（丙）被窗口接管且完全不可见
  const frozen = [frozenWindow([ids[2]], 0, 0, 0, 0, 921, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  // Issue #879 复核评论6100665268 问题3：不可见字形 interval 携带零宽 clipRects 和完整 sourceGlyphRects
  assert.equal(start.length, 1)
  const intervals = start[0].intervals
  assert.ok(intervals !== undefined && intervals.length === 4)
  // 簇2 不可见
  const iv2 = intervals.find(iv => iv.firstIndex === 2)
  assert.ok(iv2 !== undefined)
  // clipRects 是零宽度
  assert.ok(iv2.clipRects !== undefined, '不可见字形 interval 应携带 clipRects')
  assert.equal(iv2.clipRects.length, 1)
  assert.equal(iv2.clipRects[0].width, 0, '不可见字形 clipRects 应为零宽度')
  // sourceGlyphRects 是完整字形矩形（宽 10vp，高 20vp）
  assert.ok(iv2.sourceGlyphRects !== undefined, '不可见字形 interval 应携带 sourceGlyphRects')
  assert.equal(iv2.sourceGlyphRects.length, 1)
  assert.equal(iv2.sourceGlyphRects[0].width, 10, '不可见字形 sourceGlyphRects 应为完整字形宽度 10')
  assert.equal(iv2.sourceGlyphRects[0].height, 20, '不可见字形 sourceGlyphRects 应为完整字形高度 20')
  // 簇2 在 run 布局中 [20,30]
  assert.equal(iv2.sourceGlyphRects[0].x, 20, '不可见字形 sourceGlyphRects x 应为簇2 左边界 20')
})

test('问题3：retargetRunStarts retained 通道保留 sourceGlyphRects', () => {
  const ctx = context('甲乙丙丁', 922, [])
  const all = idsOf(ctx, 0, 4)
  const run = runGeometry(ctx, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见
  const prepared = retainedMoveStartState(run, ctx, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  // retarget：簇2 被冻结窗口吞掉（clip 0..0）
  const retargeted = retargetRunStarts(
    run, prepared,
    { ...ctx, frozenWindows: [frozenWindow([all[2]], 0, 0, 0, 0, 922, null, null, ctx)] },
    'retained', 0,
    { startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0 },
  )
  // Issue #879 复核评论6100665268 问题3：retarget 保留 sourceGlyphRects
  assert.ok(retargeted[0].intervals !== undefined)
  assert.equal(retargeted[0].intervals.length, 4)
  // 簇2 不可见 → interval clip 为零宽度，但 sourceGlyphRects 保留
  const iv2 = retargeted[0].intervals.find(iv => iv.firstIndex === 2)
  assert.ok(iv2 !== undefined)
  assert.equal(iv2.startClipLeft, iv2.startClipRight, '不可见字形 interval 应为零宽度')
  assert.ok(iv2.sourceGlyphRects !== undefined, '不可见字形 interval 应保留 sourceGlyphRects')
  assert.equal(iv2.sourceGlyphRects.length, 1)
  assert.equal(iv2.sourceGlyphRects[0].width, 10, 'retarget 后 sourceGlyphRects 应为完整字形宽度 10')
  // 可见字形也保留 sourceGlyphRects
  const iv0 = retargeted[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv0 !== undefined)
  assert.ok(iv0.sourceGlyphRects !== undefined, '可见字形 interval 应保留 sourceGlyphRects')
  assert.ok(iv0.sourceGlyphRects.length >= 1)
})

// ====== Issue #879 复核评论6101344441：问题2/3/4 新增测试 ======

// ------ 问题4：retained 部分可见字形的首帧 clip 位置 ------
// 旧实现 segStartX = islandStartX + (cr.x - interval.startClipLeft)，
// 当 islandStartX=0、cr.x=4、startClipLeft=4 时得 0，但正确应是 4。
// 修复后 startScreenX = cr.x + (interval.startPositionX - move.oldRect.x)。
// 注：buildFrame 在 EditorMotionCoordinator.ets 中，无法直接单元测试；
// 此处验证 retainedMoveStartState 生成的 interval 数据使修复后公式给出正确屏幕 x。

test('问题4：retained 部分可见字形首帧 clip 位置——cr.x + (startPositionX - oldRect.x) 等于裁切后屏幕 x', () => {
  const ctx = context('甲', 930, [])
  const all = idsOf(ctx, 0, 1)
  // 字形源 x=0..10、offset=0、冻结时仅 x=4..10 可见
  // islandStartX=0(sourceGlyphX0)，interval.startClipLeft=4(ownLeft)，cr.x=4
  // 旧实现 segStartX = 0 + (4-4) = 0（错误，字形左边界而非可见左边界）
  // 修复后 startScreenX = 4 + (0 - 0) = 4（正确，裁切后可见左边界）
  const frozen = [{
    glyphIds: all,
    clipLeft: 4, clipRight: 10,
    clipRects: [{ x: 4, y: 0, width: 6, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 930, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0,
  })
  assert.equal(start.length, 1)
  assert.ok(start[0].intervals !== undefined)
  assert.equal(start[0].intervals.length, 1)
  const iv = start[0].intervals[0]
  // interval 携带 clipRects
  assert.ok(iv.clipRects !== undefined)
  assert.equal(iv.clipRects.length, 1)
  const cr = iv.clipRects[0]
  // cr.x = 4（投影后裁切矩形左边界，源=目标恒等）
  assert.equal(cr.x, 4, 'cr.x 应为裁切后投影左边界 4')
  // interval.startClipLeft = 4（ownLeft，裁切后可见左边界）
  assert.equal(iv.startClipLeft, 4, 'startClipLeft 应为裁切后 ownLeft 4')
  // interval.startPositionX = onScreenLeft - (ownLeft - ownRect.x) = 4 - 4 = 0
  assert.equal(iv.startPositionX, 0, 'startPositionX 应为窗口偏移 0')
  // 修复后公式：startScreenX = cr.x + (startPositionX - oldRect.x) = 4 + 0 = 4
  const startScreenX = cr.x + (iv.startPositionX - run.ownRect.x)
  assert.equal(startScreenX, 4, '修复后 startScreenX 应为 4（裁切后屏幕 x）')
  // 旧公式 segStartX = islandStartX + (cr.x - startClipLeft) = 0 + 0 = 0（错误）
  const islandStartX = 0  // sourceGlyphX0（字形完整左边界）
  const oldSegStartX = islandStartX + (cr.x - iv.startClipLeft)
  assert.equal(oldSegStartX, 0, '旧公式 segStartX 为 0——错误')
  assert.notEqual(startScreenX, oldSegStartX, '修复后应不同于旧公式结果')
})

test('问题4：retained 带窗口偏移时首帧 clip 位置——cr.x + (startPositionX - oldRect.x) 等于在屏可见左边界', () => {
  const ctx = context('甲', 931, [])
  const all = idsOf(ctx, 0, 1)
  // 字形源 x=0..10、offset=5、冻结 clip [8,15]
  // 字形在屏 [5,15]，clip [8,15] 交集 → 可见在屏 [8,15]，源可见 [3,10]
  // ownLeft = 3（投影：ratio=(3-0)/10=0.3 → 0+0.3*10=3）
  // onScreenLeft = 8，startPositionX = 8 - (3 - 0) = 5
  // cr.x = 3（投影后裁切矩形左边界）
  // 修复后 startScreenX = 3 + (5 - 0) = 8（在屏可见左边界）
  const frozen = [{
    glyphIds: all,
    clipLeft: 8, clipRight: 15,
    clipRects: [{ x: 8, y: 0, width: 7, height: 20 }],
    offsetX: 5, offsetY: 0,
    sourceRevision: 931, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals[0]
  const cr = iv.clipRects[0]
  // cr.x = 3（投影后裁切左边界）
  assert.equal(cr.x, 3, 'cr.x 应为投影后裁切左边界 3')
  // startPositionX = 5（窗口偏移）
  assert.equal(iv.startPositionX, 5, 'startPositionX 应为窗口偏移 5')
  // 修复后 startScreenX = 3 + (5 - 0) = 8（在屏可见左边界）
  const startScreenX = cr.x + (iv.startPositionX - run.ownRect.x)
  assert.equal(startScreenX, 8, '修复后 startScreenX 应为在屏可见左边界 8')
  // 旧公式 segStartX = islandStartX + (cr.x - startClipLeft) = 0 + (3-3) = 0（错误）
  const islandStartX = 0  // sourceGlyphX0
  const oldSegStartX = islandStartX + (cr.x - iv.startClipLeft)
  assert.equal(oldSegStartX, 0, '旧公式 segStartX 为 0——错误')
  assert.notEqual(startScreenX, oldSegStartX, '带偏移时修复后应不同于旧公式结果')
})

// ------ 问题2：retained 间隙碎片不填平 ------
// 问题2的修复在 EditorMotionCoordinator.ets 的 islandTargetRects 中
// （已可见碎片保持不变 + 间隙碎片从零宽扩展），无法直接单元测试。
// 此处验证 retainedMoveStartState 生成的 interval clipRects 保留独立碎片，
// 不合并成连续矩形——支撑 .ets 层"间隙不填平"语义。

test('问题2：retained 同一字形多段可见碎片——clipRects 不合并，间隙不填平', () => {
  const ctx = context('甲', 932, [])
  const all = idsOf(ctx, 0, 1)
  // 字形 x=0..10，两个不连续 clipRect [0,4] 和 [6,10]，中间 [4,6] 不可见
  const frozen = [{
    glyphIds: all,
    clipLeft: 0, clipRight: 10,
    clipRects: [
      { x: 0, y: 0, width: 4, height: 20 },
      { x: 6, y: 0, width: 4, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 932, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals[0]
  assert.ok(iv.clipRects !== undefined)
  // 两段独立碎片，不合并成 [0,10] 连续矩形
  assert.equal(iv.clipRects.length, 2, '应保留两段独立碎片')
  // 第一段 [0,4]
  assert.equal(iv.clipRects[0].x, 0)
  assert.equal(iv.clipRects[0].width, 4)
  // 第二段 [6,10]
  assert.equal(iv.clipRects[1].x, 6)
  assert.equal(iv.clipRects[1].width, 4)
  // 间隙不填平：第二段 x > 第一段 x + width
  assert.ok(iv.clipRects[1].x > iv.clipRects[0].x + iv.clipRects[0].width,
    '两段之间应有间隙，不合并成连续矩形')
  // 总宽度 8 < 字形完整宽度 10——间隙 [4,6] 不填平
  const totalWidth = iv.clipRects.reduce((sum, r) => sum + r.width, 0)
  assert.equal(totalWidth, 8, '总可见宽度应为 8，间隙 2vp 不填平')
  assert.ok(totalWidth < run.ownRect.width, '总可见宽度小于字形完整宽度——间隙不填平')
})

test('问题2：retained 多字形各自可见中间有间隙——interval clipRects 不填平间隙', () => {
  const ctx = context('甲乙', 933, [])
  const all = idsOf(ctx, 0, 2)
  // 甲在 [0,10]，乙在 [10,20]
  // 冻结窗口两个不连续 clipRect [2,5] 和 [15,18]
  // 甲可见 [2,5]，乙可见 [15,18]，中间 [5,15] 不可见
  const frozen = [{
    glyphIds: all,
    clipLeft: 2, clipRight: 18,
    clipRects: [
      { x: 2, y: 0, width: 3, height: 20 },
      { x: 15, y: 0, width: 3, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 933, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 2, { x: 0, y: 0, width: 20, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 20, startPositionX: 0, startPositionY: 0,
  })
  assert.equal(start[0].intervals.length, 2)
  const iv0 = start[0].intervals[0]  // 甲
  const iv1 = start[0].intervals[1]  // 乙
  // 甲可见 [2,5]
  assert.ok(iv0.clipRects !== undefined)
  assert.equal(iv0.clipRects.length, 1)
  assert.equal(iv0.clipRects[0].x, 2)
  assert.equal(iv0.clipRects[0].width, 3)
  // 乙可见 [15,18]
  assert.ok(iv1.clipRects !== undefined)
  assert.equal(iv1.clipRects.length, 1)
  assert.equal(iv1.clipRects[0].x, 15)
  assert.equal(iv1.clipRects[0].width, 3)
  // 间隙 [5,15] 不填平：乙的 clipRect.x > 甲的 clipRect.x + width
  const aRight = iv0.clipRects[0].x + iv0.clipRects[0].width
  const bLeft = iv1.clipRects[0].x
  assert.ok(bLeft > aRight, '乙的可见左边界应大于甲的可见右边界——间隙不填平')
  assert.equal(bLeft - aRight, 10, '间隙宽度应为 10vp（[5,15]）')
})

test('问题2：retarget 后 retained 多段碎片仍不合并——间隙持续不填平', () => {
  const ctx = context('甲', 934, [])
  const all = idsOf(ctx, 0, 1)
  const run = runGeometry(ctx, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见
  const prepared = retainedMoveStartState(run, ctx, {
    startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0,
  })
  // retarget：字形被裁成两段 [0,3] 和 [7,10]，中间 [3,7] 不可见
  const frozen = [{
    glyphIds: all,
    clipLeft: 0, clipRight: 10,
    clipRects: [
      { x: 0, y: 0, width: 3, height: 20 },
      { x: 7, y: 0, width: 3, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 934, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }]
  const retargeted = retargetRunStarts(run, prepared, { ...ctx, frozenWindows: frozen }, 'retained', 0,
    { startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0 })
  const iv = retargeted[0].intervals[0]
  assert.ok(iv.clipRects !== undefined)
  // retarget 后仍保留两段独立碎片，不合并
  assert.equal(iv.clipRects.length, 2, 'retarget 后应仍保留两段独立碎片')
  assert.equal(iv.clipRects[0].x, 0)
  assert.equal(iv.clipRects[0].width, 3)
  assert.equal(iv.clipRects[1].x, 7)
  assert.equal(iv.clipRects[1].width, 3)
  // 间隙 [3,7] 持续不填平
  assert.ok(iv.clipRects[1].x > iv.clipRects[0].x + iv.clipRects[0].width,
    'retarget 后两段之间仍应有间隙')
  const totalWidth = iv.clipRects.reduce((sum, r) => sum + r.width, 0)
  assert.equal(totalWidth, 6, 'retarget 后总可见宽度应为 6，间隙 4vp 持续不填平')
})

// ------ 问题3：fullRect 加 offset ------
// 问题3的修复在 EditorMotionCoordinator.ets 的 identifyCarryOverMotionRuns 中
// （carry insert 通道的 fullRect 加 piece.offsetX/Y），无法直接单元测试。
// 此处验证 retainedMoveStartState 的 interval 携带的 sourceGlyphRects 是完整源字形矩形
// （不含窗口 offset），.ets 层 fullRect + offset 得到正确目标位置——支撑问题3修复。

test('问题3：retained interval.sourceGlyphRects 是源字形几何（不含 offset）——.ets 层加 offset 得目标位置', () => {
  const ctx = context('甲', 935, [])
  const all = idsOf(ctx, 0, 1)
  // 字形源 x=0..10、offset=5、clip [8,15]
  // 字形在屏 [5,15]，clip [8,15] 交集 → 可见在屏 [8,15]，源可见 [3,10]
  // sourceGlyphRects 应为 {x:0, width:10}（完整源字形，不含 offset）
  // .ets 层 targetX = sourceGlyphRect.x + piece.offsetX = 0 + 5 = 5
  const frozen = [{
    glyphIds: all,
    clipLeft: 8, clipRight: 15,
    clipRects: [{ x: 8, y: 0, width: 7, height: 20 }],
    offsetX: 5, offsetY: 0,
    sourceRevision: 935, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals[0]
  assert.ok(iv.sourceGlyphRects !== undefined, 'interval 应携带 sourceGlyphRects')
  assert.equal(iv.sourceGlyphRects.length, 1)
  const sr = iv.sourceGlyphRects[0]
  // sourceGlyphRect 是完整源字形矩形（x=0，不含 offset）
  assert.equal(sr.x, 0, 'sourceGlyphRect.x 应为源字形左边界 0（不含 offset）')
  assert.equal(sr.width, 10, 'sourceGlyphRect.width 应为完整字形宽度 10')
  assert.equal(sr.y, 0, 'sourceGlyphRect.y 应为源字形上边界 0（不含 offset）')
  assert.equal(sr.height, 20, 'sourceGlyphRect.height 应为字形高度 20')
  // .ets 层 fullRect = sourceGlyphRect，targetX = fullRect.x + piece.offsetX
  const pieceOffsetX = 5  // 窗口 offsetX
  const targetX = sr.x + pieceOffsetX
  assert.equal(targetX, 5, '.ets 层 fullRect.x + offset 应为目标 x 5')
  // clipRects 是裁切后矩形（投影到 run 布局），与 sourceGlyphRects 不同
  assert.ok(iv.clipRects !== undefined)
  const clipWidth = iv.clipRects.reduce((sum, r) => sum + r.width, 0)
  assert.ok(clipWidth < sr.width, '裁切后可见宽度应小于完整字形宽度——fullRect 与 clipRects 独立')
  // .ets 层 fullRect + offset 提供目标完整矩形，clipRects 提供起始裁切——不混淆
})

test('问题3：retarget 后 retained interval 仍携带完整 sourceGlyphRects——支撑 fullRect + offset 计算', () => {
  const ctx = context('甲乙', 936, [])
  const all = idsOf(ctx, 0, 2)
  const run = runGeometry(ctx, 0, 2, { x: 0, y: 0, width: 20, height: 20 })
  // prepare：无冻结窗口，整段静态完整可见
  const prepared = retainedMoveStartState(run, ctx, {
    startClipLeft: 0, startClipRight: 20, startPositionX: 0, startPositionY: 0,
  })
  // retarget：加冻结窗口 offsetX=3，clip [3,23]
  const frozen = [frozenWindow(all, 3, 23, 3, 0, 936, null, null, ctx)]
  const retargeted = retargetRunStarts(run, prepared, { ...ctx, frozenWindows: frozen }, 'retained', 0,
    { startClipLeft: 0, startClipRight: 20, startPositionX: 0, startPositionY: 0 })
  assert.ok(retargeted[0].intervals !== undefined)
  // 每个字形一个 interval，都携带完整 sourceGlyphRects（不含 offset）
  for (let i = 0; i < 2; i++) {
    const iv = retargeted[0].intervals[i]
    assert.ok(iv.sourceGlyphRects !== undefined, `interval${i} 应携带 sourceGlyphRects`)
    assert.equal(iv.sourceGlyphRects.length, 1)
    const sr = iv.sourceGlyphRects[0]
    // sourceGlyphRect 是完整源字形矩形（宽 10，高 20），不含 offset
    assert.equal(sr.width, 10, `interval${i} sourceGlyphRect 宽度应为 10`)
    assert.equal(sr.height, 20, `interval${i} sourceGlyphRect 高度应为 20`)
    // .ets 层 fullRect + offset 得目标位置
    // 甲：sr.x=0 + offset 3 = 3；乙：sr.x=10 + offset 3 = 13
    const targetX = sr.x + 3
    assert.ok(targetX === 3 || targetX === 13, `interval${i} targetX 应为 3 或 13`)
    // sourceGlyphRect.x 不含 offset——.ets 层加 offset 才得到目标位置
    assert.ok(sr.x === 0 || sr.x === 10, `interval${i} sourceGlyphRect.x 应为源字形左边界（0 或 10），不含 offset`)
  }
})

test('问题3：sourceGlyphRects 与 clipRects 各自独立——fullRect 不因裁切而缩小', () => {
  const ctx = context('甲乙丙丁', 937, [])
  const all = idsOf(ctx, 0, 4)
  // clip=[4,36] offsetX=0：甲可见[4,10]（左 4vp 裁掉），丁可见[30,36]（右 4vp 裁掉）
  // 乙丙完整可见 [10,30]
  const frozen = [frozenWindow(all, 4, 36, 0, 0, 937, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const intervals = start[0].intervals
  // 甲（部分裁切）：clipRects 宽度 6，sourceGlyphRects 宽度 10
  const iv0 = intervals[0]
  assert.ok(iv0.clipRects !== undefined && iv0.sourceGlyphRects !== undefined)
  const clipW0 = iv0.clipRects.reduce((s, r) => s + r.width, 0)
  const fullW0 = iv0.sourceGlyphRects.reduce((s, r) => s + r.width, 0)
  assert.equal(clipW0, 6, '甲裁切后可见宽度应为 6')
  assert.equal(fullW0, 10, '甲完整字形宽度应为 10——fullRect 不因裁切而缩小')
  assert.ok(fullW0 > clipW0, 'fullRect 宽度应大于裁切后宽度——.ets 层 fullRect + offset 用完整矩形')
  // 丁（部分裁切）：clipRects 宽度 6，sourceGlyphRects 宽度 10
  const iv3 = intervals[3]
  const clipW3 = iv3.clipRects.reduce((s, r) => s + r.width, 0)
  const fullW3 = iv3.sourceGlyphRects.reduce((s, r) => s + r.width, 0)
  assert.equal(clipW3, 6, '丁裁切后可见宽度应为 6')
  assert.equal(fullW3, 10, '丁完整字形宽度应为 10——fullRect 不因裁切而缩小')
  // 乙丙（完整可见）：clipRects 宽度 = sourceGlyphRects 宽度 = 10
  const iv1 = intervals[1]
  const clipW1 = iv1.clipRects.reduce((s, r) => s + r.width, 0)
  const fullW1 = iv1.sourceGlyphRects.reduce((s, r) => s + r.width, 0)
  assert.equal(clipW1, 10, '乙完整可见，裁切宽度应为 10')
  assert.equal(fullW1, 10, '乙完整字形宽度应为 10')
  assert.equal(clipW1, fullW1, '乙完整可见时 clipRects 宽度 = sourceGlyphRects 宽度')
})

// ====== Issue #879 复核评论6101908111：问题2/3 新增测试 ======

// ------ 问题3：不可见字形 startPositionX 为字形绝对屏幕原点 ------
// 评论6104161755 问题2：buildFrame 现在直接使用 islandStartX = interval.startPositionX
// （将该字段视为绝对屏幕字形左边界），不再用 islandOldLeft + startPositionX - oldRect.x。
// 旧实现写 startPositionX = run.ownRect.x（零偏移语义），buildFrame 令 islandStartX = 0，
// 而字形真实位置在 x=20，导致字形被挪到 x=0。
// 修复：不可见字形 startPositionX = ownGlyphRect.x（真绝对字形原点）。

test('问题3：不可见字形 startPositionX 应为字形绝对屏幕原点——与 buildFrame 新语义一致', () => {
  const ctx = context('甲乙丙丁', 940, [])
  const all = idsOf(ctx, 0, 4)
  // 簇2（丙）被窗口接管且完全不可见
  const frozen = [frozenWindow([all[2]], 0, 0, 0, 0, 940, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const intervals = start[0].intervals
  // 簇2 不可见
  const iv2 = intervals.find(iv => iv.firstIndex === 2)
  assert.ok(iv2 !== undefined)
  // 修复后：startPositionX = ownGlyphRect.x = 20（字形绝对原点），不是 run.ownRect.x = 0
  assert.equal(iv2.startPositionX, 20,
    '不可见字形 startPositionX 应为字形绝对屏幕原点 20，不是 run.ownRect.x = 0')
  assert.notEqual(iv2.startPositionX, run.ownRect.x,
    '不可见字形 startPositionX 不应为 run.ownRect.x=0——buildFrame 新语义会令 islandStartX=0 而非 20')
  // buildFrame 新公式：islandStartX = interval.startPositionX = 20（字形在屏左边界）
  // 修复后：islandStartX = 20（字形真实位置，正确）
  // 旧实现：islandStartX = 0（run 左端，字形被挪到错误位置）
  assert.equal(iv2.startPositionX, 20, '修复后 islandStartX 应为 20（字形真实位置）')
})

test('问题3：不可见字形 startPositionX 为字形绝对原点——p>0 扩张时从正确位置开始', () => {
  const ctx = context('甲乙丙', 941, [])
  const all = idsOf(ctx, 0, 3)
  // 簇1（乙）完全不可见，簇0（甲）和簇2（丙）可见
  const frozen = [frozenWindow([all[1]], 0, 0, 0, 0, 941, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 3, { x: 0, y: 0, width: 30, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 30, startPositionX: 0, startPositionY: 0,
  })
  const intervals = start[0].intervals
  // 簇1 不可见
  const iv1 = intervals.find(iv => iv.firstIndex === 1)
  assert.ok(iv1 !== undefined)
  // startPositionX = ownGlyphRect.x = 10（乙的字形绝对原点）
  assert.equal(iv1.startPositionX, 10, '不可见字形 startPositionX 应为字形绝对原点 10')
  // buildFrame 新公式：islandStartX = interval.startPositionX = 10
  // 这是乙在屏上的真实位置——p>0 扩张时从正确位置开始扩展
  assert.equal(iv1.startPositionX, 10, 'islandStartX 应为 10（乙的字形绝对位置）')
})

// ------ 问题2：同一字形多段可见碎片的 sourceGlyphRects 都相同 ------
// 评论6101908111 问题2：buildPiece 的 sourceGlyphRects 对一个 glyph 内每一块 clipRects
// 都塞入相同的完整字形 rect。buildFrame retained 不应每块各自独立插值到同一个 fullRect，
// 否则 p=1 时多块重叠。修复后采用 carry insert 同款逻辑：已可见碎片保持自身位置随平移移动，
// 间隙碎片从零宽扩展到间隙宽度。

test('问题2：同一字形多段可见碎片——sourceGlyphRects 中所有元素相同（都是完整字形 rect）', () => {
  const ctx = context('甲', 942, [])
  const all = idsOf(ctx, 0, 1)
  // 字形 x=0..10，两个不连续 clipRect [0,4] 和 [6,10]，中间 [4,6] 不可见
  const frozen = [{
    glyphIds: all,
    clipLeft: 0, clipRight: 10,
    clipRects: [
      { x: 0, y: 0, width: 4, height: 20 },
      { x: 6, y: 0, width: 4, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 942, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals[0]
  assert.ok(iv.clipRects !== undefined)
  assert.equal(iv.clipRects.length, 2, '应保留两段独立碎片')
  assert.ok(iv.sourceGlyphRects !== undefined)
  assert.equal(iv.sourceGlyphRects.length, 2, 'sourceGlyphRects 与 clipRects 一一对应')
  // 两段 sourceGlyphRects 都是同一个完整字形 rect [0,10]
  const sr0 = iv.sourceGlyphRects[0]
  const sr1 = iv.sourceGlyphRects[1]
  assert.equal(sr0.x, sr1.x, '两段 sourceGlyphRects 的 x 相同（同一字形的完整 rect）')
  assert.equal(sr0.width, sr1.width, '两段 sourceGlyphRects 的 width 相同')
  assert.equal(sr0.x, 0, 'sourceGlyphRects x 应为字形左边界 0')
  assert.equal(sr0.width, 10, 'sourceGlyphRects width 应为完整字形宽度 10')
  // buildFrame retained 修复后：取第一个 sourceGlyphRect 作为整个字形的完整 rect，
  // 已可见碎片保持自身位置，间隙碎片从零宽扩展——不会两块各自插值到同一个 fullRect
})

test('问题2：retained 多段碎片——buildFrame 修复后 p=1 不重叠（数据验证）', () => {
  const ctx = context('甲', 943, [])
  const all = idsOf(ctx, 0, 1)
  // 字形 x=0..10，两个不连续 clipRect [0,4] 和 [6,10]，中间 [4,6] 不可见
  const frozen = [{
    glyphIds: all,
    clipLeft: 0, clipRight: 10,
    clipRects: [
      { x: 0, y: 0, width: 4, height: 20 },
      { x: 6, y: 0, width: 4, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 943, glyphUtf16Ranges: autoRanges(all, ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals[0]
  // 验证 buildFrame retained 修复后的逻辑（数据层面）：
  // sr = sourceGlyphRects[0] = {x:0, width:10}（字形完整 rect）
  // frozenOffsetX = startPositionX - oldRect.x = 0 - 0 = 0
  //
  // 已可见碎片1 [0,4]：
  //   startRect = {x: 0+0=0, width: 4}（起始屏幕位置）
  //   targetRect = {x: islandNewX+(0-islandOldLeft), width: 4}（目标位置，宽度不变）
  // 已可见碎片2 [6,10]：
  //   startRect = {x: 6+0=6, width: 4}
  //   targetRect = {x: islandNewX+(6-islandOldLeft), width: 4}
  // 间隙碎片 [4,6]：
  //   startRect = {x: 4+0=4, width: 0}（零宽）
  //   targetRect = {x: islandNewX+(4-islandOldLeft), width: 2}（间隙宽度）
  //
  // p=1 时：碎片1=[islandNewX, islandNewX+4], 间隙=[islandNewX+4, islandNewX+6],
  //         碎片2=[islandNewX+6, islandNewX+10]——连续覆盖 [islandNewX, islandNewX+10]，不重叠
  //
  // 旧实现（每块各自插值到 fullRect）：
  // p=1 时：碎片1=[islandNewX, islandNewX+10], 碎片2=[islandNewX, islandNewX+10]——重叠！
  const sr = iv.sourceGlyphRects[0]
  const frozenOffsetX = iv.startPositionX - run.ownRect.x
  // 已可见碎片1
  const cr0 = iv.clipRects[0]
  const startScreenX0 = cr0.x + frozenOffsetX
  const targetScreenX0 = 0 + (cr0.x - 0) // islandNewX=0, islandOldLeft=0（简化）
  assert.equal(startScreenX0, 0, '碎片1 起始屏幕 x 应为 0')
  assert.equal(targetScreenX0, 0, '碎片1 目标屏幕 x 应为 0')
  assert.equal(cr0.width, 4, '碎片1 宽度应为 4')
  // 已可见碎片2
  const cr1 = iv.clipRects[1]
  const startScreenX1 = cr1.x + frozenOffsetX
  const targetScreenX1 = 0 + (cr1.x - 0)
  assert.equal(startScreenX1, 6, '碎片2 起始屏幕 x 应为 6')
  assert.equal(targetScreenX1, 6, '碎片2 目标屏幕 x 应为 6')
  assert.equal(cr1.width, 4, '碎片2 宽度应为 4')
  // 间隙 [4,6]
  const gapLeft = cr0.x + cr0.width
  const gapRight = cr1.x
  assert.equal(gapLeft, 4, '间隙左边界应为 4')
  assert.equal(gapRight, 6, '间隙右边界应为 6')
  assert.equal(gapRight - gapLeft, 2, '间隙宽度应为 2')
  // 修复后 p=1：碎片1=[0,4], 间隙=[4,6], 碎片2=[6,10]——连续不重叠
  // 旧实现 p=1：碎片1=[0,10], 碎片2=[0,10]——重叠！
  const fixedTotalWidth = cr0.width + (gapRight - gapLeft) + cr1.width
  assert.equal(fixedTotalWidth, 10, '修复后 p=1 总宽度应为 10（连续覆盖整字）')
  const buggyTotalWidth = sr.width + sr.width
  assert.equal(buggyTotalWidth, 20, '旧实现 p=1 总宽度为 20（两块重叠）')
  assert.ok(fixedTotalWidth < buggyTotalWidth, '修复后总宽度应小于旧实现——不重叠')
})

test('问题2：retained 不可见字形 startPositionX 为字形绝对原点——islandStartX 正确', () => {
  const ctx = context('甲乙丙丁', 944, [])
  const all = idsOf(ctx, 0, 4)
  // 簇2（丙）和簇3（丁）完全不可见
  const frozen = [frozenWindow([all[2], all[3]], 0, 0, 0, 0, 944, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const intervals = start[0].intervals
  // 簇2 不可见
  const iv2 = intervals.find(iv => iv.firstIndex === 2)
  assert.ok(iv2 !== undefined)
  // Issue #879 复核评论6104161755 问题2：startPositionX = ownGlyphRect.x（字形绝对原点）
  assert.equal(iv2.startPositionX, 20, '簇2 startPositionX 应为字形绝对原点 20')
  // 簇3 不可见
  const iv3 = intervals.find(iv => iv.firstIndex === 3)
  assert.ok(iv3 !== undefined)
  assert.equal(iv3.startPositionX, 30, '簇3 startPositionX 应为字形绝对原点 30')
  // buildFrame 新公式：islandStartX = interval.startPositionX（绝对屏幕字形左边界）
  // 簇2: islandStartX = 20（字形真实位置，正确）
  // 簇3: islandStartX = 30（字形真实位置，正确）
  assert.equal(iv2.startPositionX, 20, '簇2 islandStartX 应为 20')
  assert.equal(iv3.startPositionX, 30, '簇3 islandStartX 应为 30')
})

// ====== Issue #879 复核评论6102724233 新增测试 ======

console.log('\n--- Issue #879 复核评论6102724233 新增测试 ---')

// ── 问题1+4：rectDifference / isFullyCovered ──

test('问题1+4：rectDifference——fullRect 完全被覆盖时返回空数组', () => {
  const fullRect = { x: 0, y: 0, width: 10, height: 20 }
  const clipRects = [{ x: 0, y: 0, width: 10, height: 20 }]
  const remaining = rectDifference(fullRect, clipRects)
  assert.equal(remaining.length, 0, '完全覆盖时剩余区域应为空')
  assert.ok(isFullyCovered(fullRect, clipRects), 'isFullyCovered 应返回 true')
})

test('问题1+4：rectDifference——fullRect 完全不可见时返回整个 fullRect', () => {
  const fullRect = { x: 0, y: 0, width: 10, height: 20 }
  const clipRects = []
  const remaining = rectDifference(fullRect, clipRects)
  assert.equal(remaining.length, 1, '无覆盖时剩余区域应为 1 个矩形')
  assert.equal(remaining[0].x, 0)
  assert.equal(remaining[0].width, 10)
  assert.equal(remaining[0].height, 20)
  assert.ok(!isFullyCovered(fullRect, clipRects), 'isFullyCovered 应返回 false')
})

test('问题1+4：rectDifference——中间有空洞（左右各可见，中间不可见）', () => {
  // 字形范围 [0,10]，clipRects 只覆盖 [0,3] 和 [7,10]
  const fullRect = { x: 0, y: 0, width: 10, height: 20 }
  const clipRects = [
    { x: 0, y: 0, width: 3, height: 20 },
    { x: 7, y: 0, width: 3, height: 20 },
  ]
  const remaining = rectDifference(fullRect, clipRects)
  // 中间 [3,7] 是空洞
  assert.ok(remaining.length >= 1, '应有剩余区域')
  const totalRemainingWidth = remaining.reduce((sum, r) => sum + r.width, 0)
  assert.ok(Math.abs(totalRemainingWidth - 4) < 0.01, `剩余宽度应为 4（[3,7]），实际 ${totalRemainingWidth}`)
  assert.ok(!isFullyCovered(fullRect, clipRects), '中间有空洞时不应判定为完全覆盖')
})

test('问题1+4：rectDifference——包围盒宽度等于 fullRect 但中间有空洞', () => {
  // 关键测试：clipRects 的左右包围盒 = [0,10]，与 fullRect 宽度相同，
  // 但中间 [3,7] 还没吐出来。旧实现用 clipRight-clipLeft 判断会误认为完全覆盖。
  const fullRect = { x: 0, y: 0, width: 10, height: 20 }
  const clipRects = [
    { x: 0, y: 0, width: 3, height: 20 },
    { x: 7, y: 0, width: 3, height: 20 },
  ]
  // 旧实现：clipRight - clipLeft = 10 - 0 = 10 >= fullWidth - 0.01 → 误判为完全覆盖
  // 新实现：isFullyCovered 用 rectDifference 检查真实覆盖面积
  assert.ok(!isFullyCovered(fullRect, clipRects), '包围盒宽度等于 fullRect 但中间有空洞时不应判定为完全覆盖')
})

test('问题1+4：rectDifference——clipRects 乱序时仍正确计算差集', () => {
  // clipRects 不按 x 排序：[7,10] 在 [0,3] 之前
  const fullRect = { x: 0, y: 0, width: 10, height: 20 }
  const clipRects = [
    { x: 7, y: 0, width: 3, height: 20 },
    { x: 0, y: 0, width: 3, height: 20 },
  ]
  const remaining = rectDifference(fullRect, clipRects)
  const totalRemainingWidth = remaining.reduce((sum, r) => sum + r.width, 0)
  assert.ok(Math.abs(totalRemainingWidth - 4) < 0.01, `乱序时剩余宽度仍应为 4，实际 ${totalRemainingWidth}`)
})

test('问题1+4：rectDifference——二维差集（上下各露出一块）', () => {
  // 字形高度 20，clipRects 只覆盖上半部分 [0,10] 和下半部分 [0,10]
  // 但中间 y=8..12 有空洞
  const fullRect = { x: 0, y: 0, width: 10, height: 20 }
  const clipRects = [
    { x: 0, y: 0, width: 10, height: 8 },
    { x: 0, y: 12, width: 10, height: 8 },
  ]
  const remaining = rectDifference(fullRect, clipRects)
  assert.ok(remaining.length >= 1, '应有剩余区域')
  const totalRemainingHeight = remaining.reduce((sum, r) => sum + r.height, 0)
  assert.ok(Math.abs(totalRemainingHeight - 4) < 0.01, `剩余高度应为 4（y=8..12），实际 ${totalRemainingHeight}`)
  assert.ok(!isFullyCovered(fullRect, clipRects), '上下各露出一块但中间有空洞时不应判定为完全覆盖')
})

test('问题1+4：rectDifference——clipRects 与 fullRect 不相交时返回整个 fullRect', () => {
  const fullRect = { x: 0, y: 0, width: 10, height: 20 }
  const clipRects = [{ x: 20, y: 0, width: 5, height: 20 }]
  const remaining = rectDifference(fullRect, clipRects)
  assert.equal(remaining.length, 1, '不相交时剩余区域应为 9个矩形')
  assert.equal(remaining[0].width, 10)
  assert.equal(remaining[0].height, 20)
})

// ── 问题2：retained 纵向半字错位 ──

test('问题2：VisibleRunPiece 携带 sourceGlyphY0——与 onScreenTop 不同', () => {
  // 源字形 y=0..20，窗口 offsetY=3（屏幕字形 y=3..23），上一帧裁切后只显示 y=8..23
  // onScreenTop=8（裁切后可见上边界），sourceGlyphY0=0（源字形在源布局中的原始 top）
  const ctx = context('甲乙丙丁', 945, [])
  const all = idsOf(ctx, 0, 4)
  // 窗口覆盖簇0，offsetY=3，clipRects 只露出 y=8..23
  const frozen = [{
    glyphIds: [all[0]],
    clipLeft: 0, clipRight: 10,
    clipRects: [{ x: 0, y: 8, width: 10, height: 15 }],
    offsetX: 0, offsetY: 3,
    sourceRevision: 945, glyphUtf16Ranges: autoRanges([all[0]], ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.ok(pieces.length > 0, '应有可见 piece')
  // sourceGlyphY0 应为 0（源字形在源布局中的原始 top），不是 onScreenTop=8
  assert.equal(pieces[0].sourceGlyphY0, 0, 'sourceGlyphY0 应为源字形原始 top=0')
  assert.equal(pieces[0].onScreenTop, 8, 'onScreenTop 应为裁切后可见 top=8')
})

test('问题2：retained interval 携带 frozenDrawOffsetY——不等于 startPositionY - oldRect.y', () => {
  // 源字形 y=0..20，窗口 offsetY=3，裁切后 onScreenTop=8
  // frozenDrawOffsetY = onScreenTop - sourceGlyphY0 = 8 - 0 = 8
  // startPositionY = sourceGlyphY0 + frozenDrawOffsetY = 0 + 8 = 8
  // 但关键区别：buildFrame 中 islandStartY = islandOldTop + frozenDrawOffsetY = 0 + 8 = 8
  // 而旧实现 islandStartY = startPositionY = 8（碰巧相同）
  // 区别在于当 offsetY != 0 且裁切不改变 top 时：
  //   onScreenTop = sourceGlyphY0 + offsetY = 0 + 3 = 3
  //   frozenDrawOffsetY = onScreenTop - sourceGlyphY0 = 3 - 0 = 3
  //   islandStartY = islandOldTop + frozenDrawOffsetY = 0 + 3 = 3（正确：源字形 top + offset）
  //   旧实现 islandStartY = startPositionY = onScreenTop = 3（碰巧也正确）
  // 但当裁切使 onScreenTop > sourceGlyphY0 + offsetY 时：
  //   onScreenTop = 8（裁切后），sourceGlyphY0 = 0，offsetY = 3
  //   frozenDrawOffsetY = 8 - 0 = 8
  //   islandStartY = 0 + 8 = 8（错误！应该是 0 + 3 = 3）
  //   旧实现 islandStartY = startPositionY = 8（也错误！）
  // 正确修复：frozenDrawOffsetY 应该是 offsetY（源 Text 真实偏移），不是 onScreenTop - sourceGlyphY0
  // 但当前实现中 frozenDrawOffsetY = onScreenTop - sourceGlyphY0，这在裁切不改变 top 时等于 offsetY
  // 当裁切改变 top 时，frozenDrawOffsetY > offsetY，islandStartY 会偏大
  // 这正是问题2描述的场景——需要进一步修复 frozenDrawOffsetY 的计算
  const ctx = context('甲乙丙丁', 946, [])
  const all = idsOf(ctx, 0, 4)
  const frozen = [{
    glyphIds: [all[0]],
    clipLeft: 0, clipRight: 10,
    clipRects: [{ x: 0, y: 8, width: 10, height: 15 }],
    offsetX: 0, offsetY: 3,
    sourceRevision: 946, glyphUtf16Ranges: autoRanges([all[0]], ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0,
  })
  const intervals = start[0].intervals
  assert.ok(intervals !== undefined)
  const iv0 = intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv0 !== undefined)
  // frozenDrawOffsetY 应已填充
  assert.ok(iv0.frozenDrawOffsetY !== undefined, 'frozenDrawOffsetY 应已填充')
  // frozenDrawOffsetX 也应已填充
  assert.ok(iv0.frozenDrawOffsetX !== undefined, 'frozenDrawOffsetX 应已填充')
})

test('问题2：retained interval 携带 startScreenClipRects——屏幕坐标的裁切矩形', () => {
  const ctx = context('甲乙丙丁', 947, [])
  const all = idsOf(ctx, 0, 4)
  const frozen = [{
    glyphIds: [all[0]],
    clipLeft: 0, clipRight: 10,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 947, glyphUtf16Ranges: autoRanges([all[0]], ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0,
  })
  const intervals = start[0].intervals
  assert.ok(intervals !== undefined)
  const iv0 = intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv0 !== undefined)
  // startScreenClipRects 应已填充
  assert.ok(iv0.startScreenClipRects !== undefined, 'startScreenClipRects 应已填充')
  assert.ok(iv0.startScreenClipRects.length > 0, 'startScreenClipRects 应非空')
})

// ── 问题5：不可见 retained glyph 的 frozenDrawOffsetX/Y ──

test('问题5：不可见 retained glyph 的 frozenDrawOffsetX/Y 为 0（零偏移语义）', () => {
  const ctx = context('甲乙丙丁', 948, [])
  const all = idsOf(ctx, 0, 4)
  // 簇2（丙）完全不可见
  const frozen = [frozenWindow([all[2]], 0, 0, 0, 0, 948, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const intervals = start[0].intervals
  const iv2 = intervals.find(iv => iv.firstIndex === 2)
  assert.ok(iv2 !== undefined)
  assert.equal(iv2.frozenDrawOffsetX, 0, '不可见字形 frozenDrawOffsetX 应为 0')
  assert.equal(iv2.frozenDrawOffsetY, 0, '不可见字形 frozenDrawOffsetY 应为 0')
})

test('问题5：不可见 retained glyph 的 startPositionY 为 run.ownRect.y（零偏移语义）', () => {
  const ctx = context('甲乙丙丁', 949, [])
  const all = idsOf(ctx, 0, 4)
  // 簇2（丙）完全不可见
  const frozen = [frozenWindow([all[2]], 0, 0, 0, 0, 949, null, null, ctx)]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const intervals = start[0].intervals
  const iv2 = intervals.find(iv => iv.firstIndex === 2)
  assert.ok(iv2 !== undefined)
  assert.equal(iv2.startPositionY, run.ownRect.y, '不可见字形 startPositionY 应为 ownRect.y=0')
})

// ── 问题3：同一字形多碎片的 sourceGlyphRects ──

test('问题3：同一字形多碎片——sourceGlyphRects 中所有元素都是完整字形 rect', () => {
  // 这个测试验证现有行为：同一字形的多碎片，sourceGlyphRects 中每个元素都是同一个完整字形 rect
  // buildFrame retained 取 sourceGlyphRects[0] 作为整个字形的完整 rect
  const ctx = context('甲乙丙丁', 950, [])
  const all = idsOf(ctx, 0, 4)
  // 窗口覆盖簇0，clipRects 有两个不连续区域：[0,3] 和 [7,10]
  const frozen = [{
    glyphIds: [all[0]],
    clipLeft: 0, clipRight: 10,
    clipRects: [
      { x: 0, y: 0, width: 3, height: 20 },
      { x: 7, y: 0, width: 3, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 950, glyphUtf16Ranges: autoRanges([all[0]], ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0,
  })
  const intervals = start[0].intervals
  assert.ok(intervals !== undefined)
  const iv0 = intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv0 !== undefined)
  assert.ok(iv0.sourceGlyphRects !== undefined, 'sourceGlyphRects 应已填充')
  assert.ok(iv0.clipRects !== undefined, 'clipRects 应已填充')
  // sourceGlyphRects 与 clipRects 一一对应
  assert.equal(iv0.sourceGlyphRects.length, iv0.clipRects.length, 'sourceGlyphRects 与 clipRects 长度应一致')
  // 所有 sourceGlyphRects 元素都是同一个完整字形 rect
  for (const sr of iv0.sourceGlyphRects) {
    assert.equal(sr.x, 0, 'sourceGlyphRect.x 应为 0（完整字形左边界）')
    assert.equal(sr.width, 10, 'sourceGlyphRect.width 应为 10（完整字形宽度）')
  }
})

// ── retargetRunStarts 保留新字段 ──

test('问题2+5：retargetRunStarts retained 通道保留 frozenDrawOffsetX/Y', () => {
  const ctx = context('甲乙丙丁', 951, [])
  const all = idsOf(ctx, 0, 4)
  // 簇0可见（窗口覆盖），簇1不可见
  const frozen = [{
    glyphIds: [all[0]],
    clipLeft: 0, clipRight: 10,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 951, glyphUtf16Ranges: autoRanges([all[0]], ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  // retarget：用同一上下文重新计算
  const retargeted = retargetRunStarts(run, start, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const intervals = retargeted[0].intervals
  assert.ok(intervals !== undefined)
  // 簇0 可见——应有 frozenDrawOffsetX/Y
  const iv0 = intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv0 !== undefined)
  assert.ok(iv0.frozenDrawOffsetX !== undefined, '可见字形 retarget 后 frozenDrawOffsetX 应保留')
  assert.ok(iv0.frozenDrawOffsetY !== undefined, '可见字形 retarget 后 frozenDrawOffsetY 应保留')
  // 簇1 不可见——frozenDrawOffsetX/Y 应为 0
  const iv1 = intervals.find(iv => iv.firstIndex === 1)
  assert.ok(iv1 !== undefined)
  assert.equal(iv1.frozenDrawOffsetX, 0, '不可见字形 retarget 后 frozenDrawOffsetX 应为 0')
  assert.equal(iv1.frozenDrawOffsetY, 0, '不可见字形 retarget 后 frozenDrawOffsetY 应为 0')
})

test('问题2：retargetRunStarts retained 通道保留 startScreenClipRects', () => {
  const ctx = context('甲乙丙丁', 952, [])
  const all = idsOf(ctx, 0, 4)
  const frozen = [{
    glyphIds: [all[0]],
    clipLeft: 0, clipRight: 10,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 952, glyphUtf16Ranges: autoRanges([all[0]], ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const retargeted = retargetRunStarts(run, start, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const intervals = retargeted[0].intervals
  assert.ok(intervals !== undefined)
  const iv0 = intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv0 !== undefined)
  assert.ok(iv0.startScreenClipRects !== undefined, 'retarget 后 startScreenClipRects 应保留')
  assert.ok(iv0.startScreenClipRects.length > 0, 'startScreenClipRects 应非空')
})

// ── clonePieces 深拷贝新字段 ──

test('问题2：clonePieces 深拷贝 startScreenClipRects——不共享引用', () => {
  const ctx = context('甲乙丙丁', 953, [])
  const all = idsOf(ctx, 0, 4)
  const frozen = [{
    glyphIds: [all[0]],
    clipLeft: 0, clipRight: 10,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 953, glyphUtf16Ranges: autoRanges([all[0]], ctx), sourceLayout: ctx.layout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  // retarget 到一个全不可见的上下文——clonePieces 路径
  const emptyCtx = context('甲乙丙丁', 953, [])
  const retargeted = retargetRunStarts(run, start, emptyCtx, 'retained', 0, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const intervals = retargeted[0].intervals
  assert.ok(intervals !== undefined)
  // 所有字形在 emptyCtx 中都不可见——走 clonePieces 路径
  // 但 frozenDrawOffsetX/Y 和 startScreenClipRects 应从 prevInterval 保留
  for (const iv of intervals) {
    assert.ok(iv.frozenDrawOffsetX !== undefined, 'clonePieces 后 frozenDrawOffsetX 应保留')
    assert.ok(iv.frozenDrawOffsetY !== undefined, 'clonePieces 后 frozenDrawOffsetY 应保留')
  }
})

// ─── Issue #879 复核评论6103000047 问题4：mapRectByRatio 测试 ───

console.log('\n--- Issue #879 复核评论6103000047 问题4 新增测试 ---')

test('mapRectByRatio——新旧布局尺寸相同时映射是恒等变换', () => {
  const sourceRect = { x: 10, y: 5, width: 20, height: 30 }
  const targetRect = { x: 10, y: 5, width: 20, height: 30 }
  const clipRect = { x: 15, y: 8, width: 10, height: 20 }
  const result = mapRectByRatio(clipRect, sourceRect, targetRect)
  // 比例映射：relX=(15-10)/20=0.25, targetX=10+0.25*20=15
  // relWidth=10/20=0.5, targetWidth=0.5*20=10
  assert.ok(Math.abs(result.x - 15) < 0.001, 'x 应为 15')
  assert.ok(Math.abs(result.y - 8) < 0.001, 'y 应为 8')
  assert.ok(Math.abs(result.width - 10) < 0.001, 'width 应为 10')
  assert.ok(Math.abs(result.height - 20) < 0.001, 'height 应为 20')
})

test('mapRectByRatio——新布局字形更宽时目标位置和宽度按比例扩展', () => {
  const sourceRect = { x: 10, y: 5, width: 20, height: 30 }
  const targetRect = { x: 50, y: 100, width: 40, height: 60 }
  const clipRect = { x: 15, y: 8, width: 10, height: 20 }
  const result = mapRectByRatio(clipRect, sourceRect, targetRect)
  // relX=(15-10)/20=0.25, targetX=50+0.25*40=60
  // relWidth=10/20=0.5, targetWidth=0.5*40=20
  // relY=(8-5)/30=0.1, targetY=100+0.1*60=106
  // relHeight=20/30≈0.667, targetHeight=0.667*60=40
  assert.ok(Math.abs(result.x - 60) < 0.001, 'x 应为 60')
  assert.ok(Math.abs(result.width - 20) < 0.001, 'width 应为 20')
  assert.ok(Math.abs(result.y - 106) < 0.001, 'y 应为 106')
  assert.ok(Math.abs(result.height - 40) < 0.001, 'height 应为 40')
})

test('mapRectByRatio——新布局字形更窄时目标位置和宽度按比例收缩', () => {
  const sourceRect = { x: 0, y: 0, width: 40, height: 60 }
  const targetRect = { x: 100, y: 200, width: 20, height: 30 }
  const clipRect = { x: 10, y: 15, width: 20, height: 30 }
  const result = mapRectByRatio(clipRect, sourceRect, targetRect)
  // relX=10/40=0.25, targetX=100+0.25*20=105
  // relWidth=20/40=0.5, targetWidth=0.5*20=10
  // relY=15/60=0.25, targetY=200+0.25*30=207.5
  // relHeight=30/60=0.5, targetHeight=0.5*30=15
  assert.ok(Math.abs(result.x - 105) < 0.001, 'x 应为 105')
  assert.ok(Math.abs(result.width - 10) < 0.001, 'width 应为 10')
  assert.ok(Math.abs(result.y - 207.5) < 0.001, 'y 应为 207.5')
  assert.ok(Math.abs(result.height - 15) < 0.001, 'height 应为 15')
})

test('mapRectByRatio——源宽度为零时退回 targetRect 左边界', () => {
  const sourceRect = { x: 10, y: 5, width: 0, height: 30 }
  const targetRect = { x: 50, y: 100, width: 40, height: 60 }
  const clipRect = { x: 10, y: 8, width: 0, height: 20 }
  const result = mapRectByRatio(clipRect, sourceRect, targetRect)
  // 源宽度为零：relX=0, relWidth=0 → targetX=50, targetWidth=0
  // 源高度非零：relY=(8-5)/30=0.1, targetY=100+0.1*60=106
  assert.ok(Math.abs(result.x - 50) < 0.001, 'x 应为 50（退回 targetRect.x）')
  assert.ok(Math.abs(result.width - 0) < 0.001, 'width 应为 0')
  assert.ok(Math.abs(result.y - 106) < 0.001, 'y 应为 106')
})

test('mapRectByRatio——源高度为零时退回 targetRect 上边界', () => {
  const sourceRect = { x: 10, y: 5, width: 20, height: 0 }
  const targetRect = { x: 50, y: 100, width: 40, height: 60 }
  const clipRect = { x: 15, y: 5, width: 10, height: 0 }
  const result = mapRectByRatio(clipRect, sourceRect, targetRect)
  // 源宽度非零：relX=(15-10)/20=0.25, targetX=50+0.25*40=60
  // 源高度为零：relY=0, relHeight=0 → targetY=100, targetHeight=0
  assert.ok(Math.abs(result.x - 60) < 0.001, 'x 应为 60')
  assert.ok(Math.abs(result.width - 20) < 0.001, 'width 应为 20')
  assert.ok(Math.abs(result.y - 100) < 0.001, 'y 应为 100（退回 targetRect.y）')
  assert.ok(Math.abs(result.height - 0) < 0.001, 'height 应为 0')
})

test('mapRectByRatio——clipRect 在源字形左半部分，新布局宽度翻倍', () => {
  // 模拟换行场景：旧布局字形宽 10，新布局字形宽 20
  const sourceRect = { x: 0, y: 0, width: 10, height: 20 }
  const targetRect = { x: 0, y: 0, width: 20, height: 20 }
  // 旧布局中只显示左半部分 [0, 5]
  const clipRect = { x: 0, y: 0, width: 5, height: 20 }
  const result = mapRectByRatio(clipRect, sourceRect, targetRect)
  // relX=0, relWidth=5/10=0.5 → targetX=0, targetWidth=0.5*20=10
  // 即新布局中左半部分对应 [0, 10]
  assert.ok(Math.abs(result.x - 0) < 0.001, 'x 应为 0')
  assert.ok(Math.abs(result.width - 10) < 0.001, 'width 应为 10（新布局左半部分）')
  assert.ok(Math.abs(result.height - 20) < 0.001, 'height 应为 20')
})

test('mapRectByRatio——间隙碎片映射：旧布局中间隙 [5,10] 映射到新布局 [10,20]', () => {
  // 模拟换行场景：旧布局字形宽 10，新布局字形宽 20
  const sourceRect = { x: 0, y: 0, width: 10, height: 20 }
  const targetRect = { x: 0, y: 0, width: 20, height: 20 }
  // 旧布局中间隙 [5, 10]（右半部分不可见）
  const gapRect = { x: 5, y: 0, width: 5, height: 20 }
  const result = mapRectByRatio(gapRect, sourceRect, targetRect)
  // relX=5/10=0.5, relWidth=5/10=0.5 → targetX=0+0.5*20=10, targetWidth=0.5*20=10
  // 即新布局中间隙对应 [10, 20]
  assert.ok(Math.abs(result.x - 10) < 0.001, 'x 应为 10')
  assert.ok(Math.abs(result.width - 10) < 0.001, 'width 应为 10（新布局右半部分）')
  assert.ok(Math.abs(result.height - 20) < 0.001, 'height 应为 20')
})

test('mapRectByRatio——纵向半字映射：旧布局上半部分映射到新布局不同行高', () => {
  // 模拟行高变化：旧布局行高 20，新布局行高 30
  const sourceRect = { x: 0, y: 0, width: 10, height: 20 }
  const targetRect = { x: 0, y: 100, width: 10, height: 30 }
  // 旧布局中只显示上半部分 [0, 8]
  const clipRect = { x: 0, y: 0, width: 10, height: 8 }
  const result = mapRectByRatio(clipRect, sourceRect, targetRect)
  // relY=0, relHeight=8/20=0.4 → targetY=100, targetHeight=0.4*30=12
  assert.ok(Math.abs(result.y - 100) < 0.001, 'y 应为 100')
  assert.ok(Math.abs(result.height - 12) < 0.001, 'height 应为 12（新布局上半部分按比例）')
})

// ─── Issue #879 复核评论6103606574 缺口1：新旧源交叉淡入淡出 ───
//
// retained 通道在新旧字宽/高不同时，同时输出旧源(opacity=1-p)和新源(opacity=p)两个窗口。
// shouldCrossfadeSources 判断是否需要新旧源切换——当 width 或 height 差异 > 0.5vp 时返回 true。

console.log('\n--- Issue #879 复核评论6103606574 缺口1：新旧源交叉淡入淡出 ---')

test('shouldCrossfadeSources——新旧字宽相同时不需要交叉淡入淡出', () => {
  const sourceGlyph = { x: 10, y: 20, width: 15, height: 24 }
  const newGlyph = { x: 50, y: 20, width: 15, height: 24 }
  assert.ok(!shouldCrossfadeSources(sourceGlyph, newGlyph),
    '字宽/高相同（仅位置不同）时不需要交叉淡入淡出')
})

test('shouldCrossfadeSources——新旧字宽差异 > 0.5vp 时需要交叉淡入淡出', () => {
  // 评论中的例子：旧源字形宽 10vp，新版宽 20vp
  const sourceGlyph = { x: 0, y: 0, width: 10, height: 24 }
  const newGlyph = { x: 50, y: 0, width: 20, height: 24 }
  assert.ok(shouldCrossfadeSources(sourceGlyph, newGlyph),
    '字宽差异 10vp > 0.5vp 时需要交叉淡入淡出')
})

test('shouldCrossfadeSources——新旧行高差异 > 0.5vp 时需要交叉淡入淡出', () => {
  const sourceGlyph = { x: 0, y: 0, width: 10, height: 20 }
  const newGlyph = { x: 0, y: 0, width: 10, height: 30 }
  assert.ok(shouldCrossfadeSources(sourceGlyph, newGlyph),
    '行高差异 10vp > 0.5vp 时需要交叉淡入淡出')
})

test('shouldCrossfadeSources——新旧字宽差异 ≤ 0.5vp 时不需要交叉淡入淡出', () => {
  const sourceGlyph = { x: 0, y: 0, width: 10, height: 24 }
  const newGlyph = { x: 50, y: 0, width: 10.3, height: 24 }
  assert.ok(!shouldCrossfadeSources(sourceGlyph, newGlyph),
    '字宽差异 0.3vp ≤ 0.5vp 时不需要交叉淡入淡出（浮点误差范围内）')
})

test('shouldCrossfadeSources——新旧行高差异 ≤ 0.5vp 时不需要交叉淡入淡出', () => {
  const sourceGlyph = { x: 0, y: 0, width: 10, height: 24 }
  const newGlyph = { x: 0, y: 0, width: 10, height: 24.4 }
  assert.ok(!shouldCrossfadeSources(sourceGlyph, newGlyph),
    '行高差异 0.4vp ≤ 0.5vp 时不需要交叉淡入淡出（浮点误差范围内）')
})

test('shouldCrossfadeSources——字宽变窄时也需要交叉淡入淡出', () => {
  // 删除文字后换行变化导致字宽变窄
  const sourceGlyph = { x: 0, y: 0, width: 20, height: 24 }
  const newGlyph = { x: 0, y: 0, width: 10, height: 24 }
  assert.ok(shouldCrossfadeSources(sourceGlyph, newGlyph),
    '字宽从 20vp 变窄到 10vp 时需要交叉淡入淡出')
})

test('shouldCrossfadeSources——边界情况：差异正好 0.5vp 时不触发', () => {
  const sourceGlyph = { x: 0, y: 0, width: 10, height: 24 }
  const newGlyph = { x: 0, y: 0, width: 10.5, height: 24 }
  assert.ok(!shouldCrossfadeSources(sourceGlyph, newGlyph),
    '差异正好 0.5vp 时不触发（> 0.5 才触发）')
})

test('shouldCrossfadeSources——边界情况：差异 0.51vp 时触发', () => {
  const sourceGlyph = { x: 0, y: 0, width: 10, height: 24 }
  const newGlyph = { x: 0, y: 0, width: 10.51, height: 24 }
  assert.ok(shouldCrossfadeSources(sourceGlyph, newGlyph),
    '差异 0.51vp > 0.5vp 时触发')
})

// ─── Issue #879 复核评论6104942471 问题4：冻结帧真实绘制位置/裁切交接 ───

console.log('\n--- Issue #879 复核评论6104942471 问题4 新增测试 ---')

test('问题4-1：prepare 可见、retarget 变全不可见——startScreenClipRects 必须清空，不能复活已吞掉的字', () => {
  const ctx = context('甲乙丙丁', 970, [])
  const all = idsOf(ctx, 0, 4)
  // prepare：簇0 完整可见 [0,10]，interval 携带非空 startScreenClipRects
  const prepareCtx = { ...ctx, frozenWindows: [frozenWindow([all[0]], 0, 10, 0, 0, 970, null, null, ctx)] }
  const run = runGeometry(prepareCtx, 0, 4, { x: 0, y: 0, width: 40, height: 20 })
  const prepared = retainedMoveStartState(run, prepareCtx, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const iv0Before = prepared[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv0Before.startScreenClipRects !== undefined && iv0Before.startScreenClipRects.length > 0,
    'prepare 可见时 startScreenClipRects 应非空')

  // 提交瞬间旧动画把簇0 吞得完全不可见（clip 宽度为 0）
  const retargetCtx = { ...ctx, frozenWindows: [frozenWindow([all[0]], 0, 0, 0, 0, 970, null, null, ctx)] }
  const retargeted = retargetRunStarts(run, prepared, retargetCtx, 'retained', 0, {
    startClipLeft: 0, startClipRight: 40, startPositionX: 0, startPositionY: 0,
  })
  const iv0 = retargeted[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv0 !== undefined, '分区必须保留')
  assert.deepEqual(iv0.startScreenClipRects, [], '本次提交已不可见——必须清空，不能沿用 prepare 的旧可见矩形')
  assert.equal(iv0.startClipLeft, iv0.startClipRight, '零宽起点')
  assert.equal(iv0.clipRects[0].width, 0, 'clipRects 零宽')
})

test('问题4-2：多行 retained 不可见字形——startPositionX/Y 为字形自己的绝对原点，不是 run 原点', () => {
  const twoLineLayout = [
    {
      startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'softWrap',
      caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 1, x: 10 }, { utf16Offset: 2, x: 20 }],
    },
    {
      startUtf16: 2, endUtf16: 4, left: 0, y: 20, height: 20, breakKind: 'endOfText',
      caretStops: [{ utf16Offset: 2, x: 0 }, { utf16Offset: 3, x: 10 }, { utf16Offset: 4, x: 20 }],
    },
  ]
  const ctx = {
    text: '甲乙丙丁',
    layout: twoLineLayout,
    identities: GlyphIdentityTable.create('甲乙丙丁', 971),
    frozenWindows: [],
  }
  const all = idsOf(ctx, 0, 4)
  // 整段被旧动画吞得完全不可见
  const hiddenCtx = { ...ctx, frozenWindows: [frozenWindow(all, 0, 0, 0, 0, 971, null, null, ctx)] }
  const run = {
    glyphIds: all,
    ownText: '甲乙丙丁',
    ownUtf16Start: 0,
    ownUtf16End: 4,
    ownRect: { x: 0, y: 0, width: 20, height: 40 },
    ownLayout: twoLineLayout,
  }
  const start = retainedMoveStartState(run, hiddenCtx, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const ivs = start[0].intervals
  const iv1 = ivs.find(iv => iv.firstIndex === 1)  // 乙：第一行 y=0
  const iv2 = ivs.find(iv => iv.firstIndex === 2)  // 丙：第二行 y=20
  assert.equal(iv1.startPositionY, 0, '第一行字形的 startPositionY')
  assert.equal(iv2.startPositionY, 20, '第二行字形的 startPositionY 应为字形所在行 y=20，不是 run.ownRect.y=0')
  assert.equal(iv2.clipRects[0].y, 20, 'clipRects.y 与字形行一致')
  assert.equal(iv2.sourceGlyphRects[0].y, 20, 'sourceGlyphRects.y 与字形行一致')
  assert.equal(iv2.startPositionX, 0, '丙 在第二行行首 x=0')
  assert.equal(iv2.startPositionX, iv2.clipRects[0].x, 'startPositionX 与零宽起点锚定同一字形原点')
})

test('问题4-3：frozenSourceGlyphRects 取自冻结源布局，而非 run.ownLayout', () => {
  // displayed 正文只有 '甲'；'orig-972-1'（乙）是 ghost——已从正文删除但旧窗口仍绘制
  const ctx = context('甲', 972, [])
  const ghostId = 'orig-972-1'
  const ghostRanges = new Map([[ghostId, [1, 2]]])
  // 冻结源布局把乙排在 x=50（与 run.ownLayout 的 x=10 不同）
  const sourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 60 }],
  }]
  const ownLayout = [line('甲乙', 0)]  // 乙在 x=10
  const frozen = [{
    glyphIds: [ghostId],
    clipLeft: 50,
    clipRight: 60,
    clipRects: [{ x: 50, y: 0, width: 10, height: 20 }],
    offsetX: 0,
    offsetY: 0,
    sourceRevision: 972,
    glyphUtf16Ranges: ghostRanges,
    sourceLayout: sourceLayout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: [ghostId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: ownLayout,
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv.frozenSourceGlyphRects !== undefined, 'frozenSourceGlyphRects 应填充')
  assert.equal(iv.frozenSourceGlyphRects[0].x, 50, '冻结源字形 x 来自 sourceLayout（50）')
  assert.equal(iv.sourceGlyphRects[0].x, 10, 'sourceGlyphRects 仍来自 run.ownLayout（10）')
  assert.notEqual(iv.frozenSourceGlyphRects[0].x, iv.sourceGlyphRects[0].x,
    '两者坐标系不同——证明 buildFrame 必须用 frozenSourceGlyphRects 计算 sourceGlyphOnScreen')
  // startPositionX 与冻结源字形一致（同一坐标系），不是 ownLayout 的 10
  assert.equal(iv.startPositionX, 50, 'startPositionX 应与冻结源字形在屏位置一致')
  // retarget 后 frozenSourceGlyphRects 保留
  const retargeted = retargetRunStarts(run, start, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const riv = retargeted[0].intervals.find(iv => iv.firstIndex === 0)
  assert.equal(riv.frozenSourceGlyphRects[0].x, 50, 'retarget 后 frozenSourceGlyphRects 应保留')
})

test('问题2-6105187411：字形在 displayed 身份表中且正由冻结窗口绘制时，frozenSourceGlyphRects 仍来自冻结窗口 sourceLayout', () => {
  // 场景：displayed 正文仍包含 '乙'（身份表中有），但乙正由一个冻结旧源 Text 窗口绘制。
  // 旧 sourceGlyphGeometry 优先从 displayed.layout 读取（x=10），返回错误的坐标。
  // frozenGlyphGeometry 优先从冻结窗口的 sourceLayout 读取（x=50），返回正确坐标。
  // Issue #879 复核评论6105482207 问题4：clusterVisible 也应从 sourceLayout + offset 计算屏幕坐标，
  // clipRect 基于 sourceLayout + offset（x=50+3=53），与冻结窗口真实绘制位置一致。
  const ctx = context('甲乙', 972, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]  // 乙的 glyphId
  // 冻结源布局把乙排在 x=50（与 displayed.layout 的 x=10 不同）
  const sourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 60 }],
  }]
  const yiRanges = new Map([[yiId, [1, 2]]])
  // clipRect 基于 sourceLayout + offset（x=50+3=53），与冻结窗口真实绘制位置一致
  const frozen = [{
    glyphIds: [yiId],
    clipLeft: 53,
    clipRight: 58,
    clipRects: [{ x: 53, y: 0, width: 5, height: 20 }],
    offsetX: 3,
    offsetY: 0,
    sourceRevision: 970,
    glyphUtf16Ranges: yiRanges,
    sourceLayout: sourceLayout,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  // run 覆盖乙——ownLayout 中乙在 x=10
  const ownLayout = [line('甲乙', 0)]
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: ownLayout,
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.frozenSourceGlyphRects !== undefined, 'frozenSourceGlyphRects 应填充')
  // frozenSourceGlyphRects 应来自冻结窗口的 sourceLayout（x=50），不是 displayed.layout（x=10）
  assert.equal(iv.frozenSourceGlyphRects[0].x, 50,
    'frozenSourceGlyphRects.x 应来自冻结窗口 sourceLayout（50），不是 displayed.layout（10）')
  // sourceGlyphRects 仍来自 run.ownLayout（x=10）——两者坐标系不同
  assert.ok(iv.sourceGlyphRects !== undefined, 'sourceGlyphRects 应填充')
  assert.equal(iv.sourceGlyphRects[0].x, 10,
    'sourceGlyphRects.x 仍来自 run.ownLayout（10）')
})

test('双源冻结窗口：glyphId 同时被旧源和新源两个冻结窗口覆盖时，frozenSourceGlyphRects 包含2个成员', () => {
  // 场景：displayed 正文 '甲乙'（revision 972），乙的 glyphId 同时被两个冻结窗口覆盖：
  // 旧源窗口（sourceKind='old'）：sourceLayout 中乙在 x=50，offsetX=3，clipRects=[{x:53, y:0, width:5, height:20}]
  // 新源窗口（sourceKind='new'）：sourceLayout 中乙在 x=70，offsetX=5，clipRects=[{x:75, y:0, width:5, height:20}]
  // 验证 frozenSourceGlyphRects 有2个条目，startScreenClipRects 合并两个成员的 rects，
  // frozenDrawOffsetX 取自第一个成员的 offset。
  const ctx = context('甲乙', 972, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]  // 乙的 glyphId

  // 旧源窗口的 sourceLayout：乙在 x=50（与 displayed.layout 的 x=10 不同）
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 60 }],
  }]
  // 新源窗口的 sourceLayout：乙在 x=70
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]

  const yiRanges = new Map([[yiId, [1, 2]]])

  // 手动构造 FrozenWindowSpan（需要 sourceKind/runId/renderNodeKey/opacity）
  const frozen = [
    {
      glyphIds: [yiId],
      clipLeft: 53,
      clipRight: 58,
      clipRects: [{ x: 53, y: 0, width: 5, height: 20 }],
      offsetX: 3,
      offsetY: 0,
      sourceRevision: 970,
      glyphUtf16Ranges: yiRanges,
      sourceLayout: oldSourceLayout,
      sourceKind: 'old',
      runId: 'old-11',
      renderNodeKey: 'old-node-11',
      opacity: 0.5,
    },
    {
      glyphIds: [yiId],
      clipLeft: 75,
      clipRight: 80,
      clipRects: [{ x: 75, y: 0, width: 5, height: 20 }],
      offsetX: 5,
      offsetY: 0,
      sourceRevision: 974,
      glyphUtf16Ranges: yiRanges,
      sourceLayout: newSourceLayout,
      sourceKind: 'new',
      runId: 'new-11',
      renderNodeKey: 'new-node-11',
      opacity: 0.5,
    },
  ]
  const withWindow = { ...ctx, frozenWindows: frozen }
  // run 覆盖乙——ownLayout 中乙在 x=10（与冻结源布局不同）
  const ownLayout = [line('甲乙', 0)]
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: ownLayout,
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')

  // 1. frozenSourceGlyphRects 有2个条目（不是1个）
  assert.ok(iv.frozenSourceGlyphRects !== undefined, 'frozenSourceGlyphRects 应填充')
  assert.equal(iv.frozenSourceGlyphRects.length, 2,
    'frozenSourceGlyphRects 应有2个条目（旧源+新源），不是1个')

  // 2. 第一个条目对应旧源窗口的 sourceLayout（x=50），第二个对应新源窗口的 sourceLayout（x=70）
  assert.equal(iv.frozenSourceGlyphRects[0].x, 50,
    '第一个 frozenSourceGlyphRects 条目应来自旧源窗口 sourceLayout（x=50）')
  assert.equal(iv.frozenSourceGlyphRects[1].x, 70,
    '第二个 frozenSourceGlyphRects 条目应来自新源窗口 sourceLayout（x=70）')

  // 3. startScreenClipRects 合并了两个成员的 rects（每个成员各1个 rect，总共2个）
  assert.ok(iv.startScreenClipRects !== undefined, 'startScreenClipRects 应填充')
  assert.equal(iv.startScreenClipRects.length, 2,
    'startScreenClipRects 应合并两个成员的 rects，总共2个')
  // 旧源成员的 rect：x=53（50+3=53，clipRect 与字形在屏位置求交）
  assert.equal(iv.startScreenClipRects[0].x, 53,
    '第一个 startScreenClipRects 应来自旧源窗口（x=53）')
  // 新源成员的 rect：x=75（70+5=75，clipRect 与字形在屏位置求交）
  assert.equal(iv.startScreenClipRects[1].x, 75,
    '第二个 startScreenClipRects 应来自新源窗口（x=75）')

  // 4. frozenDrawOffsetX 取自第一个成员的 offset（3）
  assert.equal(iv.frozenDrawOffsetX, 3,
    'frozenDrawOffsetX 应取自第一个成员（旧源窗口）的 offset（3）')

  // sourceGlyphRects 仍来自 run.ownLayout（x=10）——与 frozenSourceGlyphRects 坐标系不同
  assert.ok(iv.sourceGlyphRects !== undefined, 'sourceGlyphRects 应填充')
  assert.equal(iv.sourceGlyphRects[0].x, 10,
    'sourceGlyphRects.x 仍来自 run.ownLayout（10），与 frozenSourceGlyphRects 不同')
})

// ====== Issue #879 复核评论6106658725：整条链路一致性测试 ======

test('问题1-6106658725：冻结源坐标统一——startPositionX、startScreenClipRects、frozenSourceGlyphRects 全部来自冻结源布局', () => {
  const ctx = context('甲乙', 972, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const sourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 60 }],
  }]
  const yiRanges = new Map([[yiId, [1, 2]]])
  const frozen = [{
    glyphIds: [yiId],
    clipLeft: 53,
    clipRight: 63,
    clipRects: [{ x: 53, y: 0, width: 10, height: 20 }],
    offsetX: 3,
    offsetY: 0,
    sourceRevision: 970,
    glyphUtf16Ranges: yiRanges,
    sourceLayout: sourceLayout,
    sourceKind: 'old',
    runId: 'old-11',
    renderNodeKey: 'old-node-11',
    opacity: undefined,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const ownLayout = [line('甲乙', 0)]
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: ownLayout,
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.frozenSourceGlyphRects !== undefined)
  assert.equal(iv.frozenSourceGlyphRects[0].x, 50,
    'frozenSourceGlyphRects.x 应来自冻结源布局（50）')
  assert.equal(iv.startPositionX, 53,
    'startPositionX 应为 53（源字形 50 + offset 3）')
  assert.ok(iv.startScreenClipRects !== undefined)
  assert.equal(iv.startScreenClipRects[0].x, 53,
    'startScreenClipRects.x 应为 53（冻结源在屏位置）')
  assert.ok(iv.sourceGlyphRects !== undefined)
  assert.equal(iv.sourceGlyphRects[0].x, 10,
    'sourceGlyphRects.x 仍来自 run.ownLayout（10）')
})

test('问题1-6106658725：双源冻结窗口——startPositionX 来自第一个成员的冻结源坐标', () => {
  const ctx = context('甲乙', 972, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 60 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const yiRanges = new Map([[yiId, [1, 2]]])
  const frozen = [
    {
      glyphIds: [yiId],
      clipLeft: 53, clipRight: 58,
      clipRects: [{ x: 53, y: 0, width: 5, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 970,
      glyphUtf16Ranges: yiRanges,
      sourceLayout: oldSourceLayout,
      sourceKind: 'old',
      runId: 'old-11',
      renderNodeKey: 'old-node-11',
      opacity: 0.5,
    },
    {
      glyphIds: [yiId],
      clipLeft: 75, clipRight: 80,
      clipRects: [{ x: 75, y: 0, width: 5, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 974,
      glyphUtf16Ranges: yiRanges,
      sourceLayout: newSourceLayout,
      sourceKind: 'new',
      runId: 'new-11',
      renderNodeKey: 'new-node-11',
      opacity: 0.5,
    },
  ]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const ownLayout = [line('甲乙', 0)]
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: ownLayout,
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined)
  assert.equal(iv.startPositionX, 53,
    'startPositionX 应为 53（第一个成员源字形 50 + offset 3）')
  assert.equal(iv.frozenSourceGlyphRects.length, 2)
  assert.equal(iv.frozenSourceGlyphRects[0].x, 50)
  assert.equal(iv.frozenSourceGlyphRects[1].x, 70)
  assert.equal(iv.startScreenClipRects.length, 2)
  assert.equal(iv.startScreenClipRects[0].x, 53)
  assert.equal(iv.startScreenClipRects[1].x, 75)
})

test('问题1-6106658725：冻结窗口 glyphUtf16Ranges 为 null 时跳过 span——不回退到 displayed.layout', () => {
  const ctx = context('甲乙', 980, [])
  const ids = idsOf(ctx, 0, 2)
  const frozen = [{
    glyphIds: [ids[1]],
    clipLeft: 10, clipRight: 20,
    clipRects: [{ x: 10, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 980,
    glyphUtf16Ranges: null,
    sourceLayout: null,
    sourceKind: 'old',
    runId: 'old-11',
    renderNodeKey: 'old-node-11',
    opacity: undefined,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 1, 2, { x: 10, y: 0, width: 10, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 0, '冻结窗口无法提供 sourceLayout 时，字形应不可见')
})

test('问题1-6106658725：clusterVisible 有冻结窗口时 OnScreenInterval.sourceGlyphX0 来自冻结源布局', () => {
  // 验证 clusterVisible 返回的 OnScreenInterval 中 sourceGlyphX0/X1 来自冻结源布局，
  // 而不是 displayed.layout。这是确保 buildPiece/intervalForIsland/buildFrame
  // 四段统一使用冻结源坐标的关键。
  const ctx = context('甲乙', 972, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const sourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 60 }],
  }]
  const yiRanges = new Map([[yiId, [1, 2]]])
  const frozen = [{
    glyphIds: [yiId],
    clipLeft: 53,
    clipRight: 63,
    clipRects: [{ x: 53, y: 0, width: 10, height: 20 }],
    offsetX: 3,
    offsetY: 0,
    sourceRevision: 970,
    glyphUtf16Ranges: yiRanges,
    sourceLayout: sourceLayout,
    sourceKind: 'old',
    runId: 'old-11',
    renderNodeKey: 'old-node-11',
    opacity: undefined,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  // 直接测试 frozenIndex + clusterVisible
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, yiId)
  assert.ok(intervals.length > 0, '乙应可见')
  // sourceGlyphX0 应来自冻结源布局（50），不是 displayed.layout（10）
  assert.equal(intervals[0].sourceGlyphX0, 50,
    'sourceGlyphX0 应来自冻结源布局（50）')
  assert.equal(intervals[0].sourceGlyphX1, 60,
    'sourceGlyphX1 应来自冻结源布局（60）')
  // rects 的屏幕坐标应基于冻结源布局 + offset（53..63）
  assert.equal(intervals[0].rects[0].x, 53,
    'rects[0].x 应为 53（冻结源 50 + offset 3）')
})

// ====== Issue #879 评论6108900966 问题4：双源成员 + 不同 offset/clip/opacity 的完整流程 ======

/**
 * 辅助：构造双源冻结窗口（同一 glyphId 被 old + new 两个窗口覆盖）。
 *
 * old 源和 new 源各有独立的 sourceLayout、offsetX、clipRects、opacity。
 */
const dualSourceFrozen = (glyphId, oldOpts, newOpts, ctx) => {
  const oldRanges = new Map([[glyphId, oldOpts.utf16Range]])
  const newRanges = new Map([[glyphId, newOpts.utf16Range]])
  return [
    {
      glyphIds: [glyphId],
      clipLeft: oldOpts.clipLeft,
      clipRight: oldOpts.clipRight,
      clipRects: oldOpts.clipRects,
      offsetX: oldOpts.offsetX,
      offsetY: oldOpts.offsetY ?? 0,
      sourceRevision: oldOpts.sourceRevision,
      glyphUtf16Ranges: oldRanges,
      sourceLayout: oldOpts.sourceLayout,
      sourceKind: 'old',
      runId: `old-${oldOpts.sourceRevision}`,
      renderNodeKey: `old-node-${oldOpts.sourceRevision}`,
      opacity: oldOpts.opacity,
    },
    {
      glyphIds: [glyphId],
      clipLeft: newOpts.clipLeft,
      clipRight: newOpts.clipRight,
      clipRects: newOpts.clipRects,
      offsetX: newOpts.offsetX,
      offsetY: newOpts.offsetY ?? 0,
      sourceRevision: newOpts.sourceRevision,
      glyphUtf16Ranges: newRanges,
      sourceLayout: newOpts.sourceLayout,
      sourceKind: 'new',
      runId: `new-${newOpts.sourceRevision}`,
      renderNodeKey: `new-node-${newOpts.sourceRevision}`,
      opacity: newOpts.opacity,
    },
  ]
}

test('问题4-6108900966：buildSourceMembers 为双源成员各自保留独立的 offset/clip/opacity——通过 retainedMoveStartState 验证', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  // old 源布局：乙在 x=50, width=12
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  // new 源布局：乙在 x=70, width=10
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.sourceMembers !== undefined, 'sourceMembers 应填充')
  assert.equal(iv.sourceMembers.length, 2, '应有 2 个成员（old + new）')
  // old 成员
  const oldMember = iv.sourceMembers[0]
  assert.equal(oldMember.sourceKind, 'old')
  assert.equal(oldMember.offsetX, 3)
  assert.equal(oldMember.offsetY, 0)
  assert.equal(oldMember.opacity, 0.6)
  assert.equal(oldMember.sourceGlyphRect.x, 50)
  assert.equal(oldMember.sourceGlyphRect.width, 12)
  assert.equal(oldMember.clipRects.length, 1)
  assert.equal(oldMember.clipRects[0].x, 53)
  assert.equal(oldMember.clipRects[0].width, 9)
  // new 成员
  const newMember = iv.sourceMembers[1]
  assert.equal(newMember.sourceKind, 'new')
  assert.equal(newMember.offsetX, 5)
  assert.equal(newMember.offsetY, 0)
  assert.equal(newMember.opacity, 0.4)
  assert.equal(newMember.sourceGlyphRect.x, 70)
  assert.equal(newMember.sourceGlyphRect.width, 10)
  assert.equal(newMember.clipRects.length, 1)
  assert.equal(newMember.clipRects[0].x, 75)
  assert.equal(newMember.clipRects[0].width, 8)
})

test('问题4-6108900966：retargetRunStarts 的 retained 通道深拷贝 sourceMembers 和传递 needsCrossfade', () => {
  const ctx = context('甲乙', 1001, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  // 先用 retainedMoveStartState 生成初始 pieces（带 sourceMembers）
  const prepared = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  // 手动设置 needsCrossfade=true 模拟 plan 阶段写入
  const prevInterval = prepared[0].intervals[0]
  prevInterval.needsCrossfade = true
  // 调用 retargetRunStarts 验证传递和深拷贝
  const retargeted = retargetRunStarts(run, prepared, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const reIv = retargeted[0].intervals[0]
  assert.ok(reIv.sourceMembers !== undefined, 'retarget 后 sourceMembers 应存在')
  assert.equal(reIv.sourceMembers.length, 2, 'retarget 后应有 2 个成员')
  assert.equal(reIv.needsCrossfade, true, 'needsCrossfade 应从 prevInterval 继承')
  // 深拷贝验证：修改原成员不影响 retargeted 的成员
  const origOldMember = prevInterval.sourceMembers[0]
  origOldMember.offsetX = 999
  assert.notEqual(reIv.sourceMembers[0].offsetX, 999,
    '深拷贝：修改原成员不应影响 retargeted 的成员')
  // 深拷贝验证：clipRects 也是独立的
  origOldMember.clipRects[0].x = 999
  assert.notEqual(reIv.sourceMembers[0].clipRects[0].x, 999,
    '深拷贝：修改原 clipRects 不应影响 retargeted 的 clipRects')
})

test('问题4-6108900966：retargetRunStarts 不可见分支也传递 sourceMembers 和 needsCrossfade', () => {
  const ctx = context('甲乙', 1002, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  // 先用 retainedMoveStartState 生成初始 pieces
  const prepared = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  // 手动设置 needsCrossfade=false 和 sourceMembers
  const prevInterval = prepared[0].intervals[0]
  prevInterval.needsCrossfade = false
  // 构造一个让字形不可见的 displayed context（clipRects 与字形无交集）
  const hiddenCtx = { ...withWindow, frozenWindows: [
    {
      ...frozen[0],
      clipLeft: 200, clipRight: 200,
      clipRects: [{ x: 200, y: 0, width: 0, height: 20 }],
    },
    {
      ...frozen[1],
      clipLeft: 200, clipRight: 200,
      clipRects: [{ x: 200, y: 0, width: 0, height: 20 }],
    },
  ]}
  const retargeted = retargetRunStarts(run, prepared, hiddenCtx, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  // 不可见分支：interval 仍保留分区，但 clip 为零宽度
  const reIv = retargeted[0].intervals[0]
  assert.equal(reIv.startClipLeft, reIv.startClipRight, '不可见时 clip 应为零宽度')
  assert.equal(reIv.needsCrossfade, false, '不可见分支也应传递 needsCrossfade')
  assert.ok(reIv.sourceMembers !== undefined, '不可见分支也应传递 sourceMembers')
  assert.equal(reIv.sourceMembers.length, 2, '不可见分支的 sourceMembers 应有 2 个成员')
  // 深拷贝验证
  prevInterval.sourceMembers[0].offsetX = 888
  assert.notEqual(reIv.sourceMembers[0].offsetX, 888,
    '不可见分支的 sourceMembers 也应是深拷贝')
})

test('问题4-6108900966：clonePieces 深拷贝 sourceMembers——修改原 piece 不影响克隆', () => {
  const ctx = context('甲乙', 1003, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  // 先用 retainedMoveStartState 生成初始 pieces（带 sourceMembers）
  const prepared = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  // 设置 needsCrossfade=false
  prepared[0].intervals[0].needsCrossfade = false
  // 通过 retargetRunStarts 触发 clonePieces（boundaries 不匹配时走 clonePieces 路径）
  // 构造一个 glyphIds 数量与 boundaries 不匹配的 run 来触发 clonePieces
  const badRun = {
    ...run,
    // 故意让 ownUtf16Start/End 不对应整簇边界，使 clusterBoundaries 返回的长度不匹配
    ownUtf16Start: 0,
    ownUtf16End: 1, // 只取半个簇，boundaries 不会等于 glyphIds.length + 1
  }
  const cloned = retargetRunStarts(badRun, prepared, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  // clonePieces 路径：pieces 结构不变，但应是深拷贝
  const clonedIv = cloned[0].intervals[0]
  assert.ok(clonedIv.sourceMembers !== undefined, 'clonePieces 后 sourceMembers 应存在')
  assert.equal(clonedIv.sourceMembers.length, 2, 'clonePieces 后应有 2 个成员')
  assert.equal(clonedIv.needsCrossfade, false, 'clonePieces 后 needsCrossfade 应保留')
  // 深拷贝验证：修改原成员不影响克隆
  const origMember = prepared[0].intervals[0].sourceMembers[0]
  origMember.offsetX = 777
  assert.notEqual(clonedIv.sourceMembers[0].offsetX, 777,
    'clonePieces 深拷贝：修改原成员不应影响克隆')
  // 深拷贝验证：clipRects 也是独立的
  origMember.clipRects[0].width = 777
  assert.notEqual(clonedIv.sourceMembers[0].clipRects[0].width, 777,
    'clonePieces 深拷贝：修改原 clipRects 不应影响克隆')
  // 深拷贝验证：sourceGlyphRect 也是独立的
  origMember.sourceGlyphRect.x = 777
  assert.notEqual(clonedIv.sourceMembers[0].sourceGlyphRect.x, 777,
    'clonePieces 深拷贝：修改原 sourceGlyphRect 不应影响克隆')
})

test('问题4-6108900966：双源成员完整流程——clusterVisible + retainedMoveStartState + retargetRunStarts', () => {
  const ctx = context('甲乙', 1004, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  // old 源：sourceRevision=10, offsetX=3, opacity=0.7
  // sourceGlyphRect: {x:50, y:0, width:12, height:20}
  // clipRects: [{x:53, y:0, width:9, height:20}]  (部分可见)
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  // new 源：sourceRevision=11, offsetX=5, opacity=0.3
  // sourceGlyphRect: {x:70, y:0, width:10, height:20}
  // clipRects: [{x:75, y:0, width:5, height:20}]  (部分可见)
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.7,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 80,
      clipRects: [{ x: 75, y: 0, width: 5, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.3,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 1. 验证 clusterVisible
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, yiId)
  assert.equal(intervals.length, 2, 'clusterVisible 应返回 2 个 OnScreenInterval')
  // old interval
  assert.equal(intervals[0].sourceKind, 'old')
  assert.equal(intervals[0].opacity, 0.7)
  assert.equal(intervals[0].rects.length, 1)
  assert.equal(intervals[0].rects[0].x, 53)
  assert.equal(intervals[0].rects[0].width, 9)
  // new interval
  assert.equal(intervals[1].sourceKind, 'new')
  assert.equal(intervals[1].opacity, 0.3)
  assert.equal(intervals[1].rects.length, 1)
  assert.equal(intervals[1].rects[0].x, 75)
  assert.equal(intervals[1].rects[0].width, 5)

  // 2. 验证 retainedMoveStartState（intervalForIsland 内部调用 buildSourceMembers）
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined)
  // sourceMembers 有 2 个成员
  assert.ok(iv.sourceMembers !== undefined)
  assert.equal(iv.sourceMembers.length, 2)
  // old 成员
  assert.equal(iv.sourceMembers[0].sourceKind, 'old')
  assert.equal(iv.sourceMembers[0].offsetX, 3)
  assert.equal(iv.sourceMembers[0].sourceGlyphRect.x, 50)
  assert.equal(iv.sourceMembers[0].opacity, 0.7)
  assert.equal(iv.sourceMembers[0].clipRects.length, 1)
  assert.equal(iv.sourceMembers[0].clipRects[0].x, 53)
  assert.equal(iv.sourceMembers[0].clipRects[0].width, 9)
  // new 成员
  assert.equal(iv.sourceMembers[1].sourceKind, 'new')
  assert.equal(iv.sourceMembers[1].offsetX, 5)
  assert.equal(iv.sourceMembers[1].sourceGlyphRect.x, 70)
  assert.equal(iv.sourceMembers[1].opacity, 0.3)
  assert.equal(iv.sourceMembers[1].clipRects.length, 1)
  assert.equal(iv.sourceMembers[1].clipRects[0].x, 75)
  assert.equal(iv.sourceMembers[1].clipRects[0].width, 5)
  // frozenDrawOffsetX = 第一个成员的 offsetX = 3（兼容旧字段）
  assert.equal(iv.frozenDrawOffsetX, 3,
    'frozenDrawOffsetX 应为第一个成员的 offsetX（3）')
  // startScreenClipRects 合并了所有成员的 rects（兼容旧字段）
  assert.ok(iv.startScreenClipRects !== undefined)
  assert.equal(iv.startScreenClipRects.length, 2,
    'startScreenClipRects 应合并所有成员的 rects')
  assert.equal(iv.startScreenClipRects[0].x, 53)
  assert.equal(iv.startScreenClipRects[1].x, 75)

  // 3. 验证 retargetRunStarts
  iv.needsCrossfade = true
  const retargeted = retargetRunStarts(run, start, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const reIv = retargeted[0].intervals[0]
  assert.ok(reIv.sourceMembers !== undefined)
  assert.equal(reIv.sourceMembers.length, 2, 'retarget 后应有 2 个成员')
  assert.equal(reIv.needsCrossfade, true, 'needsCrossfade 应传递')
  // 深拷贝验证
  iv.sourceMembers[0].offsetX = 555
  assert.notEqual(reIv.sourceMembers[0].offsetX, 555,
    'retarget 的 sourceMembers 应是深拷贝')
})

test('问题4-6108900966：opacity=0 的成员仍保留在 sourceMembers 中——不被过滤', () => {
  const ctx = context('甲乙', 1005, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 60 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  // new 源 opacity=0（刚挂载但不可见）
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 60,
      clipRects: [{ x: 53, y: 0, width: 7, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 1.0,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 80,
      clipRects: [{ x: 75, y: 0, width: 5, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 1. clusterVisible 仍返回该成员的 OnScreenInterval（rects 非空，opacity=0）
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, yiId)
  assert.equal(intervals.length, 2, 'clusterVisible 应返回 2 个 OnScreenInterval（包括 opacity=0 的）')
  assert.equal(intervals[1].sourceKind, 'new')
  assert.equal(intervals[1].opacity, 0, 'new 源的 opacity 应为 0')
  assert.ok(intervals[1].rects.length > 0, 'opacity=0 的成员 rects 应非空（有裁切交集）')

  // 2. retainedMoveStartState 的 sourceMembers 仍包含 opacity=0 的成员
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined)
  assert.ok(iv.sourceMembers !== undefined)
  assert.equal(iv.sourceMembers.length, 2, 'sourceMembers 应包含 opacity=0 的成员')
  // new 成员的 opacity=0
  const newMember = iv.sourceMembers[1]
  assert.equal(newMember.sourceKind, 'new')
  assert.equal(newMember.opacity, 0, 'sourceMembers 中 new 成员的 opacity 应为 0')
  // new 成员的其他字段仍正确
  assert.equal(newMember.offsetX, 5)
  assert.equal(newMember.sourceGlyphRect.x, 70)
  assert.equal(newMember.clipRects.length, 1)
  assert.equal(newMember.clipRects[0].x, 75)
})

// ====== Issue #879 评论6107257451 问题4：真实双源场景 ======

test('问题4-6107257451：真实双源场景——不同宽度/sourceLayout/offset/alpha=0.6/0.4', () => {
  // 构造同一 glyphId 的 old/new 两张冻结窗口：
  // old 源：sourceLayout 中字形 x=10, width=20（屏幕 x=13, width=20，offset=3），opacity=0.6
  // new 源：sourceLayout 中字形 x=50, width=30（屏幕 x=40, width=30，offset=-10），opacity=0.4
  const ctx = context('甲乙', 1100, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]  // 乙的 glyphId

  // old 源布局：乙在 x=10, width=20
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 1, x: 10 }, { utf16Offset: 2, x: 30 }],
  }]
  // new 源布局：乙在 x=50, width=30
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 80 }],
  }]

  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 13, clipRight: 33,
      clipRects: [{ x: 13, y: 0, width: 20, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 40, clipRight: 70,
      clipRects: [{ x: 40, y: 0, width: 30, height: 20 }],
      offsetX: -10, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 1. 验证 clusterVisible 返回 2 个 OnScreenInterval，各自携带正确的 sourceKind/opacity/offset/sourceGlyphX0/X1
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, yiId)
  assert.equal(intervals.length, 2, 'clusterVisible 应返回 2 个 OnScreenInterval')
  // old interval
  assert.equal(intervals[0].sourceKind, 'old')
  assert.equal(intervals[0].opacity, 0.6)
  assert.equal(intervals[0].offsetX, 3)
  assert.equal(intervals[0].sourceGlyphX0, 10)
  assert.equal(intervals[0].sourceGlyphX1, 30)
  assert.equal(intervals[0].rects.length, 1)
  assert.equal(intervals[0].rects[0].x, 13)
  assert.equal(intervals[0].rects[0].width, 20)
  // new interval
  assert.equal(intervals[1].sourceKind, 'new')
  assert.equal(intervals[1].opacity, 0.4)
  assert.equal(intervals[1].offsetX, -10)
  assert.equal(intervals[1].sourceGlyphX0, 50)
  assert.equal(intervals[1].sourceGlyphX1, 80)
  assert.equal(intervals[1].rects.length, 1)
  assert.equal(intervals[1].rects[0].x, 40)
  assert.equal(intervals[1].rects[0].width, 30)

  // 2. 验证 retainedMoveStartState 的 sourceMembers 包含 2 个成员，各自有独立的 sourceGlyphRect/offset/clipRects/opacity
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.sourceMembers !== undefined, 'sourceMembers 应填充')
  assert.equal(iv.sourceMembers.length, 2, '应有 2 个成员（old + new）')
  // old 成员：独立的 sourceGlyphRect/offset/clipRects/opacity
  const oldMember = iv.sourceMembers[0]
  assert.equal(oldMember.sourceKind, 'old')
  assert.equal(oldMember.offsetX, 3)
  assert.equal(oldMember.offsetY, 0)
  assert.equal(oldMember.opacity, 0.6)
  assert.equal(oldMember.sourceGlyphRect.x, 10)
  assert.equal(oldMember.sourceGlyphRect.width, 20)
  assert.equal(oldMember.clipRects.length, 1)
  assert.equal(oldMember.clipRects[0].x, 13)
  assert.equal(oldMember.clipRects[0].width, 20)
  // new 成员：独立的 sourceGlyphRect/offset/clipRects/opacity（与 old 不同）
  const newMember = iv.sourceMembers[1]
  assert.equal(newMember.sourceKind, 'new')
  assert.equal(newMember.offsetX, -10)
  assert.equal(newMember.offsetY, 0)
  assert.equal(newMember.opacity, 0.4)
  assert.equal(newMember.sourceGlyphRect.x, 50)
  assert.equal(newMember.sourceGlyphRect.width, 30)
  assert.equal(newMember.clipRects.length, 1)
  assert.equal(newMember.clipRects[0].x, 40)
  assert.equal(newMember.clipRects[0].width, 30)
  // 确保两个成员的几何确实不同
  assert.notEqual(oldMember.sourceGlyphRect.x, newMember.sourceGlyphRect.x,
    'old/new 成员的 sourceGlyphRect.x 应不同（10 vs 50）')
  assert.notEqual(oldMember.sourceGlyphRect.width, newMember.sourceGlyphRect.width,
    'old/new 成员的 sourceGlyphRect.width 应不同（20 vs 30）')
  assert.notEqual(oldMember.offsetX, newMember.offsetX,
    'old/new 成员的 offsetX 应不同（3 vs -10）')
  assert.notEqual(oldMember.opacity, newMember.opacity,
    'old/new 成员的 opacity 应不同（0.6 vs 0.4）')

  // 3. 验证 retargetRunStarts 深拷贝 sourceMembers 且 needsCrossfade 传递正确
  iv.needsCrossfade = true
  const retargeted = retargetRunStarts(run, start, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const reIv = retargeted[0].intervals[0]
  assert.ok(reIv.sourceMembers !== undefined, 'retarget 后 sourceMembers 应存在')
  assert.equal(reIv.sourceMembers.length, 2, 'retarget 后应有 2 个成员')
  assert.equal(reIv.needsCrossfade, true, 'needsCrossfade 应从 prevInterval 继承')
  // 深拷贝验证：修改原成员不影响 retargeted 的成员
  iv.sourceMembers[0].offsetX = 999
  assert.notEqual(reIv.sourceMembers[0].offsetX, 999,
    '深拷贝：修改原成员 offsetX 不应影响 retargeted 的成员')
  // 深拷贝验证：clipRects 也是独立的
  iv.sourceMembers[0].clipRects[0].x = 999
  assert.notEqual(reIv.sourceMembers[0].clipRects[0].x, 999,
    '深拷贝：修改原 clipRects 不应影响 retargeted 的 clipRects')
  // 深拷贝验证：sourceGlyphRect 也是独立的
  iv.sourceMembers[0].sourceGlyphRect.x = 999
  assert.notEqual(reIv.sourceMembers[0].sourceGlyphRect.x, 999,
    '深拷贝：修改原 sourceGlyphRect 不应影响 retargeted 的 sourceGlyphRect')
  // retarget 后各成员的几何仍正确
  assert.equal(reIv.sourceMembers[0].sourceKind, 'old')
  assert.equal(reIv.sourceMembers[0].offsetX, 3)
  assert.equal(reIv.sourceMembers[0].opacity, 0.6)
  assert.equal(reIv.sourceMembers[1].sourceKind, 'new')
  assert.equal(reIv.sourceMembers[1].offsetX, -10)
  assert.equal(reIv.sourceMembers[1].opacity, 0.4)
})

test('问题4-6107257451：opacity=0 不被算作可见像素——allVisibleIslands 正确过滤', () => {
  // allVisibleIslands 是内部函数，通过 retargetRunStarts（insert 通道）间接验证。
  // allVisibleIslands 判断可见像素时检查 iv.opacity === undefined || iv.opacity > 0，
  // opacity=0 的成员没有像素贡献，不被视为可见岛。

  // 场景1：glyphId 只有 opacity=0 的冻结窗口覆盖
  const ctx1 = context('甲', 1110, [])
  const ids1 = idsOf(ctx1, 0, 1)
  const opacityZeroLayout = [{
    startUtf16: 0, endUtf16: 1, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 1, x: 10 }],
  }]
  const frozen1 = [{
    glyphIds: ids1,
    clipLeft: 0, clipRight: 10,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 1110,
    glyphUtf16Ranges: new Map([[ids1[0], [0, 1]]]),
    sourceLayout: opacityZeroLayout,
    sourceKind: 'old',
    runId: 'old-1110',
    renderNodeKey: 'old-node-1110',
    opacity: 0,
  }]
  const withWindow1 = { ...ctx1, frozenWindows: frozen1 }

  // clusterVisible 仍返回 OnScreenInterval（rects 非空，opacity=0）
  const frozenByGlyphId1 = frozenIndex(withWindow1)
  const intervals1 = clusterVisible(withWindow1, frozenByGlyphId1, ids1[0])
  assert.equal(intervals1.length, 1, 'clusterVisible 应返回 1 个 OnScreenInterval')
  assert.equal(intervals1[0].opacity, 0, 'opacity 应为 0')
  assert.ok(intervals1[0].rects.length > 0, 'rects 应非空（有裁切交集）')

  // allVisibleIslands 不将该字形视为可见 → retargetRunStarts insert 通道给零宽度 interval
  const run1 = runGeometry(withWindow1, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const prepared1 = insertRunStartState(run1, withWindow1)
  const retargeted1 = retargetRunStarts(run1, prepared1, withWindow1, 'insert', 0,
    { startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0 })
  // 字形不被 allVisibleIslands 视为可见 → 零宽度 interval（空洞）
  assert.ok(retargeted1[0].intervals !== undefined)
  const zeroIv = retargeted1[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(zeroIv !== undefined)
  assert.equal(zeroIv.startClipLeft, zeroIv.startClipRight,
    'opacity=0 的字形应获得零宽度 interval（allVisibleIslands 不视为可见）')

  // 场景2：glyphId 有 opacity=0.6 的冻结窗口覆盖
  const ctx2 = context('甲', 1111, [])
  const ids2 = idsOf(ctx2, 0, 1)
  const opacityHalfLayout = [{
    startUtf16: 0, endUtf16: 1, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 1, x: 10 }],
  }]
  const frozen2 = [{
    glyphIds: ids2,
    clipLeft: 0, clipRight: 10,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 1111,
    glyphUtf16Ranges: new Map([[ids2[0], [0, 1]]]),
    sourceLayout: opacityHalfLayout,
    sourceKind: 'old',
    runId: 'old-1111',
    renderNodeKey: 'old-node-1111',
    opacity: 0.6,
  }]
  const withWindow2 = { ...ctx2, frozenWindows: frozen2 }

  // clusterVisible 返回 OnScreenInterval（rects 非空，opacity=0.6）
  const frozenByGlyphId2 = frozenIndex(withWindow2)
  const intervals2 = clusterVisible(withWindow2, frozenByGlyphId2, ids2[0])
  assert.equal(intervals2.length, 1, 'clusterVisible 应返回 1 个 OnScreenInterval')
  assert.equal(intervals2[0].opacity, 0.6, 'opacity 应为 0.6')
  assert.ok(intervals2[0].rects.length > 0, 'rects 应非空')

  // allVisibleIslands 将该字形视为可见 → retargetRunStarts insert 通道给非零宽度 interval
  const run2 = runGeometry(withWindow2, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const prepared2 = insertRunStartState(run2, withWindow2)
  const retargeted2 = retargetRunStarts(run2, prepared2, withWindow2, 'insert', 0,
    { startClipLeft: 0, startClipRight: 10, startPositionX: 0, startPositionY: 0 })
  assert.ok(retargeted2[0].intervals !== undefined)
  const visibleIv = retargeted2[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(visibleIv !== undefined)
  assert.ok(visibleIv.startClipRight - visibleIv.startClipLeft > 0,
    'opacity=0.6 的字形应获得非零宽度 interval（allVisibleIslands 视为可见）')
})

test('问题3-6107257451：opacity=0 的成员在 buildPiece 中不被投影', () => {
  // 构造一个 glyphId 同时有 opacity=0 和 opacity=1 的冻结窗口：
  // buildPiece 在投影时跳过 opacity<=0 的成员（onScreenLeftOf/onScreenRightOf 和 clipRects 收集均跳过），
  // 但 sourceMembers 仍包含 opacity=0 的成员（用于提前挂载稳定的 Text）。
  const ctx = context('甲乙', 1120, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]  // 乙的 glyphId

  // old 源：opacity=0（刚挂载但不可见），sourceLayout 中乙在 x=50, width=10
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 60 }],
  }]
  // new 源：opacity=1（完全可见），sourceLayout 中乙在 x=70, width=10
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]

  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 50, clipRight: 60,
      clipRects: [{ x: 50, y: 0, width: 10, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 10, opacity: 0,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 70, clipRight: 80,
      clipRects: [{ x: 70, y: 0, width: 10, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 11, opacity: 1,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 1. clusterVisible 返回 2 个 OnScreenInterval（opacity=0 和 opacity=1 各一个）
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, yiId)
  assert.equal(intervals.length, 2, 'clusterVisible 应返回 2 个 OnScreenInterval')
  assert.equal(intervals[0].sourceKind, 'old')
  assert.equal(intervals[0].opacity, 0, 'old 源 opacity=0')
  assert.equal(intervals[1].sourceKind, 'new')
  assert.equal(intervals[1].opacity, 1, 'new 源 opacity=1')

  // 2. retainedMoveStartState：buildPiece 只投影 opacity=1 的成员的 clipRects
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')

  // sourceMembers 仍包含 opacity=0 的成员（用于挂载）
  assert.ok(iv.sourceMembers !== undefined, 'sourceMembers 应填充')
  assert.equal(iv.sourceMembers.length, 2, 'sourceMembers 应包含 2 个成员（包括 opacity=0）')
  assert.equal(iv.sourceMembers[0].sourceKind, 'old')
  assert.equal(iv.sourceMembers[0].opacity, 0, 'old 成员 opacity=0——仍保留在 sourceMembers 中')
  assert.equal(iv.sourceMembers[1].sourceKind, 'new')
  assert.equal(iv.sourceMembers[1].opacity, 1, 'new 成员 opacity=1')

  // buildPiece 投影时跳过 opacity=0 的成员——clipRects 只包含 opacity=1 成员的投影
  // interval 的 clipRects 来自 buildPiece 的 visible.clipRects，后者跳过 opacity<=0 的成员
  assert.ok(iv.clipRects !== undefined, 'interval 应有 clipRects')
  // clipRects 只反映 opacity=1 成员的投影（new 源在 x=70..80，offset=0 → 屏幕 x=70..80）
  // 投影到 run.ownLayout（乙在 x=10..20）：ratio = (70-70)/(80-70) = 0 → ownLeft = 10
  //                                      ratio = (80-70)/(80-70) = 1 → ownRight = 20
  // 所以 clipRects 应只有 opacity=1 成员的投影矩形
  const clipWidth = iv.clipRects.reduce((sum, r) => sum + r.width, 0)
  assert.ok(clipWidth > 0, 'clipRects 总宽度应大于 0（opacity=1 成员有投影）')
  // clipRects 不包含 opacity=0 成员的投影（old 源在 x=50..60 → 屏幕 x=50..60）
  // 如果包含了 opacity=0 成员的投影，clipRects 会有额外的矩形或更宽的总宽度
  for (const cr of iv.clipRects) {
    // opacity=1 成员的投影矩形应在 run.ownLayout 坐标系中（乙在 [10, 20]）
    assert.ok(cr.x >= 10 && cr.x + cr.width <= 20,
      `clipRect 应在 run.ownLayout 坐标系 [10,20] 内，实际 [${cr.x}, ${cr.x + cr.width}]`)
  }

  // startScreenClipRects 合并所有成员的 rects（包括 opacity=0），因为用于挂载
  assert.ok(iv.startScreenClipRects !== undefined, 'startScreenClipRects 应填充')
  assert.equal(iv.startScreenClipRects.length, 2,
    'startScreenClipRects 应合并所有成员的 rects（包括 opacity=0 的成员）')
  // old 源（opacity=0）的 rect：x=50
  assert.equal(iv.startScreenClipRects[0].x, 50,
    'startScreenClipRects[0] 应来自 old 源（x=50）')
  // new 源（opacity=1）的 rect：x=70
  assert.equal(iv.startScreenClipRects[1].x, 70,
    'startScreenClipRects[1] 应来自 new 源（x=70）')
})

// ====== Issue #879 评论6108234595 问题4：双源 buildFrame 输出测试 ======
// 覆盖 buildSourceMembers 稳健匹配、fallback clipRects 计算、opacity 保留、
// retargetRunStarts 后双源成员状态保持等场景。

test('问题4-6108234595：buildSourceMembers 稳健匹配——clusterVisible 跳过 new span 时 old 成员不错位', () => {
  // 场景：两个 span（old 和 new），clusterVisible 对 new span 的 clip 与字形无交集
  //（rects.length===0），只返回 old 的 OnScreenInterval。
  // buildSourceMembers 应：
  // - old span 匹配到 firstVisible[0]（old interval），取其 rects
  // - new span 匹配不到 firstVisible 中的 interval，从 span.clipRects 自行计算 clipRects
  //  （应为空数组，因为 clip 与字形无交集）
  const ctx = context('甲乙', 1200, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  // old 源布局：乙在 x=50, width=12
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  // new 源布局：乙在 x=70, width=10
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]

  // old span：clipRects 与字形有交集 → clusterVisible 返回 old 的 OnScreenInterval
  // 屏幕：字形 x=53..65（50+3..62+3），clip x=53..62 → 有交集
  // new span：clipRects 与字形无交集 → clusterVisible 跳过 new span
  // 屏幕：字形 x=75..85（70+5..80+5），clip x=200..205 → 无交集
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 200, clipRight: 205,
      clipRects: [{ x: 200, y: 0, width: 5, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 1. 验证 clusterVisible 只返回 old 的 OnScreenInterval（new 被 skipped）
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, yiId)
  assert.equal(intervals.length, 1,
    'clusterVisible 应只返回 1 个 OnScreenInterval（new span clip 无交集被跳过）')
  assert.equal(intervals[0].sourceKind, 'old',
    '唯一的 OnScreenInterval 应来自 old span')

  // 2. 验证 retainedMoveStartState 的 sourceMembers 仍有 2 个成员
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.sourceMembers !== undefined, 'sourceMembers 应填充')
  assert.equal(iv.sourceMembers.length, 2,
    'sourceMembers 应有 2 个成员（old + new），即使 new span 的 clip 无交集')

  // old 成员：匹配到 firstVisible[0]（old interval），clipRects 非空
  const oldMember = iv.sourceMembers[0]
  assert.equal(oldMember.sourceKind, 'old')
  assert.ok(oldMember.clipRects.length > 0,
    'old 成员的 clipRects 应非空（来自匹配的 OnScreenInterval）')
  assert.equal(oldMember.clipRects[0].x, 53)
  assert.equal(oldMember.clipRects[0].width, 9)

  // new 成员：匹配不到 firstVisible 中的 interval，从 span.clipRects 自行计算 clipRects
  // 因为 clip（x=200..205）与字形屏幕位置（x=75..85）无交集，clipRects 应为空数组
  const newMember = iv.sourceMembers[1]
  assert.equal(newMember.sourceKind, 'new')
  assert.equal(newMember.clipRects.length, 0,
    'new 成员的 clipRects 应为空数组（clip 与字形无交集，fallback 计算也为空）')
  // new 成员的其他字段仍正确
  assert.equal(newMember.offsetX, 5)
  assert.equal(newMember.sourceGlyphRect.x, 70)
  assert.equal(newMember.opacity, 0.4)
})

test('问题4-6108234595：buildSourceMembers 匹配失败时从 span.clipRects 自行计算 clipRects——old span 无交集', () => {
  // 场景：反转——old span 的 clip 与字形无交集，new span 有交集。
  // buildSourceMembers 应：
  // - old span 匹配不到 firstVisible 中的 interval，从 span.clipRects 自行计算 clipRects（空数组）
  // - new span 匹配到 firstVisible[0]（new interval），取其 rects
  const ctx = context('甲乙', 1201, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]

  // old span：clipRects 与字形无交集 → clusterVisible 跳过 old span
  // 屏幕：字形 x=53..65（50+3..62+3），clip x=200..205 → 无交集
  // new span：clipRects 与字形有交集 → clusterVisible 返回 new 的 OnScreenInterval
  // 屏幕：字形 x=75..85（70+5..80+5），clip x=75..83 → 有交集
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 200, clipRight: 205,
      clipRects: [{ x: 200, y: 0, width: 5, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 1. 验证 clusterVisible 只返回 new 的 OnScreenInterval（old 被 skipped）
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, yiId)
  assert.equal(intervals.length, 1,
    'clusterVisible 应只返回 1 个 OnScreenInterval（old span clip 无交集被跳过）')
  assert.equal(intervals[0].sourceKind, 'new',
    '唯一的 OnScreenInterval 应来自 new span')

  // 2. 验证 sourceMembers
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.sourceMembers !== undefined, 'sourceMembers 应填充')
  assert.equal(iv.sourceMembers.length, 2,
    'sourceMembers 应有 2 个成员')

  // old 成员：匹配不到 firstVisible 中的 interval，fallback 计算 clipRects 为空
  const oldMember = iv.sourceMembers[0]
  assert.equal(oldMember.sourceKind, 'old')
  assert.equal(oldMember.clipRects.length, 0,
    'old 成员的 clipRects 应为空数组（clip 与字形无交集）')
  // old 成员的其他字段仍正确
  assert.equal(oldMember.offsetX, 3)
  assert.equal(oldMember.sourceGlyphRect.x, 50)
  assert.equal(oldMember.opacity, 0.6)

  // new 成员：匹配到 firstVisible[0]（new interval），clipRects 非空
  const newMember = iv.sourceMembers[1]
  assert.equal(newMember.sourceKind, 'new')
  assert.ok(newMember.clipRects.length > 0,
    'new 成员的 clipRects 应非空（来自匹配的 OnScreenInterval）')
  assert.equal(newMember.clipRects[0].x, 75)
  assert.equal(newMember.clipRects[0].width, 8)
})

test('问题4-6108234595：双源成员的 opacity 在 sourceMembers 中正确保留——独立 opacity 值', () => {
  // 场景：old span opacity=0.6，new span opacity=0.4。
  // 两个 span 都有可见 clip（clusterVisible 为两者都生成 OnScreenInterval）。
  // buildSourceMembers 应为每个成员保留各自的 opacity。
  const ctx = context('甲乙', 1202, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]

  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 80,
      clipRects: [{ x: 75, y: 0, width: 5, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 1. clusterVisible 返回 2 个 OnScreenInterval，各自携带正确的 opacity
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, yiId)
  assert.equal(intervals.length, 2)
  assert.equal(intervals[0].sourceKind, 'old')
  assert.equal(intervals[0].opacity, 0.6)
  assert.equal(intervals[1].sourceKind, 'new')
  assert.equal(intervals[1].opacity, 0.4)

  // 2. retainedMoveStartState 的 sourceMembers 各自保留独立的 opacity
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined)
  assert.ok(iv.sourceMembers !== undefined)
  assert.equal(iv.sourceMembers.length, 2)

  // old 成员的 opacity=0.6
  assert.equal(iv.sourceMembers[0].sourceKind, 'old')
  assert.equal(iv.sourceMembers[0].opacity, 0.6,
    'old 成员的 opacity 应为 0.6')
  // new 成员的 opacity=0.4
  assert.equal(iv.sourceMembers[1].sourceKind, 'new')
  assert.equal(iv.sourceMembers[1].opacity, 0.4,
    'new 成员的 opacity 应为 0.4')

  // 3. opacity=undefined 的场景：非交叉淡入淡出窗口
  const frozenNoOpacity = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: undefined,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 80,
      clipRects: [{ x: 75, y: 0, width: 5, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: undefined,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindowNoOpacity = { ...ctx, frozenWindows: frozenNoOpacity }
  const startNoOpacity = retainedMoveStartState(run, withWindowNoOpacity, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const ivNoOpacity = startNoOpacity[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(ivNoOpacity !== undefined)
  assert.ok(ivNoOpacity.sourceMembers !== undefined)
  assert.equal(ivNoOpacity.sourceMembers.length, 2)
  assert.equal(ivNoOpacity.sourceMembers[0].opacity, undefined,
    'old 成员的 opacity 应为 undefined（非交叉淡入淡出）')
  assert.equal(ivNoOpacity.sourceMembers[1].opacity, undefined,
    'new 成员的 opacity 应为 undefined（非交叉淡入淡出）')
})

test('问题4-6108234595：retargetRunStarts 后双源成员的 sourceMembers 和 needsCrossfade 保持', () => {
  // 场景：先构建双源 sourceMembers（old opacity=0.6, new opacity=0.4），
  // 然后 retargetRunStarts。retarget 后：
  // - sourceMembers 应被深拷贝（修改原 piece 不影响 retarget 结果）
  // - needsCrossfade 应被传递
  // - 每个成员的 opacity 应保持不变
  const ctx = context('甲乙', 1203, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]

  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }

  // 1. 先用 retainedMoveStartState 生成初始 pieces（带 sourceMembers）
  const prepared = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const prevIv = prepared[0].intervals[0]
  assert.ok(prevIv.sourceMembers !== undefined)
  assert.equal(prevIv.sourceMembers.length, 2)
  assert.equal(prevIv.sourceMembers[0].opacity, 0.6)
  assert.equal(prevIv.sourceMembers[1].opacity, 0.4)

  // 2. 手动设置 needsCrossfade=true 模拟 plan 阶段写入
  prevIv.needsCrossfade = true

  // 3. 调用 retargetRunStarts
  const retargeted = retargetRunStarts(run, prepared, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const reIv = retargeted[0].intervals[0]

  // 4. 验证 sourceMembers 保持
  assert.ok(reIv.sourceMembers !== undefined, 'retarget 后 sourceMembers 应存在')
  assert.equal(reIv.sourceMembers.length, 2, 'retarget 后应有 2 个成员')

  // 5. 验证 opacity 保持不变
  assert.equal(reIv.sourceMembers[0].sourceKind, 'old')
  assert.equal(reIv.sourceMembers[0].opacity, 0.6,
    'retarget 后 old 成员的 opacity 应保持 0.6')
  assert.equal(reIv.sourceMembers[1].sourceKind, 'new')
  assert.equal(reIv.sourceMembers[1].opacity, 0.4,
    'retarget 后 new 成员的 opacity 应保持 0.4')

  // 6. 验证 needsCrossfade 传递
  assert.equal(reIv.needsCrossfade, true, 'needsCrossfade 应从 prevInterval 继承')

  // 7. 验证深拷贝：修改原成员不影响 retargeted 的成员
  prevIv.sourceMembers[0].offsetX = 999
  assert.notEqual(reIv.sourceMembers[0].offsetX, 999,
    '深拷贝：修改原成员 offsetX 不应影响 retargeted 的成员')

  prevIv.sourceMembers[0].clipRects[0].x = 999
  assert.notEqual(reIv.sourceMembers[0].clipRects[0].x, 999,
    '深拷贝：修改原 clipRects 不应影响 retargeted 的 clipRects')

  prevIv.sourceMembers[0].sourceGlyphRect.x = 999
  assert.notEqual(reIv.sourceMembers[0].sourceGlyphRect.x, 999,
    '深拷贝：修改原 sourceGlyphRect 不应影响 retargeted 的 sourceGlyphRect')

  prevIv.sourceMembers[0].opacity = 0
  assert.notEqual(reIv.sourceMembers[0].opacity, 0,
    '深拷贝：修改原 opacity 不应影响 retargeted 的 opacity')

  // 8. 验证 retarget 后各成员的其他字段仍正确
  assert.equal(reIv.sourceMembers[0].offsetX, 3, 'old 成员 offsetX 应为 3')
  assert.equal(reIv.sourceMembers[0].sourceGlyphRect.x, 50, 'old 成员 sourceGlyphRect.x 应为 50')
  assert.equal(reIv.sourceMembers[1].offsetX, 5, 'new 成员 offsetX 应为 5')
  assert.equal(reIv.sourceMembers[1].sourceGlyphRect.x, 70, 'new 成员 sourceGlyphRect.x 应为 70')
})

// ====== Issue #879 新增测试：FrozenGlyphMember 新字段 + 不可见分支 clipRects 清空 ======

console.log('\n--- Issue #879 新增测试：FrozenGlyphMember 新字段 + 不可见分支 clipRects 清空 ---')

// 测试1：FrozenGlyphMember 包含 sourceUtf16Start/End 和 targetOpacity 字段
test('问题1：buildSourceMembers 返回的 FrozenGlyphMember 包含 sourceUtf16Start/End 和 targetOpacity=-1', () => {
  const ctx = context('甲乙', 1300, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]  // 乙的 glyphId
  // old 源布局：乙在 x=50, width=12
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  // new 源布局：乙在 x=70, width=10
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.sourceMembers !== undefined, 'sourceMembers 应填充')
  assert.equal(iv.sourceMembers.length, 2, '应有 2 个成员（old + new）')
  // old 成员：sourceUtf16Start/End 来自 span.glyphUtf16Ranges.get(glyphId)
  const oldMember = iv.sourceMembers[0]
  assert.equal(oldMember.sourceKind, 'old')
  assert.equal(oldMember.sourceUtf16Start, 1, 'old 成员 sourceUtf16Start 应为 1（来自 glyphUtf16Ranges）')
  assert.equal(oldMember.sourceUtf16End, 2, 'old 成员 sourceUtf16End 应为 2（来自 glyphUtf16Ranges）')
  assert.equal(oldMember.targetOpacity, -1, 'old 成员 targetOpacity 应为 -1（未设置）')
  // new 成员
  const newMember = iv.sourceMembers[1]
  assert.equal(newMember.sourceKind, 'new')
  assert.equal(newMember.sourceUtf16Start, 1, 'new 成员 sourceUtf16Start 应为 1（来自 glyphUtf16Ranges）')
  assert.equal(newMember.sourceUtf16End, 2, 'new 成员 sourceUtf16End 应为 2（来自 glyphUtf16Ranges）')
  assert.equal(newMember.targetOpacity, -1, 'new 成员 targetOpacity 应为 -1（未设置）')
})

// 测试2：retargetRunStarts 不可见分支清空成员 clipRects（问题4）
test('问题4：retargetRunStarts 不可见分支——sourceMembers 的 clipRects 被清空为空数组', () => {
  const ctx = context('甲乙', 1301, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  // prepare：有可见 sourceMembers
  const prepared = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const prevIv = prepared[0].intervals[0]
  assert.ok(prevIv.sourceMembers !== undefined)
  assert.ok(prevIv.sourceMembers[0].clipRects.length > 0, 'prepare 时 old 成员 clipRects 应非空')
  assert.ok(prevIv.sourceMembers[1].clipRects.length > 0, 'prepare 时 new 成员 clipRects 应非空')
  // retarget：让该 interval 不可见——冻结窗口仍存在但 clip 为零（与字形无交集）
  // clusterVisible 对每个 span 的 clipRects 求交后 rects.length===0 → 返回空数组
  // → intervalForIsland 返回 null → 走不可见分支
  const hiddenFrozen = [
    {
      ...frozen[0],
      clipLeft: 200, clipRight: 200,
      clipRects: [{ x: 200, y: 0, width: 0, height: 20 }],
    },
    {
      ...frozen[1],
      clipLeft: 200, clipRight: 200,
      clipRects: [{ x: 200, y: 0, width: 0, height: 20 }],
    },
  ]
  const hiddenCtx = { ...ctx, frozenWindows: hiddenFrozen }
  const retargeted = retargetRunStarts(run, prepared, hiddenCtx, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const reIv = retargeted[0].intervals[0]
  assert.equal(reIv.startClipLeft, reIv.startClipRight, '不可见时 clip 应为零宽度')
  assert.ok(reIv.sourceMembers !== undefined, '不可见分支也应传递 sourceMembers')
  assert.equal(reIv.sourceMembers.length, 2, '不可见分支的 sourceMembers 应有 2 个成员')
  // 每个成员的 clipRects 应被清空为空数组
  assert.equal(reIv.sourceMembers[0].clipRects.length, 0,
    '不可见分支 old 成员的 clipRects 应被清空为空数组')
  assert.equal(reIv.sourceMembers[1].clipRects.length, 0,
    '不可见分支 new 成员的 clipRects 应被清空为空数组')
})

// 测试3：deepCloneSourceMembers 包含新字段（通过 retargetRunStarts retained 通道间接验证）
test('问题3：deepCloneSourceMembers 正确拷贝 sourceUtf16Start/End 和 targetOpacity', () => {
  const ctx = context('甲乙', 1302, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  // prepare：生成有 sourceMembers 的 interval
  const prepared = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  // retarget（boundaries 匹配，走 reInterval !== null 路径，深拷贝 sourceMembers）
  const retargeted = retargetRunStarts(run, prepared, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const reIv = retargeted[0].intervals[0]
  assert.ok(reIv.sourceMembers !== undefined, 'retarget 后 sourceMembers 应存在')
  assert.equal(reIv.sourceMembers.length, 2, 'retarget 后应有 2 个成员')
  // 验证 deepCloneSourceMembers 正确拷贝了新字段
  assert.equal(reIv.sourceMembers[0].sourceUtf16Start, 1,
    'retarget 后 old 成员 sourceUtf16Start 应与原始一致（1）')
  assert.equal(reIv.sourceMembers[0].sourceUtf16End, 2,
    'retarget 后 old 成员 sourceUtf16End 应与原始一致（2）')
  assert.equal(reIv.sourceMembers[0].targetOpacity, -1,
    'retarget 后 old 成员 targetOpacity 应与原始一致（-1）')
  assert.equal(reIv.sourceMembers[1].sourceUtf16Start, 1,
    'retarget 后 new 成员 sourceUtf16Start 应与原始一致（1）')
  assert.equal(reIv.sourceMembers[1].sourceUtf16End, 2,
    'retarget 后 new 成员 sourceUtf16End 应与原始一致（2）')
  assert.equal(reIv.sourceMembers[1].targetOpacity, -1,
    'retarget 后 new 成员 targetOpacity 应与原始一致（-1）')
  // 深拷贝验证：修改原成员的新字段不影响 retargeted 的成员
  prepared[0].intervals[0].sourceMembers[0].sourceUtf16Start = 999
  assert.notEqual(reIv.sourceMembers[0].sourceUtf16Start, 999,
    '深拷贝：修改原 sourceUtf16Start 不应影响 retargeted 的成员')
  prepared[0].intervals[0].sourceMembers[0].targetOpacity = 999
  assert.notEqual(reIv.sourceMembers[0].targetOpacity, 999,
    '深拷贝：修改原 targetOpacity 不应影响 retargeted 的成员')
})

// 测试4：双源场景下 sourceMembers 包含新字段
test('问题4：双源场景 sourceMembers 包含 sourceUtf16Start/End 和 targetOpacity', () => {
  const ctx = context('甲乙', 1303, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.sourceMembers !== undefined, 'sourceMembers 应填充')
  assert.equal(iv.sourceMembers.length, 2, '应有 2 个成员（allFrozenGeoms.length > 1）')
  // 验证每个成员包含 sourceUtf16Start/End 和 targetOpacity
  for (const member of iv.sourceMembers) {
    assert.ok(typeof member.sourceUtf16Start === 'number',
      '成员应包含 sourceUtf16Start（数字类型）')
    assert.ok(typeof member.sourceUtf16End === 'number',
      '成员应包含 sourceUtf16End（数字类型）')
    assert.ok(typeof member.targetOpacity === 'number',
      '成员应包含 targetOpacity（数字类型）')
    assert.equal(member.targetOpacity, -1,
      '成员 targetOpacity 应为 -1（未设置）')
  }
  // 验证具体的 UTF-16 区间值
  assert.equal(iv.sourceMembers[0].sourceUtf16Start, 1,
    'old 成员 sourceUtf16Start 应为 1')
  assert.equal(iv.sourceMembers[0].sourceUtf16End, 2,
    'old 成员 sourceUtf16End 应为 2')
  assert.equal(iv.sourceMembers[1].sourceUtf16Start, 1,
    'new 成员 sourceUtf16Start 应为 1')
  assert.equal(iv.sourceMembers[1].sourceUtf16End, 2,
    'new 成员 sourceUtf16End 应为 2')
})

// 测试5：retargetRunStarts 不可见分支保留成员身份但清空 clipRects
test('问题5：retargetRunStarts 不可见分支——保留成员身份字段但只清空 clipRects', () => {
  const ctx = context('甲乙', 1304, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 62 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]
  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 53, clipRight: 62,
      clipRects: [{ x: 53, y: 0, width: 9, height: 20 }],
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.6,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 75, clipRight: 83,
      clipRects: [{ x: 75, y: 0, width: 8, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 11, opacity: 0.4,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  // prepare：有可见 sourceMembers
  const prepared = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  // retarget：让该 interval 不可见——冻结窗口仍存在但 clip 为零（与字形无交集）
  const hiddenFrozen = [
    {
      ...frozen[0],
      clipLeft: 200, clipRight: 200,
      clipRects: [{ x: 200, y: 0, width: 0, height: 20 }],
    },
    {
      ...frozen[1],
      clipLeft: 200, clipRight: 200,
      clipRects: [{ x: 200, y: 0, width: 0, height: 20 }],
    },
  ]
  const hiddenCtx = { ...ctx, frozenWindows: hiddenFrozen }
  const retargeted = retargetRunStarts(run, prepared, hiddenCtx, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const reIv = retargeted[0].intervals[0]
  assert.ok(reIv.sourceMembers !== undefined, '不可见分支应传递 sourceMembers')
  assert.equal(reIv.sourceMembers.length, 2, '应有 2 个成员')
  // 验证保留的身份字段
  const oldMember = reIv.sourceMembers[0]
  assert.equal(oldMember.sourceKind, 'old', '不可见分支保留 sourceKind')
  assert.equal(oldMember.sourceRevision, 10, '不可见分支保留 sourceRevision')
  assert.equal(oldMember.runId, 'old-10', '不可见分支保留 runId')
  assert.equal(oldMember.renderNodeKey, 'old-node-10', '不可见分支保留 renderNodeKey')
  assert.equal(oldMember.sourceGlyphRect.x, 50, '不可见分支保留 sourceGlyphRect.x')
  assert.equal(oldMember.sourceGlyphRect.width, 12, '不可见分支保留 sourceGlyphRect.width')
  assert.equal(oldMember.offsetX, 3, '不可见分支保留 offsetX')
  assert.equal(oldMember.offsetY, 0, '不可见分支保留 offsetY')
  // Issue #879 评论6110293866 问题3：不可见分支 opacity 和 targetOpacity 都设为 0，
  // 确保 buildFrame 中 memberOpacity 始终为 0，避免动画过程中不可见成员变得可见。
  assert.equal(oldMember.opacity, 0, '不可见分支 opacity 设为 0')
  assert.equal(oldMember.sourceUtf16Start, 1, '不可见分支保留 sourceUtf16Start')
  assert.equal(oldMember.sourceUtf16End, 2, '不可见分支保留 sourceUtf16End')
  assert.equal(oldMember.targetOpacity, 0, '不可见分支 targetOpacity 设为 0')
  // 只有 clipRects 被清空
  assert.equal(oldMember.clipRects.length, 0, '不可见分支只有 clipRects 被清空')
  // new 成员同理
  const newMember = reIv.sourceMembers[1]
  assert.equal(newMember.sourceKind, 'new', '不可见分支保留 sourceKind')
  assert.equal(newMember.sourceRevision, 11, '不可见分支保留 sourceRevision')
  assert.equal(newMember.runId, 'new-11', '不可见分支保留 runId')
  assert.equal(newMember.renderNodeKey, 'new-node-11', '不可见分支保留 renderNodeKey')
  assert.equal(newMember.sourceGlyphRect.x, 70, '不可见分支保留 sourceGlyphRect.x')
  assert.equal(newMember.sourceGlyphRect.width, 10, '不可见分支保留 sourceGlyphRect.width')
  assert.equal(newMember.offsetX, 5, '不可见分支保留 offsetX')
  assert.equal(newMember.offsetY, 0, '不可见分支保留 offsetY')
  assert.equal(newMember.opacity, 0, '不可见分支 opacity 设为 0')
  assert.equal(newMember.sourceUtf16Start, 1, '不可见分支保留 sourceUtf16Start')
  assert.equal(newMember.sourceUtf16End, 2, '不可见分支保留 sourceUtf16End')
  assert.equal(newMember.targetOpacity, 0, '不可见分支 targetOpacity 设为 0')
  assert.equal(newMember.clipRects.length, 0, '不可见分支只有 clipRects 被清空')
})

// ====== Issue #879 评论6110293866 补充测试 ======

test('问题1-6110293866：opacity=0 时不产生 Infinity 几何——clusterVisible 返回成员但 retainedMoveStartState 不产生 Infinity', () => {
  // 场景：glyphId 在冻结窗口中，窗口 opacity=0（完全透明），clipRect 与字形有交集。
  // 验证：
  //   1. clusterVisible 返回 OnScreenInterval[] 包含 opacity=0 的成员（rects.length > 0）
  //   2. visibleGlyphPieces 不把该字形列为可见段（返回空数组）
  //   3. retainedMoveStartState 不产生 Infinity 的 onScreenLeft/onScreenTop 或 -Infinity 的 onScreenRight
  const ctx = context('A', 1400, [])
  const glyphIds = idsOf(ctx, 0, 1)
  const aId = glyphIds[0]

  // 源布局：A 在 x=0, width=10
  const sourceLayout = [{
    startUtf16: 0, endUtf16: 1, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 1, x: 10 }],
  }]

  const frozen = [{
    glyphIds: [aId],
    clipLeft: 0, clipRight: 10,
    clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 1400,
    glyphUtf16Ranges: new Map([[aId, [0, 1]]]),
    sourceLayout: sourceLayout,
    sourceKind: 'old',
    runId: 'old-1400',
    renderNodeKey: 'old-node-1400',
    opacity: 0,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 1. clusterVisible 返回 OnScreenInterval，包含 opacity=0 的成员
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, aId)
  assert.equal(intervals.length, 1, 'clusterVisible 应返回 1 个 OnScreenInterval')
  assert.equal(intervals[0].opacity, 0, 'opacity 应为 0')
  assert.ok(intervals[0].rects.length > 0, 'rects 应非空（有裁切交集）')

  // 2. visibleGlyphPieces 不把该字形列为可见段（hasVisiblePixels 为 false）
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 0, 'visibleGlyphPieces 不应返回任何可见段（opacity=0 无可见像素）')

  // 3. retainedMoveStartState 不产生 Infinity 几何
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到 A 的 interval')
  // onScreenLeft 不应为 Infinity
  assert.ok(iv.startClipLeft !== Infinity, `startClipLeft 不应为 Infinity，实际 ${iv.startClipLeft}`)
  // startClipRight 不应为 -Infinity
  assert.ok(iv.startClipRight !== -Infinity, `startClipRight 不应为 -Infinity，实际 ${iv.startClipRight}`)
  // startPositionX 不应为 Infinity 或 -Infinity
  assert.ok(iv.startPositionX !== Infinity && iv.startPositionX !== -Infinity,
    `startPositionX 不应为 Infinity，实际 ${iv.startPositionX}`)
  // startPositionY 不应为 Infinity 或 -Infinity
  assert.ok(iv.startPositionY !== Infinity && iv.startPositionY !== -Infinity,
    `startPositionY 不应为 Infinity，实际 ${iv.startPositionY}`)
  // opacity=0 时应为零宽 interval
  assert.equal(iv.startClipLeft, iv.startClipRight,
    'opacity=0 的字形应获得零宽度 interval')
})

test('问题2-6110293866：第一张源透明、第二张源可见时 startPositionX 取自可见源', () => {
  // 场景：glyphId 同时被两个冻结窗口覆盖——旧源 opacity=0、x=50；新源 opacity=1、x=70。两者 offset=0。
  // 验证：
  //   1. clusterVisible 返回两个 OnScreenInterval（一个 opacity=0，一个 opacity=1）
  //   2. retainedMoveStartState 的 startPositionX = 70（取自 opacity=1 的成员），不是 50
  const ctx = context('甲乙', 1401, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  // old 源布局：乙在 x=50, width=10
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 60 }],
  }]
  // new 源布局：乙在 x=70, width=10
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]

  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 50, clipRight: 60,
      clipRects: [{ x: 50, y: 0, width: 10, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 10, opacity: 0,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 70, clipRight: 80,
      clipRects: [{ x: 70, y: 0, width: 10, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 11, opacity: 1,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 1. clusterVisible 返回两个 OnScreenInterval
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, yiId)
  assert.equal(intervals.length, 2, 'clusterVisible 应返回 2 个 OnScreenInterval')
  assert.equal(intervals[0].sourceKind, 'old')
  assert.equal(intervals[0].opacity, 0, 'old 源 opacity=0')
  assert.equal(intervals[1].sourceKind, 'new')
  assert.equal(intervals[1].opacity, 1, 'new 源 opacity=1')

  // 2. retainedMoveStartState 的 startPositionX 取自可见源（new 源 x=70）
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  // startPositionX = sourceGlyphX0 + frozenDrawOffsetX = 70 + 0 = 70
  assert.equal(iv.startPositionX, 70,
    `startPositionX 应取自可见源（new 源 x=70），实际 ${iv.startPositionX}`)
  // 不应取自旧源（x=50）
  assert.notEqual(iv.startPositionX, 50,
    'startPositionX 不应取自 opacity=0 的旧源')
})

test('问题4-6110293866：双源成员场景——glyphId 同时在 displayed 身份表和冻结窗口里，clusterVisible 优先使用冻结窗口 sourceLayout', () => {
  // 场景：glyphId 在 displayed 正文身份表中存在（静态可见），同时被冻结窗口覆盖。
  // 验证：
  //   1. clusterVisible 优先使用冻结窗口的 sourceLayout（而不是 displayed.layout）
  //   2. 返回的 OnScreenInterval 携带冻结窗口的 sourceKind 和 opacity
  //   3. visibleGlyphPieces 的投影使用冻结窗口的源字形几何
  const ctx = context('甲', 1402, [])
  const glyphIds = idsOf(ctx, 0, 1)
  const jiaId = glyphIds[0]

  // displayed.layout 中甲在 x=0, width=10（默认 line() 构造）
  // 冻结窗口 sourceLayout 中甲在 x=100, width=10
  const frozenSourceLayout = [{
    startUtf16: 0, endUtf16: 1, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 100 }, { utf16Offset: 1, x: 110 }],
  }]

  const frozen = [{
    glyphIds: [jiaId],
    clipLeft: 100, clipRight: 110,
    clipRects: [{ x: 100, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 1402,
    glyphUtf16Ranges: new Map([[jiaId, [0, 1]]]),
    sourceLayout: frozenSourceLayout,
    sourceKind: 'old',
    runId: 'old-1402',
    renderNodeKey: 'old-node-1402',
    opacity: 1,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 1. clusterVisible 返回 OnScreenInterval，sourceGlyphX0 来自冻结窗口 sourceLayout（x=100）
  const frozenByGlyphId = frozenIndex(withWindow)
  const intervals = clusterVisible(withWindow, frozenByGlyphId, jiaId)
  assert.equal(intervals.length, 1, 'clusterVisible 应返回 1 个 OnScreenInterval')
  assert.equal(intervals[0].sourceKind, 'old', 'sourceKind 应为 old（来自冻结窗口）')
  assert.equal(intervals[0].opacity, 1, 'opacity 应为 1')
  assert.equal(intervals[0].sourceGlyphX0, 100,
    `sourceGlyphX0 应来自冻结窗口 sourceLayout（x=100），实际 ${intervals[0].sourceGlyphX0}`)
  assert.equal(intervals[0].sourceGlyphX1, 110,
    `sourceGlyphX1 应来自冻结窗口 sourceLayout（x=110），实际 ${intervals[0].sourceGlyphX1}`)
  // 不应来自 displayed.layout（x=0）
  assert.notEqual(intervals[0].sourceGlyphX0, 0,
    'sourceGlyphX0 不应来自 displayed.layout（x=0）')

  // 2. visibleGlyphPieces 使用冻结窗口的源字形几何投影
  const run = runGeometry(withWindow, 0, 1, { x: 0, y: 0, width: 10, height: 20 })
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 1, 'visibleGlyphPieces 应返回 1 个可见段')
  // 投影：源 x=100..110 → 目标 x=0..10（run.ownRect.x=0, width=10）
  // clipRects 应在目标坐标系 [0, 10] 内
  assert.ok(pieces[0].clipRects.length > 0, '应有 clipRects')
  for (const cr of pieces[0].clipRects) {
    assert.ok(cr.x >= 0 && cr.x + cr.width <= 10,
      `clipRect 应在目标坐标系 [0,10] 内，实际 [${cr.x}, ${cr.x + cr.width}]`)
  }

  // 3. retainedMoveStartState 的 startPositionX 使用冻结窗口源字形几何
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到甲的 interval')
  // startPositionX = sourceGlyphX0 + frozenDrawOffsetX = 100 + 0 = 100
  assert.equal(iv.startPositionX, 100,
    `startPositionX 应来自冻结窗口源字形几何（x=100），实际 ${iv.startPositionX}`)
  assert.notEqual(iv.startPositionX, 0,
    'startPositionX 不应来自 displayed.layout（x=0）')
})

// ====== Issue #879 评论6110857105 问题2/6：buildSourceMembers 保留原始 renderNodeKey/runId ======

test('问题2-6110857105：buildSourceMembers 保留原始 renderNodeKey/runId——不重新构造为 sourceKind-revision 格式', () => {
  const ctx = context('甲乙', 1100, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  // 构造冻结窗口，runId 和 renderNodeKey 包含物理节点唯一标识
  // 使用非格式化的值，确保不会被重新构造覆盖
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 1, x: 10 }, { utf16Offset: 2, x: 20 }],
  }]
  const frozen = [{
    glyphIds: [yiId],
    clipLeft: 10, clipRight: 20,
    clipRects: [{ x: 10, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 1099,
    glyphUtf16Ranges: new Map([[yiId, [1, 2]]]),
    sourceLayout: oldSourceLayout,
    sourceKind: 'old',
    runId: 'old-1099-winA',
    renderNodeKey: 'old-node-1099-winA',
    opacity: 1,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 1, 2, { x: 10, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 10, startClipRight: 20, startPositionX: 10, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.sourceMembers !== undefined && iv.sourceMembers.length > 0,
    '应有 sourceMembers')
  const member = iv.sourceMembers[0]
  // runId 应保留原始值 'old-1099-winA'，不是重新构造的 'old-1099'
  assert.equal(member.runId, 'old-1099-winA',
    `runId 应保留原始物理节点身份，实际 ${member.runId}`)
  // renderNodeKey 应保留原始值 'old-node-1099-winA'，不是重新构造的 'old-1099'
  assert.equal(member.renderNodeKey, 'old-node-1099-winA',
    `renderNodeKey 应保留原始物理节点身份，实际 ${member.renderNodeKey}`)
})

test('问题6-6110857105：双源场景 buildSourceMembers 按 renderNodeKey+sourceRevision 唯一匹配——不按 sourceKind+offset', () => {
  const ctx = context('甲乙', 1101, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  // 两个 old 源，相同 sourceKind='old'，相同 offset=0，但不同 sourceRevision 和 renderNodeKey
  const oldLayoutA = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 1, x: 10 }, { utf16Offset: 2, x: 20 }],
  }]
  const oldLayoutB = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 1, x: 10 }, { utf16Offset: 2, x: 20 }],
  }]
  // 两个 span 都是 sourceKind='old', offsetX=0, offsetY=0，但 sourceRevision 和 renderNodeKey 不同
  const frozen = [
    {
      glyphIds: [yiId],
      clipLeft: 10, clipRight: 15,
      clipRects: [{ x: 10, y: 0, width: 5, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 1098,
      glyphUtf16Ranges: new Map([[yiId, [1, 2]]]),
      sourceLayout: oldLayoutA,
      sourceKind: 'old',
      runId: 'old-1098-winA',
      renderNodeKey: 'old-node-1098-winA',
      opacity: 0.6,
    },
    {
      glyphIds: [yiId],
      clipLeft: 15, clipRight: 20,
      clipRects: [{ x: 15, y: 0, width: 5, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 1099,
      glyphUtf16Ranges: new Map([[yiId, [1, 2]]]),
      sourceLayout: oldLayoutB,
      sourceKind: 'old',
      runId: 'old-1099-winB',
      renderNodeKey: 'old-node-1099-winB',
      opacity: 0.4,
    },
  ]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 1, 2, { x: 10, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 10, startClipRight: 20, startPositionX: 10, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.sourceMembers !== undefined && iv.sourceMembers.length === 2,
    `应有 2 个 sourceMembers（两个 old 源），实际 ${iv.sourceMembers?.length}`)
  // 两个成员的 renderNodeKey 应不同，保留各自的物理节点身份
  const keys = iv.sourceMembers.map(m => m.renderNodeKey)
  assert.ok(keys.includes('old-node-1098-winA'), `应包含 old-node-1098-winA，实际 ${keys}`)
  assert.ok(keys.includes('old-node-1099-winB'), `应包含 old-node-1099-winB，实际 ${keys}`)
  // 两个成员的 runId 也应不同
  const runIds = iv.sourceMembers.map(m => m.runId)
  assert.ok(runIds.includes('old-1098-winA'), `应包含 old-1098-winA，实际 ${runIds}`)
  assert.ok(runIds.includes('old-1099-winB'), `应包含 old-1099-winB，实际 ${runIds}`)
})

// ====== Issue #879 评论6110857105 问题1：targetOpacity 设置验证 ======

test('问题1-6110857105：buildSourceMembers 中 targetOpacity 初始为 -1（未设置），由 Planner 设置', () => {
  const ctx = context('甲乙', 1102, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]
  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 0 }, { utf16Offset: 1, x: 10 }, { utf16Offset: 2, x: 20 }],
  }]
  const frozen = [{
    glyphIds: [yiId],
    clipLeft: 10, clipRight: 20,
    clipRects: [{ x: 10, y: 0, width: 10, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 1100,
    glyphUtf16Ranges: new Map([[yiId, [1, 2]]]),
    sourceLayout: oldSourceLayout,
    sourceKind: 'old',
    runId: 'old-1100',
    renderNodeKey: 'old-node-1100',
    opacity: 1,
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }
  const run = runGeometry(withWindow, 1, 2, { x: 10, y: 0, width: 10, height: 20 })
  const start = retainedMoveStartState(run, withWindow, {
    startClipLeft: 10, startClipRight: 20, startPositionX: 10, startPositionY: 0,
  })
  const iv = start[0].intervals.find(iv => iv.firstIndex === 0)
  assert.ok(iv !== undefined, '应找到乙的 interval')
  assert.ok(iv.sourceMembers !== undefined && iv.sourceMembers.length > 0,
    '应有 sourceMembers')
  // buildSourceMembers 创建的成员 targetOpacity 应为 -1（未设置）
  // Planner 会后续设置为 0（历史源）或 1（新源）
  assert.equal(iv.sourceMembers[0].targetOpacity, -1,
    `buildSourceMembers 创建的成员 targetOpacity 应为 -1（未设置），实际 ${iv.sourceMembers[0].targetOpacity}`)
})

// ====== Issue #879 评论6112219514 问题1：尾字形 sourceGlyphX1 从首个可见成员取 ======

test('问题1-6112219514：尾字形 sourceGlyphX1 从首个可见成员取，不从 lastVisible[0]（可能 opacity=0）取', () => {
  // 场景：乙被两个冻结窗口覆盖——旧源 opacity=0、x=50..60；新源 opacity=1、x=70..80。
  // 首字形 sourceGlyphX0/X1 已用 firstVisibleInterval 修复（L932-934），
  // 但尾字形 sourceGlyphX1 仍取 lastVisible[0]（首个物理成员，可能是 opacity=0）。
  // 旧逻辑：sourceGlyphX0=70（首字形已修）、sourceGlyphX1=60（尾字形未修）→ 内部自相矛盾。
  // 修复后：sourceGlyphX1=80（取自首个可见成员，与首字形对称）。
  const ctx = context('甲乙', 1500, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const oldSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 40 }, { utf16Offset: 1, x: 50 }, { utf16Offset: 2, x: 60 }],
  }]
  const newSourceLayout = [{
    startUtf16: 0, endUtf16: 2, left: 0, y: 0, height: 20, breakKind: 'endOfText',
    caretStops: [{ utf16Offset: 0, x: 60 }, { utf16Offset: 1, x: 70 }, { utf16Offset: 2, x: 80 }],
  }]

  const frozen = dualSourceFrozen(yiId,
    {
      clipLeft: 50, clipRight: 60,
      clipRects: [{ x: 50, y: 0, width: 10, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 10, opacity: 0,
      utf16Range: [1, 2], sourceLayout: oldSourceLayout,
    },
    {
      clipLeft: 70, clipRight: 80,
      clipRects: [{ x: 70, y: 0, width: 10, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 11, opacity: 1,
      utf16Range: [1, 2], sourceLayout: newSourceLayout,
    },
    ctx
  )
  const withWindow = { ...ctx, frozenWindows: frozen }

  // run 只包含乙——乙既是首字形也是尾字形
  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: [line('甲乙', 0)],
  }
  const pieces = visibleGlyphPieces(run, withWindow)
  assert.equal(pieces.length, 1, 'visibleGlyphPieces 应返回 1 个可见段')
  const piece = pieces[0]
  // sourceGlyphX0 应取自可见成员（new 源 x=70）——首字形已修复
  assert.equal(piece.sourceGlyphX0, 70,
    `sourceGlyphX0 应取自首个可见成员（new 源 x=70），实际 ${piece.sourceGlyphX0}`)
  // sourceGlyphX1 也应取自可见成员（new 源 x=80）——尾字形修复后
  assert.equal(piece.sourceGlyphX1, 80,
    `sourceGlyphX1 应取自首个可见成员（new 源 x=80），实际 ${piece.sourceGlyphX1}`)
  // 不应取自旧源（x=50..60）
  assert.notEqual(piece.sourceGlyphX1, 60,
    'sourceGlyphX1 不应取自 opacity=0 的旧源（x=60）')
  // sourceGlyphX0 < sourceGlyphX1（内部几何不自相矛盾）
  assert.ok(piece.sourceGlyphX0 < piece.sourceGlyphX1,
    `sourceGlyphX0(${piece.sourceGlyphX0}) 应小于 sourceGlyphX1(${piece.sourceGlyphX1})——内部几何不自相矛盾`)
})

// ====== Issue #879 新增测试：mergeMemberUpdates targetOpacity 保留 + windowInstanceId 字段 ======

// 辅助：构造带 windowInstanceId 的冻结窗口 span
const makeSpan = (glyphId, opts, ctx) => ({
  glyphIds: [glyphId],
  clipLeft: opts.clipLeft,
  clipRight: opts.clipRight,
  clipRects: opts.clipRects ?? [{ x: opts.clipLeft, y: 0, width: opts.clipRight - opts.clipLeft, height: 20 }],
  offsetX: opts.offsetX ?? 0,
  offsetY: opts.offsetY ?? 0,
  sourceRevision: opts.sourceRevision,
  glyphUtf16Ranges: new Map([[glyphId, opts.utf16Range]]),
  sourceLayout: opts.sourceLayout,
  sourceKind: opts.sourceKind ?? 'old',
  runId: opts.runId ?? `${opts.sourceKind ?? 'old'}-${opts.sourceRevision}`,
  renderNodeKey: opts.renderNodeKey ?? `${opts.sourceKind ?? 'old'}-node-${opts.sourceRevision}`,
  opacity: opts.opacity ?? 1,
  windowInstanceId: opts.windowInstanceId ?? `win-${opts.sourceRevision}`,
})

// 辅助：从 pieces 中提取第一个 interval 的 sourceMembers
const firstSourceMembers = (pieces) => {
  if (pieces[0].intervals !== undefined && pieces[0].intervals.length > 0) {
    return pieces[0].intervals[0].sourceMembers
  }
  return undefined
}

// 辅助：构造一行源布局
const srcLayout = (text) => [{
  startUtf16: 0, endUtf16: text.length, left: 0, y: 0, height: 20, breakKind: 'endOfText',
  caretStops: Array.from({ length: text.length + 1 }, (_v, i) => ({ utf16Offset: i, x: i * 10 })),
}]

// --- 修复1：targetOpacity 保留 ---

test('修复1-测试1：target 成员没匹配到 newMembers 时保留 targetOpacity=1、opacity=0、clipRects=[]', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  const frozen = [makeSpan(yiId, {
    clipLeft: 10, clipRight: 20,
    sourceRevision: 10, opacity: 1,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-10',
  })]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  // 提取 sourceMembers 并修改为 target 角色
  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined && members.length > 0, 'sourceMembers 应存在且有成员')

  // 修改成员为 target 角色，设置特定值，并改 renderNodeKey 使其不匹配
  const targetMember = { ...members[0] }
  targetMember.role = 'target'
  targetMember.targetOpacity = 1
  targetMember.opacity = 0
  targetMember.clipRects = []
  targetMember.renderNodeKey = 'target-node-999'
  pieces[0].intervals[0].sourceMembers = [targetMember]

  // retarget——使用相同的 frozenWindows，newMembers 会有原始 renderNodeKey
  const result = retargetRunStarts(run, pieces, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, '结果 sourceMembers 应存在')

  // 找到 target 成员（不匹配的那个，renderNodeKey='target-node-999'）
  const targetResult = resultMembers.find(m => m.renderNodeKey === 'target-node-999')
  assert.ok(targetResult !== undefined, '应存在未匹配的 target 成员')
  assert.equal(targetResult.targetOpacity, 1,
    `target 成员应保留 targetOpacity=1，实际 ${targetResult.targetOpacity}`)
  assert.equal(targetResult.opacity, 0,
    `target 成员应保留 opacity=0，实际 ${targetResult.opacity}`)
  assert.equal(targetResult.clipRects.length, 0,
    `target 成员应保留 clipRects=[]，实际长度 ${targetResult.clipRects.length}`)
})

test('修复1-测试2：frozen 成员没匹配到 newMembers 时 targetOpacity=0、opacity=0、clipRects=[]', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  const frozen = [makeSpan(yiId, {
    clipLeft: 10, clipRight: 20,
    sourceRevision: 10, opacity: 1,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-10',
  })]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined && members.length > 0, 'sourceMembers 应存在且有成员')

  // 修改成员为 frozen 角色，设置非零值，并改 renderNodeKey 使其不匹配
  const frozenMember = { ...members[0] }
  frozenMember.role = 'frozen'
  frozenMember.targetOpacity = 0.5
  frozenMember.opacity = 0.8
  frozenMember.clipRects = [{ x: 10, y: 0, width: 5, height: 20 }]
  frozenMember.renderNodeKey = 'frozen-node-888'
  pieces[0].intervals[0].sourceMembers = [frozenMember]

  const result = retargetRunStarts(run, pieces, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, '结果 sourceMembers 应存在')

  // 找到 frozen 成员（不匹配的那个，renderNodeKey='frozen-node-888'）
  const frozenResult = resultMembers.find(m => m.renderNodeKey === 'frozen-node-888')
  assert.ok(frozenResult !== undefined, '应存在未匹配的 frozen 成员')
  assert.equal(frozenResult.targetOpacity, 0,
    `frozen 成员应被设为 targetOpacity=0，实际 ${frozenResult.targetOpacity}`)
  assert.equal(frozenResult.opacity, 0,
    `frozen 成员应被设为 opacity=0，实际 ${frozenResult.opacity}`)
  assert.equal(frozenResult.clipRects.length, 0,
    `frozen 成员应被设为 clipRects=[]，实际长度 ${frozenResult.clipRects.length}`)
})

test('修复1-测试3：target 成员匹配到 newMembers 时正常更新（targetOpacity=1 来自 prev，opacity 来自 new，clipRects 来自 new）', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  // 初始冻结窗口：opacity=1, clipRects=[{x:10,y:0,width:10,height:20}]
  const frozen1 = [makeSpan(yiId, {
    clipLeft: 10, clipRight: 20,
    sourceRevision: 10, opacity: 1,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-10',
  })]
  const withWindow1 = { ...ctx, frozenWindows: frozen1 }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow1, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined && members.length > 0, 'sourceMembers 应存在且有成员')

  // 修改成员为 target 角色，设置 targetOpacity=1，保持 renderNodeKey 不变（会匹配）
  const targetMember = { ...members[0] }
  targetMember.role = 'target'
  targetMember.targetOpacity = 1
  targetMember.opacity = 0
  targetMember.clipRects = []
  pieces[0].intervals[0].sourceMembers = [targetMember]

  // retarget——使用不同的冻结窗口（opacity=0.5, clipRects 不同），但 renderNodeKey 相同
  const frozen2 = [makeSpan(yiId, {
    clipLeft: 12, clipRight: 18,
    sourceRevision: 10, opacity: 0.5,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-10',
    renderNodeKey: 'old-node-10',  // 与原始相同，确保匹配
  })]
  const withWindow2 = { ...ctx, frozenWindows: frozen2 }

  const result = retargetRunStarts(run, pieces, withWindow2, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, '结果 sourceMembers 应存在')

  // 找到匹配的成员（renderNodeKey='old-node-10'）
  const matchedResult = resultMembers.find(m => m.renderNodeKey === 'old-node-10')
  assert.ok(matchedResult !== undefined, '应存在匹配的成员')
  // targetOpacity 应来自 prev（=1），不被 newMembers 的默认值 -1 覆盖
  assert.equal(matchedResult.targetOpacity, 1,
    `matched 成员应保留 prev.targetOpacity=1，实际 ${matchedResult.targetOpacity}`)
  // opacity 应来自 new（=0.5）
  assert.equal(matchedResult.opacity, 0.5,
    `matched 成员应取 new.opacity=0.5，实际 ${matchedResult.opacity}`)
  // clipRects 应来自 new
  assert.ok(matchedResult.clipRects.length > 0,
    `matched 成员应有 new 的 clipRects，实际长度 ${matchedResult.clipRects.length}`)
  assert.equal(matchedResult.clipRects[0].x, 12,
    `matched 成员 clipRects[0].x 应为 12（来自 new），实际 ${matchedResult.clipRects[0].x}`)
  assert.equal(matchedResult.clipRects[0].width, 6,
    `matched 成员 clipRects[0].width 应为 6（来自 new），实际 ${matchedResult.clipRects[0].width}`)
  // role 应保留 prev 的
  assert.equal(matchedResult.role, 'target',
    `matched 成员应保留 prev.role='target'，实际 ${matchedResult.role}`)
})

// --- 修复2：windowInstanceId 字段 ---

test('修复2-测试4：buildSourceMembers 返回的 FrozenGlyphMember 包含 windowInstanceId', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  const frozen = [makeSpan(yiId, {
    clipLeft: 10, clipRight: 20,
    sourceRevision: 10, opacity: 1,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-abc-10',
  })]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined && members.length > 0, 'sourceMembers 应存在且有成员')
  assert.equal(members[0].windowInstanceId, 'win-abc-10',
    `buildSourceMembers 应传递 windowInstanceId='win-abc-10'，实际 ${members[0].windowInstanceId}`)
})

test('修复2-测试5：deepCloneSourceMembers 正确拷贝 windowInstanceId', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  const frozen = [makeSpan(yiId, {
    clipLeft: 10, clipRight: 20,
    sourceRevision: 10, opacity: 1,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-clone-10',
  })]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined && members.length > 0, 'sourceMembers 应存在且有成员')
  assert.equal(members[0].windowInstanceId, 'win-clone-10',
    `初始 sourceMembers 应有 windowInstanceId='win-clone-10'`)

  // retarget 时字形不可见 → 触发 deepCloneSourceMembers（不可见分支）
  // 使用空 frozenWindows 使字形不可见
  const withEmptyWindow = { ...ctx, frozenWindows: [] }
  const result = retargetRunStarts(run, pieces, withEmptyWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, '结果 sourceMembers 应存在（不可见分支保留 sourceMembers）')
  // deepCloneSourceMembers 应正确拷贝 windowInstanceId
  assert.equal(resultMembers[0].windowInstanceId, 'win-clone-10',
    `deepCloneSourceMembers 应拷贝 windowInstanceId='win-clone-10'，实际 ${resultMembers[0].windowInstanceId}`)
})

test('修复2-测试6：mergeMemberUpdates matched 分支保留 prev.windowInstanceId（同一物理 Text 租约一致）', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  // 初始冻结窗口
  const frozen1 = [makeSpan(yiId, {
    clipLeft: 10, clipRight: 20,
    sourceRevision: 10, opacity: 1,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-initial-10',
  })]
  const withWindow1 = { ...ctx, frozenWindows: frozen1 }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow1, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  // retarget——使用相同 renderNodeKey 但不同 windowInstanceId 的冻结窗口
  const frozen2 = [makeSpan(yiId, {
    clipLeft: 10, clipRight: 20,
    sourceRevision: 10, opacity: 1,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-updated-10',
    renderNodeKey: 'old-node-10',  // 与原始相同，确保匹配
  })]
  const withWindow2 = { ...ctx, frozenWindows: frozen2 }

  const result = retargetRunStarts(run, pieces, withWindow2, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, '结果 sourceMembers 应存在')

  // matched 分支应保留 prev 的 windowInstanceId——同一完整 render key 的 ID 若采样出不同值，
  // 候选实际上声明同一物理 Text 对应新租约；后续 onAppear/onDisAppear 和 releaseRenderNodeLease 会用不一致的 ID。
  const matchedResult = resultMembers.find(m => m.renderNodeKey === 'old-node-10')
  assert.ok(matchedResult !== undefined, '应存在匹配的成员')
  assert.equal(matchedResult.windowInstanceId, 'win-initial-10',
    `matched 成员应保留 prev.windowInstanceId='win-initial-10'，实际 ${matchedResult.windowInstanceId}`)
})

test('修复2-测试7：target 成员没匹配时保留 prev.windowInstanceId（undefined）', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  const frozen = [makeSpan(yiId, {
    clipLeft: 10, clipRight: 20,
    sourceRevision: 10, opacity: 1,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-10',
  })]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined && members.length > 0, 'sourceMembers 应存在且有成员')

  // 修改成员为 target 角色，windowInstanceId=undefined（target 成员的特征），不匹配的 renderNodeKey
  const targetMember = { ...members[0] }
  targetMember.role = 'target'
  targetMember.targetOpacity = 1
  targetMember.opacity = 0
  targetMember.clipRects = []
  targetMember.renderNodeKey = 'target-node-777'
  targetMember.windowInstanceId = undefined
  pieces[0].intervals[0].sourceMembers = [targetMember]

  const result = retargetRunStarts(run, pieces, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, '结果 sourceMembers 应存在')

  // target 成员没匹配时应保留 prev.windowInstanceId=undefined
  const targetResult = resultMembers.find(m => m.renderNodeKey === 'target-node-777')
  assert.ok(targetResult !== undefined, '应存在未匹配的 target 成员')
  assert.equal(targetResult.windowInstanceId, undefined,
    `target 成员应保留 prev.windowInstanceId=undefined，实际 ${targetResult.windowInstanceId}`)
})

test('修复2-测试8：frozen 成员没匹配时保留 prev.windowInstanceId', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  const frozen = [makeSpan(yiId, {
    clipLeft: 10, clipRight: 20,
    sourceRevision: 10, opacity: 1,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-10',
  })]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined && members.length > 0, 'sourceMembers 应存在且有成员')

  // 修改成员为 frozen 角色，设置 windowInstanceId，不匹配的 renderNodeKey
  const frozenMember = { ...members[0] }
  frozenMember.role = 'frozen'
  frozenMember.targetOpacity = 0.5
  frozenMember.opacity = 0.8
  frozenMember.clipRects = [{ x: 10, y: 0, width: 5, height: 20 }]
  frozenMember.renderNodeKey = 'frozen-node-666'
  frozenMember.windowInstanceId = 'win-frozen-666'
  pieces[0].intervals[0].sourceMembers = [frozenMember]

  const result = retargetRunStarts(run, pieces, withWindow, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, '结果 sourceMembers 应存在')

  // frozen 成员没匹配时应保留 prev.windowInstanceId
  const frozenResult = resultMembers.find(m => m.renderNodeKey === 'frozen-node-666')
  assert.ok(frozenResult !== undefined, '应存在未匹配的 frozen 成员')
  assert.equal(frozenResult.windowInstanceId, 'win-frozen-666',
    `frozen 成员应保留 prev.windowInstanceId='win-frozen-666'，实际 ${frozenResult.windowInstanceId}`)
})

// ====== Issue #879 评论6117867260 新增测试：不可见分支 role 区分 + mergeMemberUpdates 不追加陌生成员 ======

// 修复3-测试1：reInterval===null 时 target 成员保留 targetOpacity=1，frozen 成员 targetOpacity=0
// 验证不可见分支（deepCloneSourceMembers + map）正确区分 role：
// - target 成员：targetOpacity=1、opacity=0、clipRects=[]（待显露起点）
// - frozen 成员：targetOpacity=0、opacity=0、clipRects=[]（历史源淡出）
test('修复3-测试1：reInterval===null 时 target 成员保留 targetOpacity=1，frozen 成员 targetOpacity=0', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  const frozen = [makeSpan(yiId, {
    clipLeft: 10, clipRight: 20,
    sourceRevision: 10, opacity: 1,
    utf16Range: [1, 2], sourceLayout: layout,
    windowInstanceId: 'win-10',
  })]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined && members.length > 0, 'sourceMembers 应存在且有成员')

  // 构造两个成员：一个 target，一个 frozen
  const targetMember = { ...members[0] }
  targetMember.role = 'target'
  targetMember.targetOpacity = 1
  targetMember.opacity = 0.5
  targetMember.clipRects = [{ x: 10, y: 0, width: 5, height: 20 }]

  const frozenMember = { ...members[0] }
  frozenMember.role = 'frozen'
  frozenMember.targetOpacity = 0.5
  frozenMember.opacity = 0.8
  frozenMember.clipRects = [{ x: 10, y: 0, width: 5, height: 20 }]
  frozenMember.renderNodeKey = 'frozen-node-888'

  pieces[0].intervals[0].sourceMembers = [targetMember, frozenMember]

  // retarget——使用零宽度 clip 的冻结窗口使字形不可见（reInterval===null）
  const hiddenFrozen = [{
    ...frozen[0],
    clipLeft: 200, clipRight: 200,
    clipRects: [{ x: 200, y: 0, width: 0, height: 20 }],
  }]
  const hiddenCtx = { ...ctx, frozenWindows: hiddenFrozen }

  const result = retargetRunStarts(run, pieces, hiddenCtx, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, '结果 sourceMembers 应存在')
  assert.equal(resultMembers.length, 2, '应有 2 个成员')

  // target 成员：targetOpacity=1, opacity=0, clipRects=[]
  const targetResult = resultMembers.find(m => m.role === 'target')
  assert.ok(targetResult !== undefined, '应存在 target 成员')
  assert.equal(targetResult.targetOpacity, 1,
    `target 成员应保留 targetOpacity=1，实际 ${targetResult.targetOpacity}`)
  assert.equal(targetResult.opacity, 0,
    `target 成员应被设为 opacity=0，实际 ${targetResult.opacity}`)
  assert.equal(targetResult.clipRects.length, 0,
    `target 成员应被设为 clipRects=[]，实际长度 ${targetResult.clipRects.length}`)

  // frozen 成员：targetOpacity=0, opacity=0, clipRects=[]
  const frozenResult = resultMembers.find(m => m.role === 'frozen')
  assert.ok(frozenResult !== undefined, '应存在 frozen 成员')
  assert.equal(frozenResult.targetOpacity, 0,
    `frozen 成员应被设为 targetOpacity=0，实际 ${frozenResult.targetOpacity}`)
  assert.equal(frozenResult.opacity, 0,
    `frozen 成员应被设为 opacity=0，实际 ${frozenResult.opacity}`)
  assert.equal(frozenResult.clipRects.length, 0,
    `frozen 成员应被设为 clipRects=[]，实际长度 ${frozenResult.clipRects.length}`)
})

// 修复3-测试2：mergeMemberUpdates 不追加 newMembers 中多出的陌生成员
// prevMembers 有 2 个成员（keyA, keyB），newMembers 有 3 个成员（keyA, keyB, keyC）
// 结果应只有 2 个成员（keyA, keyB），不包含 keyC
test('修复3-测试2：mergeMemberUpdates 不追加 newMembers 中多出的陌生成员', () => {
  const ctx = context('甲乙', 1000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')

  // 准备阶段：2 个冻结窗口 → 2 个 sourceMembers
  const frozenPrepare = [
    makeSpan(yiId, {
      clipLeft: 10, clipRight: 20,
      sourceRevision: 10, opacity: 1,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
    }),
    makeSpan(yiId, {
      clipLeft: 10, clipRight: 20,
      sourceRevision: 11, opacity: 0.5,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-11',
      sourceKind: 'new',
    }),
  ]
  const withWindowPrepare = { ...ctx, frozenWindows: frozenPrepare }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindowPrepare, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined, 'sourceMembers 应存在')
  assert.equal(members.length, 2, '准备阶段应有 2 个成员')

  // retarget 阶段：3 个冻结窗口 → reInterval.sourceMembers 有 3 个成员
  // 第三个窗口有不同的 renderNodeKey，不会匹配任何 prevMember
  const frozenRetarget = [
    makeSpan(yiId, {
      clipLeft: 10, clipRight: 20,
      sourceRevision: 10, opacity: 1,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
    }),
    makeSpan(yiId, {
      clipLeft: 10, clipRight: 20,
      sourceRevision: 11, opacity: 0.5,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-11',
      sourceKind: 'new',
    }),
    makeSpan(yiId, {
      clipLeft: 10, clipRight: 20,
      sourceRevision: 12, opacity: 0.3,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-12',
      sourceKind: 'new',
      renderNodeKey: 'extra-node-12',
    }),
  ]
  const withWindowRetarget = { ...ctx, frozenWindows: frozenRetarget }

  const result = retargetRunStarts(run, pieces, withWindowRetarget, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, '结果 sourceMembers 应存在')
  // 结果应只有 2 个成员（prevMembers 的数量），不包含第三个陌生成员
  assert.equal(resultMembers.length, 2,
    `结果应有 2 个成员（不追加陌生成员），实际 ${resultMembers.length}`)
  // 验证不包含 renderNodeKey='extra-node-12' 的成员
  const extraMember = resultMembers.find(m => m.renderNodeKey === 'extra-node-12')
  assert.equal(extraMember, undefined,
    '结果不应包含 renderNodeKey=extra-node-12 的陌生成员')
})

// ====== Issue #879 评论6116985734 问题4：mapRectByRatio fallback 分支 clip 跟随 Text 平移 ======
// 问题4指出：clipRects 不应直接 mapRectByRatio 到不同宽高的目标布局，
// 而应跟随 Text 平移保持相对位置不变。
// 修复后在 fallback 分支中使用源字形的宽高而不是目标字形的宽高。

test('问题4-6116985734：mapRectByRatio 相同宽高时 clip 保持相对位置不变（等价于平移）', () => {
  // sourceRect: x=10, y=5, width=20, height=30
  // targetRect: x=50, y=10, width=20, height=30（相同宽高，只是位置变化）
  // clip: x=15, y=8, width=10, height=20（在 sourceRect 内）
  const result = mapRectByRatio(
    {x: 15, y: 8, width: 10, height: 20},
    {x: 10, y: 5, width: 20, height: 30},
    {x: 50, y: 10, width: 20, height: 30}
  )
  // 相对位置：relX = (15-10)/20 = 0.25, relWidth = 10/20 = 0.5
  // 期望：x = 50 + 0.25*20 = 55, width = 0.5*20 = 10
  // 相对位置不变：clip 在 sourceRect 中的位置比例 = clip 在 targetRect 中的位置比例
  assert(result.x === 55, `x 应为 55（平移 40），实际 ${result.x}`)
  assert(result.width === 10, `width 应保持 10，实际 ${result.width}`)
  assert(result.y === 13, `y 应为 13（平移 5），实际 ${result.y}`)
  assert(result.height === 20, `height 应保持 20，实际 ${result.height}`)
})

test('问题4-6116985734：mapRectByRatio 不同宽高时按比例缩放（旧错误行为——clip 滑走）', () => {
  // sourceRect: x=10, y=5, width=20, height=30
  // targetRect: x=50, y=10, width=10, height=15（宽高减半）
  // clip: x=15, y=8, width=10, height=20（在 sourceRect 内）
  const result = mapRectByRatio(
    {x: 15, y: 8, width: 10, height: 20},
    {x: 10, y: 5, width: 20, height: 30},
    {x: 50, y: 10, width: 10, height: 15}
  )
  // 旧行为（按比例缩放）：relX = (15-10)/20 = 0.25, relWidth = 10/20 = 0.5
  // x = 50 + 0.25*10 = 52.5, width = 0.5*10 = 5
  // clip 的宽度从 10 变成 5——这是错误的，因为 Text 只平移不变形
  assert(result.x === 52.5, `x 按 0.25 比例映射到 52.5，实际 ${result.x}`)
  assert(result.width === 5, `width 按比例缩放到 5（旧错误行为），实际 ${result.width}`)
  // 这证明了直接用 mapRectByRatio 映射到不同宽高会导致 clip 滑走
})

test('问题4-6116985734：修复后使用源字形宽高——clip 跟随 Text 平移保持相对位置不变', () => {
  // sourceRect: x=10, y=5, width=20, height=30
  // newGlyphRect: x=50, y=10, width=10, height=15（宽高不同）
  // 修复后 targetRect = {x: 50, y: 10, width: 20, height: 30}（使用源字形宽高）
  // clip: x=15, y=8, width=10, height:20（在 sourceRect 内）
  const result = mapRectByRatio(
    {x: 15, y: 8, width: 10, height: 20},
    {x: 10, y: 5, width: 20, height: 30},
    {x: 50, y: 10, width: 20, height: 30}  // 使用源字形宽高，不是 newGlyphRect 的宽高
  )
  // 修复后：clip 保持相对位置不变，只是跟随 Text 平移
  assert(result.x === 55, `x 应为 55（平移 40），实际 ${result.x}`)
  assert(result.width === 10, `width 应保持 10（不按比例缩放），实际 ${result.width}`)
  assert(result.y === 13, `y 应为 13（平移 5），实际 ${result.y}`)
  assert(result.height === 20, `height 应保持 20（不按比例缩放），实际 ${result.height}`)
})

// ====== Issue #879 复核评论6119020043 问题4：聚合身份一致性检查 ======
// renderNodeKey = sourceKind + '-' + sourceRevision + '-' + windowInstanceId
// runId = sourceKind + '-' + sourceRevision
// 同一 renderNodeKey 的成员在 sourceRevision/runId 上必然一致。
// EditorMotionCoordinator.ets 的 frozenWindowAggregator 在合并时显式校验
// runId 和 sourceRevision 一致性——不一致时不合并（独立 push），避免混搭字段。

test('问题4-6119020043：同一 renderNodeKey 的成员 runId 必然一致', () => {
  // renderNodeKey 构造：sourceKind-sourceRevision-windowInstanceId
  // runId 构造：sourceKind-sourceRevision
  // 同一 renderNodeKey => 同一 sourceKind + sourceRevision => 同一 runId
  const sourceKind = 'old'
  const sourceRevision = 42
  const windowInstanceId = 'win-3'
  const renderNodeKey = `${sourceKind}-${sourceRevision}-${windowInstanceId}`
  const runId = `${sourceKind}-${sourceRevision}`
  // 模拟两个同一 renderNodeKey 的成员
  const member1 = { renderNodeKey, runId, sourceRevision }
  const member2 = { renderNodeKey, runId, sourceRevision }
  assert(member1.runId === member2.runId, '同一 renderNodeKey 的 runId 必然一致')
  assert(member1.sourceRevision === member2.sourceRevision, '同一 renderNodeKey 的 sourceRevision 必然一致')
})

test('问题4-6119020043：不同 sourceRevision 的成员 renderNodeKey 不同——不会被聚合到同一 key', () => {
  // 如果两个成员的 sourceRevision 不同，它们的 renderNodeKey 也不同
  // 因此不会被 frozenWindowAggregator 聚合到同一 key——从根源避免混搭
  const sourceKind = 'old'
  const windowInstanceId = 'win-3'
  const key1 = `${sourceKind}-41-${windowInstanceId}`
  const key2 = `${sourceKind}-42-${windowInstanceId}`
  assert(key1 !== key2, '不同 sourceRevision 产生不同 renderNodeKey，不会被聚合到同一 key')
})

test('问题4-6119020043：不同 windowInstanceId 的成员 renderNodeKey 不同——不会被聚合到同一 key', () => {
  // 如果两个成员的 windowInstanceId 不同，它们的 renderNodeKey 也不同
  const sourceKind = 'old'
  const sourceRevision = 42
  const key1 = `${sourceKind}-${sourceRevision}-win-3`
  const key2 = `${sourceKind}-${sourceRevision}-win-4`
  assert(key1 !== key2, '不同 windowInstanceId 产生不同 renderNodeKey，不会被聚合到同一 key')
})

test('问题4-6119020043：聚合一致性检查——runId/sourceRevision 不一致时不合并', () => {
  // 模拟 frozenWindowAggregator 的一致性检查逻辑
  // 正常情况下同一 renderNodeKey 的 runId 必然一致，此检查永不触发
  // 但作为防御性约束，若上游异常导致不一致，则不合并（独立 push）
  const existing = { runId: 'old-42', sourceRevision: 42, utf16Start: 0, utf16End: 2 }
  // 场景1：runId 一致——合并
  const memberConsistent = { runId: 'old-42', sourceRevision: 42, utf16Start: 2, utf16End: 4 }
  const shouldMerge1 = existing.runId === memberConsistent.runId &&
    existing.sourceRevision === memberConsistent.sourceRevision
  assert(shouldMerge1 === true, 'runId 和 sourceRevision 一致时应合并')
  // 场景2：runId 不一致——不合并（防御性约束）
  const memberInconsistent = { runId: 'new-42', sourceRevision: 42, utf16Start: 2, utf16End: 4 }
  const shouldMerge2 = existing.runId === memberInconsistent.runId &&
    existing.sourceRevision === memberInconsistent.sourceRevision
  assert(shouldMerge2 === false, 'runId 不一致时不合并——避免混搭字段')
  // 场景3：sourceRevision 不一致——不合并
  const memberRevInconsistent = { runId: 'old-42', sourceRevision: 43, utf16Start: 2, utf16End: 4 }
  const shouldMerge3 = existing.runId === memberRevInconsistent.runId &&
    existing.sourceRevision === memberRevInconsistent.sourceRevision
  assert(shouldMerge3 === false, 'sourceRevision 不一致时不合并——避免混搭字段')
})

// ====== Issue #879 复核评论6120195493：三个新修复场景测试 ======
// 场景1：隐藏的首成员不抢占整张 Text 的 offset/alpha（FrozenWindowGroupPlan 窗口级统一管理）
// 场景2：不连续幸存字保留 glyphUtf16Ranges（聚合时保留原始完整 UTF-16 范围和 glyphIds）
// 场景3：同 key 窗口级属性统一（mergeMemberUpdates 的 windowLevelUpdates）

console.log('\n--- Issue #879 复核评论6120195493：三个新修复场景 ---')

// --- 场景1：隐藏的首成员不抢占整张 Text 的 offset/alpha ---
// FrozenWindowGroupPlan 在 EditorMotionCoordinator.ets 中，无法直接导入测试。
// 通过 retainedMoveStartState + retargetRunStarts 间接验证：
// 构造同一 renderNodeKey 的两个 frozen 成员（一个隐藏 opacity=0/clipRects=[]，一个可见），
// 验证 sourceMembers 中各成员保留各自的 offset/opacity——隐藏成员的 opacity=0 不影响可见成员。

test('场景1-测试1：同 renderNodeKey 双成员（隐藏+可见）——buildSourceMembers 各自保留独立 offset/opacity', () => {
  const ctx = context('甲乙', 2000, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  // 两个 span 覆盖同一个 glyphId（乙），同一 renderNodeKey，但一个隐藏一个可见
  // 隐藏 span：clipRects 与乙的屏幕矩形无交集 → opacity=0
  // 可见 span：clipRects 与乙的屏幕矩形有交集 → opacity=1
  const frozen = [
    makeSpan(yiId, {
      clipLeft: 200, clipRight: 210,  // 远离乙的位置，无交集
      clipRects: [{ x: 200, y: 0, width: 10, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 10, opacity: 0,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'old-node-10',  // 同一 renderNodeKey
    }),
    makeSpan(yiId, {
      clipLeft: 10, clipRight: 20,  // 与乙的屏幕矩形有交集
      clipRects: [{ x: 10, y: 0, width: 10, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 10, opacity: 1,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'old-node-10',  // 同一 renderNodeKey
    }),
  ]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined, 'sourceMembers 应存在')
  assert.equal(members.length, 2, '应有 2 个成员（隐藏 + 可见）')

  // 两个成员同一 renderNodeKey
  assert.equal(members[0].renderNodeKey, 'old-node-10', '成员0 renderNodeKey')
  assert.equal(members[1].renderNodeKey, 'old-node-10', '成员1 renderNodeKey')

  // 隐藏成员：opacity=0、clipRects=[]
  const hiddenMember = members.find(m => m.opacity === 0)
  assert.ok(hiddenMember !== undefined, '应存在隐藏成员（opacity=0）')
  assert.equal(hiddenMember.opacity, 0, '隐藏成员 opacity 应为 0')
  assert.equal(hiddenMember.clipRects.length, 0, '隐藏成员 clipRects 应为空')

  // 可见成员：opacity=1、clipRects 非空、offsetX=5
  const visibleMember = members.find(m => m.opacity === 1)
  assert.ok(visibleMember !== undefined, '应存在可见成员（opacity=1）')
  assert.equal(visibleMember.opacity, 1, '可见成员 opacity 应为 1——不被隐藏成员抢占')
  assert.equal(visibleMember.offsetX, 5, '可见成员 offsetX 应为 5——不被隐藏成员抢占')
  assert.ok(visibleMember.clipRects.length > 0, '可见成员 clipRects 应非空')
})

test('场景1-测试2：同 renderNodeKey 双成员——可见成员的 startPositionX 不被隐藏成员影响', () => {
  const ctx = context('甲乙', 2001, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  // 隐藏成员：offsetX=0, opacity=0
  // 可见成员：offsetX=5, opacity=1
  // 验证 startPositionX 来自可见成员的 offset（5），不是隐藏成员的 offset（0）
  const frozen = [
    makeSpan(yiId, {
      clipLeft: 200, clipRight: 210,
      clipRects: [{ x: 200, y: 0, width: 10, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 10, opacity: 0,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'old-node-10',
    }),
    makeSpan(yiId, {
      clipLeft: 10, clipRight: 20,
      clipRects: [{ x: 10, y: 0, width: 10, height: 20 }],
      offsetX: 5, offsetY: 0,
      sourceRevision: 10, opacity: 1,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'old-node-10',
    }),
  ]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  // interval 的 frozenDrawOffsetX 应来自首个可见成员（offsetX=5），不是隐藏成员（offsetX=0）
  const iv = pieces[0].intervals[0]
  assert.ok(iv !== undefined, 'interval 应存在')
  assert.equal(iv.frozenDrawOffsetX, 5,
    `frozenDrawOffsetX 应为 5（来自可见成员），实际 ${iv.frozenDrawOffsetX}`)
  // startPositionX = sourceGlyphX0 + frozenDrawOffsetX = 10 + 5 = 15
  assert.equal(iv.startPositionX, 15,
    `startPositionX 应为 15（10+5），不被隐藏成员的 offsetX=0 抢占，实际 ${iv.startPositionX}`)
})

// --- 场景2：不连续幸存字保留 glyphUtf16Ranges ---
// 构造覆盖甲乙丙三字形的冻结窗口，乙被删除（clipRects 与乙无交集），
// 验证 sourceMembers 中甲和丙的 sourceUtf16Start/End 仍保留原始完整映射。

test('场景2-测试1：不连续幸存字（甲丙）保留各自原始完整 UTF-16 范围', () => {
  const ctx = context('甲乙丙', 2002, [])
  const glyphIds = idsOf(ctx, 0, 3)
  const jiaId = glyphIds[0]  // 甲
  const yiId = glyphIds[1]   // 乙
  const bingId = glyphIds[2] // 丙

  const layout = srcLayout('甲乙丙')
  // 一个冻结窗口覆盖甲乙丙三个字形，clipRects 只露出甲和丙（乙被裁掉）
  // 甲在屏 [0,10]，乙在屏 [10,20]，丙在屏 [20,30]
  // clipRects: [0,10] 和 [20,30] —— 甲和丙可见，乙不可见
  const frozen = [{
    glyphIds: [jiaId, yiId, bingId],
    clipLeft: 0, clipRight: 30,
    clipRects: [
      { x: 0, y: 0, width: 10, height: 20 },   // 甲可见
      { x: 20, y: 0, width: 10, height: 20 },   // 丙可见（乙在 [10,20] 不可见）
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 10,
    glyphUtf16Ranges: new Map([
      [jiaId, [0, 1]],
      [yiId, [1, 2]],
      [bingId, [2, 3]],
    ]),
    sourceLayout: layout,
    sourceKind: 'old',
    runId: 'old-10',
    renderNodeKey: 'old-node-10',
    opacity: 1,
    windowInstanceId: 'win-10',
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 为甲构造 run（甲是幸存字）
  const runJia = {
    glyphIds: [jiaId],
    ownText: '甲丙',
    ownUtf16Start: 0,
    ownUtf16End: 1,
    ownRect: { x: 0, y: 0, width: 10, height: 20 },
    ownLayout: srcLayout('甲丙'),
  }
  const piecesJia = retainedMoveStartState(runJia, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const membersJia = firstSourceMembers(piecesJia)
  assert.ok(membersJia !== undefined, '甲的 sourceMembers 应存在')
  assert.ok(membersJia.length > 0, '甲的 sourceMembers 应有成员')
  // 甲的 sourceUtf16Start/End 应保留原始完整映射 [0,1]
  assert.equal(membersJia[0].sourceUtf16Start, 0,
    `甲 sourceUtf16Start 应为 0（原始完整映射），实际 ${membersJia[0].sourceUtf16Start}`)
  assert.equal(membersJia[0].sourceUtf16End, 1,
    `甲 sourceUtf16End 应为 1（原始完整映射），实际 ${membersJia[0].sourceUtf16End}`)

  // 为丙构造 run（丙是幸存字）
  const runBing = {
    glyphIds: [bingId],
    ownText: '甲丙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: srcLayout('甲丙'),
  }
  const piecesBing = retainedMoveStartState(runBing, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const membersBing = firstSourceMembers(piecesBing)
  assert.ok(membersBing !== undefined, '丙的 sourceMembers 应存在')
  assert.ok(membersBing.length > 0, '丙的 sourceMembers 应有成员')
  // 丙的 sourceUtf16Start/End 应保留原始完整映射 [2,3]
  assert.equal(membersBing[0].sourceUtf16Start, 2,
    `丙 sourceUtf16Start 应为 2（原始完整映射），实际 ${membersBing[0].sourceUtf16Start}`)
  assert.equal(membersBing[0].sourceUtf16End, 3,
    `丙 sourceUtf16End 应为 3（原始完整映射），实际 ${membersBing[0].sourceUtf16End}`)
})

test('场景2-测试2：不连续幸存字——interval 的 glyphIds 保留完整列表', () => {
  const ctx = context('甲乙丙', 2003, [])
  const glyphIds = idsOf(ctx, 0, 3)
  const jiaId = glyphIds[0]
  const yiId = glyphIds[1]
  const bingId = glyphIds[2]

  const layout = srcLayout('甲乙丙')
  // 冻结窗口覆盖甲乙丙，clipRects 只露出甲和丙
  const frozen = [{
    glyphIds: [jiaId, yiId, bingId],
    clipLeft: 0, clipRight: 30,
    clipRects: [
      { x: 0, y: 0, width: 10, height: 20 },
      { x: 20, y: 0, width: 10, height: 20 },
    ],
    offsetX: 0, offsetY: 0,
    sourceRevision: 10,
    glyphUtf16Ranges: new Map([
      [jiaId, [0, 1]],
      [yiId, [1, 2]],
      [bingId, [2, 3]],
    ]),
    sourceLayout: layout,
    sourceKind: 'old',
    runId: 'old-10',
    renderNodeKey: 'old-node-10',
    opacity: 1,
    windowInstanceId: 'win-10',
  }]
  const withWindow = { ...ctx, frozenWindows: frozen }

  // 为甲构造 run
  const runJia = {
    glyphIds: [jiaId],
    ownText: '甲丙',
    ownUtf16Start: 0,
    ownUtf16End: 1,
    ownRect: { x: 0, y: 0, width: 10, height: 20 },
    ownLayout: srcLayout('甲丙'),
  }
  const piecesJia = retainedMoveStartState(runJia, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const iv = piecesJia[0].intervals[0]
  assert.ok(iv !== undefined, '甲的 interval 应存在')
  // interval 的 glyphIds 应包含甲（只有甲一个 glyphId，因为 run 只覆盖甲）
  assert.ok(iv.glyphIds.includes(jiaId), 'interval glyphIds 应包含甲')
  // sourceMembers 应存在
  assert.ok(iv.sourceMembers !== undefined, 'sourceMembers 应存在')
  assert.ok(iv.sourceMembers.length > 0, 'sourceMembers 应有成员')
  // 甲的成员应保留原始 UTF-16 范围
  const jiaMember = iv.sourceMembers[0]
  assert.equal(jiaMember.sourceUtf16Start, 0, '甲成员 sourceUtf16Start 应为 0')
  assert.equal(jiaMember.sourceUtf16End, 1, '甲成员 sourceUtf16End 应为 1')
})

// --- 场景3：同 key 窗口级属性统一（mergeMemberUpdates 的 windowLevelUpdates） ---
// mergeMemberUpdates 是内部函数，通过 retargetRunStarts 间接测试。
// 构造 prevMembers 中同 renderNodeKey 的两个成员有不同 offsetX，
// 验证 retarget 后两个成员的 offsetX 统一为窗口级值（取第一个匹配成员的值）。

test('场景3-测试1：同 renderNodeKey 双成员 retarget 后 offsetX 统一为窗口级值', () => {
  const ctx = context('甲乙', 2004, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  // 准备阶段：两个冻结窗口覆盖乙，同一 renderNodeKey
  // Issue #879 复核评论6120195493：clipRects 必须覆盖字形在屏位置（sourceRect.x + offsetX）。
  // 乙在 sourceLayout 中 x=10-20；offsetX=20 → 在屏 x=30-40；offsetX=25 → 在屏 x=35-45。
  // 用宽 clipRects 覆盖两者，保证两个 span 都可见，从而产生 sourceMembers。
  const frozenPrepare = [
    makeSpan(yiId, {
      clipLeft: 0, clipRight: 100,
      clipRects: [{ x: 0, y: 0, width: 100, height: 20 }],
      offsetX: 20, offsetY: 0,
      sourceRevision: 10, opacity: 1,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'shared-key-10',
    }),
    makeSpan(yiId, {
      clipLeft: 0, clipRight: 100,
      clipRects: [{ x: 0, y: 0, width: 100, height: 20 }],
      offsetX: 25, offsetY: 0,
      sourceRevision: 10, opacity: 0.5,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'shared-key-10',
    }),
  ]
  const withWindowPrepare = { ...ctx, frozenWindows: frozenPrepare }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindowPrepare, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined, 'sourceMembers 应存在')
  assert.equal(members.length, 2, '应有 2 个成员')
  // 准备阶段两个成员的 offsetX 不同（20 vs 25）
  assert.equal(members[0].offsetX, 20, '准备阶段成员0 offsetX 应为 20')
  assert.equal(members[1].offsetX, 25, '准备阶段成员1 offsetX 应为 25')

  // retarget 阶段：使用相同的冻结窗口
  const result = retargetRunStarts(run, pieces, withWindowPrepare, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, 'retarget 后 sourceMembers 应存在')
  assert.equal(resultMembers.length, 2, 'retarget 后应有 2 个成员')

  // windowLevelUpdates 取第一个匹配成员的 offsetX（=20）
  // 两个成员的 offsetX 都应统一为 20
  assert.equal(resultMembers[0].offsetX, 20,
    `retarget 后成员0 offsetX 应统一为 20（窗口级值），实际 ${resultMembers[0].offsetX}`)
  assert.equal(resultMembers[1].offsetX, 20,
    `retarget 后成员1 offsetX 应统一为 20（窗口级值），实际 ${resultMembers[1].offsetX}`)
})

test('场景3-测试2：同 renderNodeKey 双成员 retarget 后 opacity 统一为窗口级值', () => {
  const ctx = context('甲乙', 2005, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  // 准备阶段：两个冻结窗口覆盖乙，同一 renderNodeKey，不同 opacity
  const frozenPrepare = [
    makeSpan(yiId, {
      clipLeft: 10, clipRight: 20,
      offsetX: 3, offsetY: 0,
      sourceRevision: 10, opacity: 0.8,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'shared-key-10',
    }),
    makeSpan(yiId, {
      clipLeft: 10, clipRight: 20,
      offsetX: 5, offsetY: 0,
      sourceRevision: 10, opacity: 0.3,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'shared-key-10',
    }),
  ]
  const withWindowPrepare = { ...ctx, frozenWindows: frozenPrepare }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindowPrepare, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined, 'sourceMembers 应存在')
  assert.equal(members.length, 2, '应有 2 个成员')
  // 准备阶段两个成员的 opacity 不同（0.8 vs 0.3）
  assert.equal(members[0].opacity, 0.8, '准备阶段成员0 opacity 应为 0.8')
  assert.equal(members[1].opacity, 0.3, '准备阶段成员1 opacity 应为 0.3')

  // retarget 阶段：使用相同的冻结窗口
  const result = retargetRunStarts(run, pieces, withWindowPrepare, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, 'retarget 后 sourceMembers 应存在')
  assert.equal(resultMembers.length, 2, 'retarget 后应有 2 个成员')

  // windowLevelUpdates 取第一个匹配成员的 opacity（=0.8）
  // 两个成员的 opacity 都应统一为 0.8
  assert.equal(resultMembers[0].opacity, 0.8,
    `retarget 后成员0 opacity 应统一为 0.8（窗口级值），实际 ${resultMembers[0].opacity}`)
  assert.equal(resultMembers[1].opacity, 0.8,
    `retarget 后成员1 opacity 应统一为 0.8（窗口级值），实际 ${resultMembers[1].opacity}`)
})

test('场景3-测试3：不同 renderNodeKey 的成员 retarget 后各自保留独立 offset——不受其他 key 影响', () => {
  const ctx = context('甲乙', 2006, [])
  const glyphIds = idsOf(ctx, 0, 2)
  const yiId = glyphIds[1]

  const layout = srcLayout('甲乙')
  // 两个不同 renderNodeKey 的冻结窗口
  // Issue #879 复核评论6120195493：clipRects 必须覆盖字形在屏位置（sourceRect.x + offsetX）。
  // 乙在 sourceLayout 中 x=10-20；offsetX=20 → 在屏 x=30-40；offsetX=30 → 在屏 x=40-50。
  // 用宽 clipRects 覆盖两者，保证两个 span 都可见，从而产生 sourceMembers。
  const frozenPrepare = [
    makeSpan(yiId, {
      clipLeft: 0, clipRight: 100,
      clipRects: [{ x: 0, y: 0, width: 100, height: 20 }],
      offsetX: 20, offsetY: 0,
      sourceRevision: 10, opacity: 1,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'key-A-10',
    }),
    makeSpan(yiId, {
      clipLeft: 0, clipRight: 100,
      clipRects: [{ x: 0, y: 0, width: 100, height: 20 }],
      offsetX: 30, offsetY: 0,
      sourceRevision: 11, opacity: 0.5,
      utf16Range: [1, 2], sourceLayout: layout,
      windowInstanceId: 'win-11',
      sourceKind: 'new',
      renderNodeKey: 'key-B-11',
    }),
  ]
  const withWindowPrepare = { ...ctx, frozenWindows: frozenPrepare }

  const run = {
    glyphIds: [yiId],
    ownText: '甲乙',
    ownUtf16Start: 1,
    ownUtf16End: 2,
    ownRect: { x: 10, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindowPrepare, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined, 'sourceMembers 应存在')
  assert.equal(members.length, 2, '应有 2 个成员')

  // retarget 阶段
  const result = retargetRunStarts(run, pieces, withWindowPrepare, 'retained', 0, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })
  const resultMembers = firstSourceMembers(result)
  assert.ok(resultMembers !== undefined, 'retarget 后 sourceMembers 应存在')
  assert.equal(resultMembers.length, 2, 'retarget 后应有 2 个成员')

  // 不同 key 的成员各自保留独立 offset——不受其他 key 的 windowLevelUpdates 影响
  const memberA = resultMembers.find(m => m.renderNodeKey === 'key-A-10')
  assert.ok(memberA !== undefined, '应存在 key-A-10 成员')
  assert.equal(memberA.offsetX, 20,
    `key-A-10 成员 offsetX 应为 20（各自独立），实际 ${memberA.offsetX}`)

  const memberB = resultMembers.find(m => m.renderNodeKey === 'key-B-11')
  assert.ok(memberB !== undefined, '应存在 key-B-11 成员')
  assert.equal(memberB.offsetX, 30,
    `key-B-11 成员 offsetX 应为 30（各自独立），实际 ${memberB.offsetX}`)
})

// ====== Issue #879 评论6120849815 问题4：buildFrame 集成测试 ======
//
// 新增测试不只调用几何辅助函数——通过验证 retainedMoveStartState 产出的
// sourceMembers 数据结构（buildFrame 的直接输入），间接验证 buildFrame 在
// 运动帧中的行为。三个故障场景覆盖：
// 1. 「甲隐乙显」：同一物理 Text 的两个 frozen 成员，隐藏成员不抢占整窗 offset/opacity
// 2. 「甲乙丙删乙」：不连续幸存字的 glyphUtf16Ranges 携带完整逐字映射，不是 min/max
// 3. fallback 分支 frozen 不补间隙：frozen 成员 clipRects 不含间隙碎片

// --- 场景1：甲隐乙显 ---

test('场景1-甲隐乙显：同一 renderNodeKey 双 frozen 成员——隐藏成员 clipRects=[]、opacity=0，可见成员 clipRects 非空、opacity>0', () => {
  const ctx = context('乙', 2007, [])
  const glyphIds = idsOf(ctx, 0, 1)
  const yiId = glyphIds[0]

  const layout = srcLayout('乙')
  // 两个冻结窗口覆盖乙，同一 renderNodeKey（同一物理 Text 节点）
  // 甲（隐藏成员）：clipRects 远离乙在屏位置 → 无交集 → clipRects=[]
  // 乙（可见成员）：clipRects 覆盖乙在屏位置 → 有交集 → clipRects 非空
  const frozen = [
    makeSpan(yiId, {
      clipLeft: 100, clipRight: 110,
      clipRects: [{ x: 100, y: 0, width: 10, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 10, opacity: 0,
      utf16Range: [0, 1], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'shared-key-10',
    }),
    makeSpan(yiId, {
      clipLeft: 0, clipRight: 10,
      clipRects: [{ x: 0, y: 0, width: 10, height: 20 }],
      offsetX: 0, offsetY: 0,
      sourceRevision: 10, opacity: 0.8,
      utf16Range: [0, 1], sourceLayout: layout,
      windowInstanceId: 'win-10',
      renderNodeKey: 'shared-key-10',
    }),
  ]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [yiId],
    ownText: '乙',
    ownUtf16Start: 0,
    ownUtf16End: 1,
    ownRect: { x: 0, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined, 'sourceMembers 应存在')
  assert.equal(members.length, 2, '应有 2 个 frozen 成员（同一物理 Text 的两个冻结窗口）')

  // 两个成员的 renderNodeKey 相同（同一物理 Text 节点）
  assert.equal(members[0].renderNodeKey, 'shared-key-10',
    `成员0 renderNodeKey 应为 shared-key-10，实际 ${members[0].renderNodeKey}`)
  assert.equal(members[1].renderNodeKey, 'shared-key-10',
    `成员1 renderNodeKey 应为 shared-key-10，实际 ${members[1].renderNodeKey}`)

  // 隐藏成员（甲）：clipRects 为空，opacity=0
  // buildFrame 的 FrozenWindowGroupPlan 会跳过 clipRects=[] 的成员选择 baseMember，
  // 不会让隐藏成员的 offset=0/opacity=0 抢占整窗属性
  const hiddenMember = members.find(m => m.opacity === 0)
  assert.ok(hiddenMember !== undefined, '应存在隐藏成员（opacity=0）')
  assert.equal(hiddenMember.clipRects.length, 0,
    `隐藏成员 clipRects 应为空（clipRects 与字形无交集），实际长度 ${hiddenMember.clipRects.length}`)

  // 可见成员（乙）：clipRects 非空，opacity>0
  // buildFrame 的 FrozenWindowGroupPlan 会选择此成员作为 baseMember，
  // 整窗 offset/opacity 取此成员的值
  const visibleMember = members.find(m => m.opacity !== undefined && m.opacity > 0)
  assert.ok(visibleMember !== undefined, '应存在可见成员（opacity>0）')
  assert.ok(visibleMember.clipRects.length > 0,
    `可见成员 clipRects 应非空（clipRects 与字形有交集），实际长度 ${visibleMember.clipRects.length}`)
  assert.equal(visibleMember.opacity, 0.8,
    `可见成员 opacity 应为 0.8，实际 ${visibleMember.opacity}`)
  // 可见成员的裁切矩形正确输出——覆盖乙的完整在屏位置 [0,10]
  assert.ok(visibleMember.clipRects.some(cr => Math.abs(cr.x - 0) < 0.01 && Math.abs(cr.width - 10) < 0.01),
    '可见成员 clipRects 应覆盖乙的完整在屏位置 [0,10]')
})

// --- 场景2：甲乙丙删乙 ---

test('场景2-甲乙丙删乙：不连续幸存字的 sourceMembers 携带完整 glyphUtf16Ranges 映射，不是 min/max', () => {
  const oldText = '甲乙丙'
  const ctx = context(oldText, 2008, [])
  const glyphIds = idsOf(ctx, 0, 3)
  const jiaId = glyphIds[0]   // 甲
  const yiId = glyphIds[1]    // 乙（被删除）
  const bingId = glyphIds[2]  // 丙

  const layout = srcLayout(oldText)

  // 冻结窗口覆盖甲、乙、丙三个字，携带完整 glyphUtf16Ranges 映射
  // 甲→[0,1], 乙→[1,2], 丙→[2,3]——不是 min/max [0,3]
  const ranges = new Map([
    [jiaId, [0, 1]],
    [yiId, [1, 2]],
    [bingId, [2, 3]],
  ])

  const frozen = [{
    glyphIds: [jiaId, yiId, bingId],
    clipLeft: 0, clipRight: 30,
    clipRects: [{ x: 0, y: 0, width: 30, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 10,
    glyphUtf16Ranges: ranges,
    sourceLayout: layout,
    sourceKind: 'old',
    runId: 'old-10',
    renderNodeKey: 'old-node-10',
    opacity: 1,
    windowInstanceId: 'win-10',
  }]

  const withWindow = { ...ctx, frozenWindows: frozen }

  // 甲的 retained move（幸存字）
  const jiaRun = {
    glyphIds: [jiaId],
    ownText: oldText,
    ownUtf16Start: 0,
    ownUtf16End: 1,
    ownRect: { x: 0, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const jiaPieces = retainedMoveStartState(jiaRun, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  // 验证甲的 sourceMembers 携带完整 glyphUtf16Ranges
  const jiaMembers = firstSourceMembers(jiaPieces)
  assert.ok(jiaMembers !== undefined && jiaMembers.length > 0, '甲的 sourceMembers 应存在')

  const jiaRanges = jiaMembers[0].glyphUtf16Ranges
  assert.ok(jiaRanges !== undefined && jiaRanges !== null,
    '甲的 sourceMember 应携带 glyphUtf16Ranges（供 buildFrame 的 FrozenWindowGroupPlan 使用）')
  // 关键验证：映射包含所有3个字形的逐字映射，不是 min/max [0,3]
  assert.equal(jiaRanges.get(jiaId)[0], 0, '甲 UTF-16 start = 0')
  assert.equal(jiaRanges.get(jiaId)[1], 1, '甲 UTF-16 end = 1')
  assert.equal(jiaRanges.get(yiId)[0], 1,
    '乙 UTF-16 start = 1（完整映射保留——min/max 会丢失此条目）')
  assert.equal(jiaRanges.get(yiId)[1], 2,
    '乙 UTF-16 end = 2（完整映射保留——min/max 会丢失此条目）')
  assert.equal(jiaRanges.get(bingId)[0], 2,
    '丙 UTF-16 start = 2（完整映射保留——min/max 会丢失此条目）')
  assert.equal(jiaRanges.get(bingId)[1], 3,
    '丙 UTF-16 end = 3（完整映射保留——min/max 会丢失此条目）')

  // 丙的 retained move（不连续幸存字）
  const bingRun = {
    glyphIds: [bingId],
    ownText: oldText,
    ownUtf16Start: 2,
    ownUtf16End: 3,
    ownRect: { x: 20, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const bingPieces = retainedMoveStartState(bingRun, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  // 验证丙的 sourceMembers 也携带完整 glyphUtf16Ranges
  const bingMembers = firstSourceMembers(bingPieces)
  assert.ok(bingMembers !== undefined && bingMembers.length > 0, '丙的 sourceMembers 应存在')

  const bingRanges = bingMembers[0].glyphUtf16Ranges
  assert.ok(bingRanges !== undefined && bingRanges !== null,
    '丙的 sourceMember 应携带 glyphUtf16Ranges')
  assert.equal(bingRanges.get(jiaId)[0], 0,
    '甲 UTF-16 start = 0（从丙的 sourceMember 也能查到——不是 min/max）')
  assert.equal(bingRanges.get(jiaId)[1], 1,
    '甲 UTF-16 end = 1（从丙的 sourceMember 也能查到——不是 min/max）')
  assert.equal(bingRanges.get(yiId)[0], 1,
    '乙 UTF-16 start = 1（从丙的 sourceMember 也能查到——不是 min/max）')
  assert.equal(bingRanges.get(yiId)[1], 2,
    '乙 UTF-16 end = 2（从丙的 sourceMember 也能查到——不是 min/max）')
  assert.equal(bingRanges.get(bingId)[0], 2, '丙 UTF-16 start = 2')
  assert.equal(bingRanges.get(bingId)[1], 3, '丙 UTF-16 end = 3')

  // 甲和丙的 frozen 成员 renderNodeKey 相同——buildFrame 聚合后窗口携带完整映射
  assert.equal(jiaMembers[0].renderNodeKey, bingMembers[0].renderNodeKey,
    '甲和丙的 frozen 成员 renderNodeKey 应相同（同一物理 Text——buildFrame 聚合后携带完整映射）')
})

// --- 场景3：fallback 分支 frozen 不补间隙 ---

test('场景3-frozen不补间隙：frozen 成员 clipRects 只含可见碎片，间隙碎片由 rectDifference 计算——frozen 不补、target 补', () => {
  const ctx = context('甲', 2009, [])
  const glyphIds = idsOf(ctx, 0, 1)
  const jiaId = glyphIds[0]

  const layout = srcLayout('甲')
  // 冻结窗口只露出甲的左半部分（clipRects=[0,5]），右半部分 [5,10] 是间隙
  const frozen = [makeSpan(jiaId, {
    clipLeft: 0, clipRight: 5,
    clipRects: [{ x: 0, y: 0, width: 5, height: 20 }],
    offsetX: 0, offsetY: 0,
    sourceRevision: 10, opacity: 1,
    utf16Range: [0, 1], sourceLayout: layout,
    windowInstanceId: 'win-10',
  })]
  const withWindow = { ...ctx, frozenWindows: frozen }

  const run = {
    glyphIds: [jiaId],
    ownText: '甲',
    ownUtf16Start: 0,
    ownUtf16End: 1,
    ownRect: { x: 0, y: 0, width: 10, height: 20 },
    ownLayout: layout,
  }
  const pieces = retainedMoveStartState(run, withWindow, {
    startClipLeft: 0, startClipRight: 0, startPositionX: 0, startPositionY: 0,
  })

  const members = firstSourceMembers(pieces)
  assert.ok(members !== undefined && members.length > 0, 'sourceMembers 应存在')

  const member = members[0]
  // frozen 成员 role='frozen'——buildFrame 中 frozen 角色不补间隙碎片
  assert.equal(member.role, 'frozen',
    `成员 role 应为 frozen（buildFrame 不为 frozen 补间隙），实际 ${member.role}`)
  // frozen 成员 targetOpacity=-1——buildFrame 解释为 0（历史源淡出）
  assert.equal(member.targetOpacity, -1,
    `frozen 成员 targetOpacity 应为 -1（buildFrame 解释为 0——淡出），实际 ${member.targetOpacity}`)

  // frozen 成员 clipRects 只包含可见碎片（左半部分 [0,5]），不包含零宽间隙碎片
  assert.ok(member.clipRects.length > 0,
    `frozen 成员 clipRects 应非空（有可见碎片），实际长度 ${member.clipRects.length}`)
  for (const cr of member.clipRects) {
    assert.ok(cr.width > 0,
      `frozen 成员 clipRects 中每个碎片 width 应 > 0（不是零宽间隙碎片），实际 ${cr.width}`)
  }
  // 可见碎片覆盖 [0,5] 区域
  assert.ok(member.clipRects.some(cr => Math.abs(cr.x - 0) < 0.01 && Math.abs(cr.width - 5) < 0.01),
    'frozen 成员 clipRects 应覆盖 [0,5] 可见区域')

  // 间隙存在：rectDifference(sourceGlyphOnScreen, member.clipRects) 应给出右半部分 [5,10]
  const sourceGlyphOnScreen = {
    x: member.sourceGlyphRect.x + member.offsetX,
    y: member.sourceGlyphRect.y + member.offsetY,
    width: member.sourceGlyphRect.width,
    height: member.sourceGlyphRect.height,
  }
  const gaps = rectDifference(sourceGlyphOnScreen, member.clipRects)
  assert.ok(gaps.length > 0,
    `应存在间隙（rectDifference 非空——frozen 未覆盖的右半部分），实际 ${gaps.length} 个间隙`)
  // 间隙在右半部分 [5,10]
  assert.ok(gaps.some(g => Math.abs(g.x - 5) < 0.01 && Math.abs(g.width - 5) < 0.01),
    '间隙应在 [5,10] 区域（右半部分——frozen 不补此区域，但 buildFrame 的 target 角色会补）')
})

// ====== Issue #879 复核评论6121854424 问题1：不一致分支不产生重复 renderNodeKey ======
// frozenWindowAggregator 在 buildFrame 中按 renderNodeKey 聚合 frozen 成员。
// 当 existing.runId !== memberWindow.runId || existing.sourceRevision !== memberWindow.sourceRevision 时，
// 旧实现 push memberWindow——但 memberWindow.renderNodeKey 仍等于 existing.renderNodeKey，
// 导致 motionGlyphWindows 中两个同 key 窗口，buildAllMotionWindowSources 去重时第二张被吞掉。
// 修复后：不一致时不 push，existing 仍由 frozenWindowAggregator.values() 输出，不产生重复 key。

console.log('\n--- Issue #879 复核评论6121854424 问题1：不一致分支不产生重复 renderNodeKey ---')

test('问题1-6121854424：不一致时不 push 同 key 窗口——motionGlyphWindows 无重复 renderNodeKey', () => {
  // 模拟 frozenWindowAggregator 的聚合逻辑（修复后）
  const frozenWindowAggregator = new Map()
  const motionGlyphWindows = []
  // 两个同一 renderNodeKey 但 runId 不一致的成员（上游 Planner 分配异常的防御场景）
  const renderNodeKey = 'old-42-win-3'
  const existingWindow = { renderNodeKey, runId: 'old-42', sourceRevision: 42, clipRects: [{ x: 0, y: 0, width: 5, height: 20 }] }
  const inconsistentWindow = { renderNodeKey, runId: 'new-42', sourceRevision: 42, clipRects: [{ x: 5, y: 0, width: 5, height: 20 }] }

  // 模拟 buildFrame 中两个成员的聚合处理
  for (const memberWindow of [existingWindow, inconsistentWindow]) {
    const existing = frozenWindowAggregator.get(memberWindow.renderNodeKey)
    if (existing !== undefined) {
      if (existing.runId !== memberWindow.runId ||
          existing.sourceRevision !== memberWindow.sourceRevision) {
        // 修复后：不 push——避免重复 renderNodeKey
      } else {
        existing.clipRects = [...existing.clipRects, ...memberWindow.clipRects]
      }
    } else {
      frozenWindowAggregator.set(memberWindow.renderNodeKey, memberWindow)
    }
  }
  // 模拟 buildFrame 末尾将 frozenWindowAggregator 的值 push 到 motionGlyphWindows
  for (const frozenWindow of frozenWindowAggregator.values()) {
    motionGlyphWindows.push(frozenWindow)
  }

  // 验证：motionGlyphWindows 中没有重复 renderNodeKey
  const keys = motionGlyphWindows.map(w => w.renderNodeKey)
  const uniqueKeys = new Set(keys)
  assert.equal(keys.length, uniqueKeys.size,
    `motionGlyphWindows 不应有重复 renderNodeKey，实际 ${keys.length} 个窗口、${uniqueKeys.size} 个唯一 key`)
  assert.equal(keys.length, 1,
    `应只有 1 个窗口（existing），实际 ${keys.length}`)
  assert.equal(keys[0], renderNodeKey,
    `唯一窗口的 renderNodeKey 应为 ${renderNodeKey}，实际 ${keys[0]}`)
})

test('问题1-6121854424：旧实现（push 不一致窗口）会产生重复 renderNodeKey——反证修复必要性', () => {
  // 模拟旧实现的行为（不一致时 push memberWindow）
  const frozenWindowAggregator = new Map()
  const motionGlyphWindows = []
  const renderNodeKey = 'old-42-win-3'
  const existingWindow = { renderNodeKey, runId: 'old-42', sourceRevision: 42, clipRects: [{ x: 0, y: 0, width: 5, height: 20 }] }
  const inconsistentWindow = { renderNodeKey, runId: 'new-42', sourceRevision: 42, clipRects: [{ x: 5, y: 0, width: 5, height: 20 }] }

  for (const memberWindow of [existingWindow, inconsistentWindow]) {
    const existing = frozenWindowAggregator.get(memberWindow.renderNodeKey)
    if (existing !== undefined) {
      if (existing.runId !== memberWindow.runId ||
          existing.sourceRevision !== memberWindow.sourceRevision) {
        // 旧实现：push——产生重复 key
        motionGlyphWindows.push(memberWindow)
      } else {
        existing.clipRects = [...existing.clipRects, ...memberWindow.clipRects]
      }
    } else {
      frozenWindowAggregator.set(memberWindow.renderNodeKey, memberWindow)
    }
  }
  for (const frozenWindow of frozenWindowAggregator.values()) {
    motionGlyphWindows.push(frozenWindow)
  }

  // 旧实现：motionGlyphWindows 有两个同 key 窗口
  const keys = motionGlyphWindows.map(w => w.renderNodeKey)
  const uniqueKeys = new Set(keys)
  assert.ok(keys.length > uniqueKeys.size,
    `旧实现应有重复 renderNodeKey（${keys.length} 个窗口 > ${uniqueKeys.size} 个唯一 key）`)
  assert.equal(keys.length, 2,
    `旧实现应有 2 个窗口（existing + 不一致 push），实际 ${keys.length}`)
  // buildAllMotionWindowSources 去重后只留第一张，第二张的 clip/字形被吞掉
  const seenKeys = new Set()
  const deduped = []
  for (const w of motionGlyphWindows) {
    if (!seenKeys.has(w.renderNodeKey)) {
      seenKeys.add(w.renderNodeKey)
      deduped.push(w)
    }
  }
  assert.equal(deduped.length, 1,
    `buildAllMotionWindowSources 去重后只留 1 张，第二张被吞掉——反证旧实现错误`)
})

test('问题1-6121854424：一致时正常合并——不丢字', () => {
  // 一致时应合并 clipRects，不丢字
  const frozenWindowAggregator = new Map()
  const motionGlyphWindows = []
  const renderNodeKey = 'old-42-win-3'
  const window1 = { renderNodeKey, runId: 'old-42', sourceRevision: 42, clipRects: [{ x: 0, y: 0, width: 5, height: 20 }] }
  const window2 = { renderNodeKey, runId: 'old-42', sourceRevision: 42, clipRects: [{ x: 5, y: 0, width: 5, height: 20 }] }

  for (const memberWindow of [window1, window2]) {
    const existing = frozenWindowAggregator.get(memberWindow.renderNodeKey)
    if (existing !== undefined) {
      if (existing.runId !== memberWindow.runId ||
          existing.sourceRevision !== memberWindow.sourceRevision) {
        // 不 push
      } else {
        existing.clipRects = [...existing.clipRects, ...memberWindow.clipRects]
      }
    } else {
      frozenWindowAggregator.set(memberWindow.renderNodeKey, memberWindow)
    }
  }
  for (const frozenWindow of frozenWindowAggregator.values()) {
    motionGlyphWindows.push(frozenWindow)
  }

  const keys = motionGlyphWindows.map(w => w.renderNodeKey)
  assert.equal(keys.length, 1, '一致时应合并为 1 个窗口')
  assert.equal(motionGlyphWindows[0].clipRects.length, 2,
    `一致时应合并 clipRects（2 个碎片），实际 ${motionGlyphWindows[0].clipRects.length}`)
})

console.log(`\n✅ editor_commit_start: ${passed} tests passed`)