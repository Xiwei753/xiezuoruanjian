// editor_layout_math.test.mjs — editor_layout_math.ts 的纯逻辑单测。
//
// Issue #629 评论18：统一 layout source + hard break + VisualCaretPosition。
// - layoutLines 返回 LineRange[] 含 breakKind（SoftWrap / HardBreak / EndOfText）
// - hitTestPoint 返回 VisualCaretPosition（含 CaretAffinity）
// - buildLineCaretStops / horizontalForOffset / offsetForHorizontal 统一算法
//
// 运行：node --experimental-strip-types editor_layout_math.test.mjs

import { strict as assert } from 'node:assert'
import {
  layoutLines,
  hitTestPoint,
  nextCodePointBoundary,
  buildLineCaretStops,
  horizontalForOffset,
  offsetForHorizontal,
  resolveVisualLineIndex,
  LineBreakKind,
  CaretAffinity,
} from '../editor_layout_math.ts'

// 确定性 mock measureFn：每 UTF-16 code unit 10px。
const mockMeasure = (s) => s.length * 10

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

console.log('editor_layout_math 纯逻辑单测（Issue #629 评论18：统一 layout source）')
console.log('---')

// ── nextCodePointBoundary ──
test('nextCodePointBoundary: ASCII 推进 1', () => {
  assert.equal(nextCodePointBoundary('abc', 0), 1)
})
test('nextCodePointBoundary: 中文 BMP 推进 1', () => {
  assert.equal(nextCodePointBoundary('你好', 0), 1)
})
test('nextCodePointBoundary: emoji surrogate pair 推进 2', () => {
  assert.equal(nextCodePointBoundary('😀', 0), 2)
})
test('nextCodePointBoundary: 从 emoji 起始跳 2', () => {
  assert.equal(nextCodePointBoundary('a😀b', 1), 3)
})
test('nextCodePointBoundary: 越界返回 n', () => {
  assert.equal(nextCodePointBoundary('abc', 3), 3)
})
test('nextCodePointBoundary: 负数 clamp 到 0 后推进', () => {
  assert.equal(nextCodePointBoundary('abc', -1), 1)
})

// ── layoutLines ──
test('layoutLines: 空文本返回 EndOfText', () => {
  const lines = layoutLines('', 100, mockMeasure)
  assert.equal(lines.length, 1)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 0)
  assert.equal(lines[0].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: 单行不折行', () => {
  const lines = layoutLines('abc', 1000, mockMeasure)
  assert.equal(lines.length, 1)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 3)
  assert.equal(lines[0].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: 多行自动折行（每行 2 字符）', () => {
  const lines = layoutLines('abcdef', 25, mockMeasure)
  assert.equal(lines.length, 3)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 2)
  assert.equal(lines[0].breakKind, LineBreakKind.SoftWrap)
  assert.equal(lines[1].start, 2)
  assert.equal(lines[1].end, 4)
  assert.equal(lines[1].breakKind, LineBreakKind.SoftWrap)
  assert.equal(lines[2].start, 4)
  assert.equal(lines[2].end, 6)
  assert.equal(lines[2].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: 中文折行', () => {
  const lines = layoutLines('你好世界', 25, mockMeasure)
  assert.equal(lines.length, 2)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 2)
  assert.equal(lines[0].breakKind, LineBreakKind.SoftWrap)
  assert.equal(lines[1].start, 2)
  assert.equal(lines[1].end, 4)
  assert.equal(lines[1].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: surrogate pair 不切断（emoji 独占一行）', () => {
  const lines = layoutLines('a😀b', 15, mockMeasure)
  assert.equal(lines.length, 3)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 1)
  assert.equal(lines[1].start, 1)
  assert.equal(lines[1].end, 3)
  assert.equal(lines[2].start, 3)
  assert.equal(lines[2].end, 4)
})
test('layoutLines: 单字符超容器宽度独占一行', () => {
  const lines = layoutLines('abc', 5, mockMeasure)
  assert.equal(lines.length, 3)
})
test('layoutLines: containerWidth<=0 退化每行一个 code point', () => {
  const lines = layoutLines('ab', 0, mockMeasure)
  assert.equal(lines.length, 2)
})
test('layoutLines: 行区间首尾相接且覆盖全文', () => {
  const lines = layoutLines('abcdef', 25, mockMeasure)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[lines.length - 1].end, 6)
  for (let k = 0; k < lines.length - 1; k++) {
    assert.equal(lines[k].end, lines[k + 1].start, `行 ${k} 与 ${k + 1} 不相接`)
  }
})

// ── Issue #629 评论18：hard break 测试 ──
test('layoutLines: a\\nb → HardBreak + EndOfText', () => {
  const lines = layoutLines('a\nb', 1000, mockMeasure)
  assert.equal(lines.length, 2)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 1)
  assert.equal(lines[0].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[1].start, 2)
  assert.equal(lines[1].end, 3)
  assert.equal(lines[1].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: a\\n\\nb → HardBreak + HardBreak + EndOfText', () => {
  const lines = layoutLines('a\n\nb', 1000, mockMeasure)
  assert.equal(lines.length, 3)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 1)
  assert.equal(lines[0].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[1].start, 2)
  assert.equal(lines[1].end, 2)
  assert.equal(lines[1].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[2].start, 3)
  assert.equal(lines[2].end, 4)
  assert.equal(lines[2].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: a\\n → HardBreak + EndOfText (text ends with \\n)', () => {
  const lines = layoutLines('a\n', 1000, mockMeasure)
  assert.equal(lines.length, 2)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 1)
  assert.equal(lines[0].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[1].start, 2)
  assert.equal(lines[1].end, 2)
  assert.equal(lines[1].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: 空行 \\n → HardBreak + EndOfText', () => {
  const lines = layoutLines('\n', 1000, mockMeasure)
  assert.equal(lines.length, 2)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 0)
  assert.equal(lines[0].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[1].start, 1)
  assert.equal(lines[1].end, 1)
  assert.equal(lines[1].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: 硬换行后再软折行', () => {
  // 'abcdef\ng' — 第一行 6 字符（容器宽 25px，2 字符/行 → 软折 3 行）+ 硬换行 + g
  const lines = layoutLines('abcdef\ng', 25, mockMeasure)
  // 硬换行前：'abcdef' 软折 → [0,2 SoftWrap] [2,4 SoftWrap] [4,6 HardBreak]
  // 硬换行后：'g' → [7,7+1=7? no...] Let me check:
  // 'abcdef\ng': segments = ['abcdef', 'g']
  // 'abcdef': bounds [0,1,2,3,4,5,6], soft wrap: [0,2 SoftWrap] [2,4 SoftWrap] [4,6 HardBreak]
  // 'g': bounds [7,8], soft wrap: [7,8 EndOfText]
  // Total: 4 lines
  assert.equal(lines.length, 4)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 2)
  assert.equal(lines[0].breakKind, LineBreakKind.SoftWrap)
  assert.equal(lines[1].start, 2)
  assert.equal(lines[1].end, 4)
  assert.equal(lines[1].breakKind, LineBreakKind.SoftWrap)
  assert.equal(lines[2].start, 4)
  assert.equal(lines[2].end, 6)
  assert.equal(lines[2].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[3].start, 7)
  assert.equal(lines[3].end, 8)
  assert.equal(lines[3].breakKind, LineBreakKind.EndOfText)
})

// ── hitTestPoint（返回 VisualCaretPosition）──
test('hitTestPoint: 空文本返回 offset 0', () => {
  const result = hitTestPoint('', [], 20, 5, 5, mockMeasure)
  assert.equal(result.utf16Offset, 0)
  assert.equal(result.affinity, CaretAffinity.Downstream)
})
test('hitTestPoint: touchY 算行号（第 1 行行首）', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  const result = hitTestPoint('abcdef', hl, 20, 5, 25, mockMeasure)
  assert.equal(result.utf16Offset, 2)
})
test('hitTestPoint: touchX 行内命中左边界', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  const result = hitTestPoint('abcdef', hl, 20, 15, 5, mockMeasure)
  assert.equal(result.utf16Offset, 1)
})
test('hitTestPoint: touchX 行内命中右边界', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  const result = hitTestPoint('abcdef', hl, 20, 16, 5, mockMeasure)
  assert.equal(result.utf16Offset, 2)
})
test('hitTestPoint: 越界 clamp 到行首', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  const result = hitTestPoint('abcdef', hl, 20, -5, -5, mockMeasure)
  assert.equal(result.utf16Offset, 0)
})
test('hitTestPoint: 越界 clamp 到行尾', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  const result = hitTestPoint('abcdef', hl, 20, 1000, 1000, mockMeasure)
  assert.equal(result.utf16Offset, 6)
})
test('hitTestPoint: 中文命中行首', () => {
  const cl = layoutLines('你好世界', 25, mockMeasure)
  const result = hitTestPoint('你好世界', cl, 20, 5, 5, mockMeasure)
  assert.equal(result.utf16Offset, 0)
})
test('hitTestPoint: 中文命中行内', () => {
  const cl = layoutLines('你好世界', 25, mockMeasure)
  const result = hitTestPoint('你好世界', cl, 20, 15, 5, mockMeasure)
  assert.equal(result.utf16Offset, 1)
})
test('hitTestPoint: surrogate pair 命中不切断（返回 emoji 起始）', () => {
  const el = layoutLines('a😀b', 15, mockMeasure)
  const result = hitTestPoint('a😀b', el, 20, 5, 25, mockMeasure)
  assert.equal(result.utf16Offset, 1)
})
test('hitTestPoint: lineSpacingPx<=0 按第 0 行处理', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  const result = hitTestPoint('abcdef', hl, 0, 5, 25, mockMeasure)
  assert.equal(result.utf16Offset, 0)
})

// ── Issue #629 R7-C item4：soft-wrap affinity 测试 ──
// touchY 已选定 lineIndex，命中该行 soft-wrap 末尾恒为 Upstream；下一行 start 自然 Downstream。
test('hitTestPoint: soft-wrap 末尾 touchY 在行上半 → Upstream', () => {
  // 'abcdef' → 软折 2 行：[0,3] [3,6]，每行 20px
  const lines = layoutLines('abcdef', 30, mockMeasure)
  assert.equal(lines.length, 2)
  assert.equal(lines[0].breakKind, LineBreakKind.SoftWrap)
  // touchY=5 → lineIndex=0，x=28 接近行尾 offset=3 → Upstream
  const resultUp = hitTestPoint('abcdef', lines, 20, 28, 5, mockMeasure)
  assert.equal(resultUp.utf16Offset, 3)
  assert.equal(resultUp.affinity, CaretAffinity.Upstream)
})
test('hitTestPoint: soft-wrap 末尾 touchY 在行下半仍返回 Upstream（不再按上/下半区分）', () => {
  const lines = layoutLines('abcdef', 30, mockMeasure)
  // touchY=15 → lineIndex=0（行 y=0..20），x=28 接近行尾 offset=3
  // 命中的是第 0 行 soft-wrap 末尾 → 恒为 Upstream
  const resultDown = hitTestPoint('abcdef', lines, 20, 28, 15, mockMeasure)
  assert.equal(resultDown.utf16Offset, 3)
  assert.equal(resultDown.affinity, CaretAffinity.Upstream)
})
test('hitTestPoint: soft-wrap 下一行 start 返回 Downstream', () => {
  const lines = layoutLines('abcdef', 30, mockMeasure)
  // touchY=25 → lineIndex=1（行 y=20..40），x=5 → offset=3（第 1 行行首）
  // 命中的是 soft-wrap 后第 1 行行首 → Downstream
  const result = hitTestPoint('abcdef', lines, 20, 5, 25, mockMeasure)
  assert.equal(result.utf16Offset, 3)
  assert.equal(result.affinity, CaretAffinity.Downstream)
})
test('hitTestPoint: 同一 soft-wrap offset 上下半命中终点都 Upstream', () => {
  // 回归测试：旧实现按上/下半区分导致 touchY 在下半时返回 Downstream。
  // 新实现：touchY 已选定 lineIndex，命中该行 soft-wrap 末尾恒为 Upstream。
  const lines = layoutLines('abcdef', 30, mockMeasure)
  assert.equal(lines.length, 2)
  // touchY=2 → lineIndex=0，x=30 → offset=3 → Upstream
  const r1 = hitTestPoint('abcdef', lines, 20, 30, 2, mockMeasure)
  assert.equal(r1.utf16Offset, 3)
  assert.equal(r1.affinity, CaretAffinity.Upstream)
  // touchY=18 → lineIndex=0（行 y=0..20），x=30 → offset=3 → Upstream
  const r2 = hitTestPoint('abcdef', lines, 20, 30, 18, mockMeasure)
  assert.equal(r2.utf16Offset, 3)
  assert.equal(r2.affinity, CaretAffinity.Upstream)
})
test('hitTestPoint: hard break 两侧 offset 不同，不靠 affinity', () => {
  // 'a\nb' → [0,1 HardBreak] [2,3 EndOfText]
  const lines = layoutLines('a\nb', 1000, mockMeasure)
  // 点击第 0 行末尾区域（x=8 接近 offset 1 的 x=10）
  const result0 = hitTestPoint('a\nb', lines, 20, 8, 5, mockMeasure)
  assert.equal(result0.utf16Offset, 1)
  // 点击第 1 行行首（offset=2）
  const result1 = hitTestPoint('a\nb', lines, 20, 5, 25, mockMeasure)
  assert.equal(result1.utf16Offset, 2)
  // hard break 两侧 offset 不同（1 vs 2），不靠 affinity 区分
})

// ── 端到端：坐标 → UTF-16 offset → 字符验证 ──
test('端到端: 中文多行点击第 2 行第 1 字命中"世"', () => {
  const cl = layoutLines('你好世界', 25, mockMeasure)
  const result = hitTestPoint('你好世界', cl, 20, 5, 25, mockMeasure)
  assert.equal('你好世界'.substring(result.utf16Offset, result.utf16Offset + 1), '世')
})
test('端到端: emoji 点击命中完整 emoji 不切断', () => {
  const el = layoutLines('a😀b', 15, mockMeasure)
  const result = hitTestPoint('a😀b', el, 20, 10, 25, mockMeasure)
  assert.equal('a😀b'.substring(result.utf16Offset, result.utf16Offset + 2), '😀')
})

// ── buildLineCaretStops ──
test('buildLineCaretStops: 空行返回只有行首的 stop', () => {
  const stops = buildLineCaretStops('abc', { start: 0, end: 0 }, mockMeasure)
  assert.equal(stops.length, 1)
  assert.equal(stops[0].utf16Offset, 0)
  assert.equal(stops[0].x, 0)
})
test('buildLineCaretStops: ASCII 行生成每个 code point 边界的 stop', () => {
  const stops = buildLineCaretStops('abc', { start: 0, end: 3 }, mockMeasure)
  assert.equal(stops.length, 4)
  assert.equal(stops[0].utf16Offset, 0)
  assert.equal(stops[0].x, 0)
  assert.equal(stops[1].utf16Offset, 1)
  assert.equal(stops[1].x, 10)
  assert.equal(stops[2].utf16Offset, 2)
  assert.equal(stops[2].x, 20)
  assert.equal(stops[3].utf16Offset, 3)
  assert.equal(stops[3].x, 30)
})
test('buildLineCaretStops: 中文生成正确 stops', () => {
  const stops = buildLineCaretStops('你好', { start: 0, end: 2 }, mockMeasure)
  assert.equal(stops.length, 3)
  assert.equal(stops[0].utf16Offset, 0)
  assert.equal(stops[1].utf16Offset, 1)
  assert.equal(stops[2].utf16Offset, 2)
})
test('buildLineCaretStops: surrogate pair 不切断', () => {
  const stops = buildLineCaretStops('a😀b', { start: 0, end: 4 }, mockMeasure)
  assert.equal(stops.length, 4)
  assert.equal(stops[0].utf16Offset, 0)
  assert.equal(stops[1].utf16Offset, 1)
  assert.equal(stops[2].utf16Offset, 3)
  assert.equal(stops[3].utf16Offset, 4)
})
test('buildLineCaretStops: 行内偏移（从行首开始测量）', () => {
  const stops = buildLineCaretStops('abc', { start: 3, end: 3 }, mockMeasure)
  assert.equal(stops.length, 1)
  assert.equal(stops[0].utf16Offset, 3)
  assert.equal(stops[0].x, 0)
})
test('buildLineCaretStops: 行末 stop 的 x 等于行宽', () => {
  const stops = buildLineCaretStops('你好世界', { start: 0, end: 2 }, mockMeasure)
  assert.equal(stops[stops.length - 1].utf16Offset, 2)
  assert.equal(stops[stops.length - 1].x, 20)
})

// ── Issue #629 评论18：horizontalForOffset / offsetForHorizontal ──
test('horizontalForOffset: 找到精确 offset 的 x', () => {
  const stops = buildLineCaretStops('abc', { start: 0, end: 3 }, mockMeasure)
  assert.equal(horizontalForOffset(stops, 0), 0)
  assert.equal(horizontalForOffset(stops, 1), 10)
  assert.equal(horizontalForOffset(stops, 2), 20)
  assert.equal(horizontalForOffset(stops, 3), 30)
})
test('horizontalForOffset: offset 不存在时找最近', () => {
  const stops = buildLineCaretStops('abc', { start: 0, end: 3 }, mockMeasure)
  // offset 1.5 → 最近是 offset 1 (x=10) 或 offset 2 (x=20)，距离相等取左
  assert.equal(horizontalForOffset(stops, 1), 10)
})
test('offsetForHorizontal: 找到 x=0 → offset 0', () => {
  const stops = buildLineCaretStops('abc', { start: 0, end: 3 }, mockMeasure)
  assert.equal(offsetForHorizontal(stops, 0), 0)
})
test('offsetForHorizontal: 找到 x=10 → offset 1', () => {
  const stops = buildLineCaretStops('abc', { start: 0, end: 3 }, mockMeasure)
  assert.equal(offsetForHorizontal(stops, 10), 1)
})
test('offsetForHorizontal: 找到 x=25 → offset 2 或 3', () => {
  const stops = buildLineCaretStops('abc', { start: 0, end: 3 }, mockMeasure)
  // x=25：offset 2 (x=20) 距离 5，offset 3 (x=30) 距离 5，等距取左
  assert.equal(offsetForHorizontal(stops, 25), 2)
})
test('offsetForHorizontal: x 超过行宽 → 最后一个 stop', () => {
  const stops = buildLineCaretStops('abc', { start: 0, end: 3 }, mockMeasure)
  assert.equal(offsetForHorizontal(stops, 1000), 3)
})

// ── resolveVisualLineIndex ──
test('resolveVisualLineIndex: 普通位置', () => {
  const lines = layoutLines('abcdef', 30, mockMeasure)
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 1, affinity: CaretAffinity.Downstream }), 0)
})
test('resolveVisualLineIndex: soft-wrap Upstream', () => {
  const lines = layoutLines('abcdef', 30, mockMeasure)
  // offset=3 在 soft-wrap 边界：Upstream → 第 0 行
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 3, affinity: CaretAffinity.Upstream }), 0)
})
test('resolveVisualLineIndex: soft-wrap Downstream', () => {
  const lines = layoutLines('abcdef', 30, mockMeasure)
  // offset=3 在 soft-wrap 边界：Downstream → 第 1 行
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 3, affinity: CaretAffinity.Downstream }), 1)
})
test('resolveVisualLineIndex: hard break offset=1 → 第 0 行（行末）', () => {
  const lines = layoutLines('a\nb', 1000, mockMeasure)
  // offset=1 是 'a' 的末尾，HardBreak → 归第 0 行
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 1, affinity: CaretAffinity.Downstream }), 0)
})
test('resolveVisualLineIndex: hard break offset=2 → 第 1 行（行首）', () => {
  const lines = layoutLines('a\nb', 1000, mockMeasure)
  // offset=2 是 'b' 的起始，归第 1 行
  assert.equal(resolveVisualLineIndex(lines, { utf16Offset: 2, affinity: CaretAffinity.Downstream }), 1)
})

// ── Issue #629 R7-C：多换行/空行/代理对/组合字符回归测试 ──
test('layoutLines: 连续多换行产生多个空 visual line', () => {
  const lines = layoutLines('a\n\n\nb', 1000, mockMeasure)
  assert.equal(lines.length, 4)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 1)
  assert.equal(lines[0].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[1].start, 2)
  assert.equal(lines[1].end, 2)
  assert.equal(lines[1].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[2].start, 3)
  assert.equal(lines[2].end, 3)
  assert.equal(lines[2].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[3].start, 4)
  assert.equal(lines[3].end, 5)
  assert.equal(lines[3].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: 多个硬换行段各自独立软折行', () => {
  // 'abcdef\nghijkl' 容器 25px → 'abcdef' 软折 3 行 + 'ghijkl' 软折 3 行 = 6 行
  const lines = layoutLines('abcdef\nghijkl', 25, mockMeasure)
  assert.equal(lines.length, 6)
  // 第一段: [0,2 SoftWrap] [2,4 SoftWrap] [4,6 HardBreak]
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 2)
  assert.equal(lines[0].breakKind, LineBreakKind.SoftWrap)
  assert.equal(lines[1].start, 2)
  assert.equal(lines[1].end, 4)
  assert.equal(lines[1].breakKind, LineBreakKind.SoftWrap)
  assert.equal(lines[2].start, 4)
  assert.equal(lines[2].end, 6)
  assert.equal(lines[2].breakKind, LineBreakKind.HardBreak)
  // 第二段: [7,9 SoftWrap] [9,11 SoftWrap] [11,13 EndOfText]（文本总长 13）
  assert.equal(lines[3].start, 7)
  assert.equal(lines[3].end, 9)
  assert.equal(lines[3].breakKind, LineBreakKind.SoftWrap)
  assert.equal(lines[4].start, 9)
  assert.equal(lines[4].end, 11)
  assert.equal(lines[4].breakKind, LineBreakKind.SoftWrap)
  assert.equal(lines[5].start, 11)
  assert.equal(lines[5].end, 13)
  assert.equal(lines[5].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: 代理对跨硬换行不切断', () => {
  // '😀\n😀' → ['😀'] (offset 0-2) + ['😀'] (offset 3-5)
  const lines = layoutLines('😀\n😀', 1000, mockMeasure)
  assert.equal(lines.length, 2)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 2)
  assert.equal(lines[0].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[1].start, 3)
  assert.equal(lines[1].end, 5)
  assert.equal(lines[1].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: 组合字符序列硬换行正确处理', () => {
  // e\u0301 (e + combining acute) = 2 UTF-16 units
  const lines = layoutLines('e\u0301\nbc', 1000, mockMeasure)
  assert.equal(lines.length, 2)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[0].end, 2)
  assert.equal(lines[0].breakKind, LineBreakKind.HardBreak)
  assert.equal(lines[1].start, 3)
  assert.equal(lines[1].end, 5)
  assert.equal(lines[1].breakKind, LineBreakKind.EndOfText)
})
test('hitTestPoint: 多换行文本各行列首/尾正确命中', () => {
  const lines = layoutLines('a\nb\nc', 1000, mockMeasure)
  assert.equal(lines.length, 3)
  // 第 0 行行尾
  const r0 = hitTestPoint('a\nb\nc', lines, 20, 8, 5, mockMeasure)
  assert.equal(r0.utf16Offset, 1)
  // 第 1 行行首
  const r1 = hitTestPoint('a\nb\nc', lines, 20, 5, 25, mockMeasure)
  assert.equal(r1.utf16Offset, 2)
  // 第 1 行行尾
  const r1e = hitTestPoint('a\nb\nc', lines, 20, 8, 25, mockMeasure)
  assert.equal(r1e.utf16Offset, 3)
  // 第 2 行行首
  const r2 = hitTestPoint('a\nb\nc', lines, 20, 5, 45, mockMeasure)
  assert.equal(r2.utf16Offset, 4)
})
test('hitTestPoint: 空行行首命中正确', () => {
  // 'a\n\nb' → [0,1 HardBreak] [2,2 HardBreak] [3,4 EndOfText]
  const lines = layoutLines('a\n\nb', 1000, mockMeasure)
  assert.equal(lines.length, 3)
  // 第 1 行是空行 (start=2, end=2)，touchY=25 → lineIndex=1
  const r = hitTestPoint('a\n\nb', lines, 20, 0, 25, mockMeasure)
  assert.equal(r.utf16Offset, 2)
  assert.equal(r.affinity, CaretAffinity.Downstream)
})
test('hitTestPoint: 空行 hit-test + caret stops 只有一个 stop', () => {
  const lines = layoutLines('a\n\nb', 1000, mockMeasure)
  // 空行 [2,2] 应只有一个 caret stop (offset=2, x=0)
  const stops = buildLineCaretStops('a\n\nb', lines[1], mockMeasure)
  assert.equal(stops.length, 1)
  assert.equal(stops[0].utf16Offset, 2)
  assert.equal(stops[0].x, 0)
})
test('hitTestPoint: soft-wrap 上一行末尾与下一行行首同一 offset 返回正确 affinity', () => {
  // 'abcdef' 容器 30px → [0,3 SoftWrap] [3,6 EndOfText]
  const lines = layoutLines('abcdef', 30, mockMeasure)
  assert.equal(lines.length, 2)
  // 命中第 0 行行尾 (lineIndex=0, offset=3) → Upstream
  const r0 = hitTestPoint('abcdef', lines, 20, 28, 5, mockMeasure)
  assert.equal(r0.utf16Offset, 3)
  assert.equal(r0.affinity, CaretAffinity.Upstream)
  // 命中第 1 行行首 (lineIndex=1, offset=3) → Downstream
  const r1 = hitTestPoint('abcdef', lines, 20, 5, 25, mockMeasure)
  assert.equal(r1.utf16Offset, 3)
  assert.equal(r1.affinity, CaretAffinity.Downstream)
})

// ── Issue #629 R7-C item5：layoutLines 复杂度结构性测试 ──
// 防止 allBounds.filter(allBounds) 模式再次出现。
// 策略：构造大量硬换行段，验证每段只生成自身范围的 bounds。
test('layoutLines: 大量换行段输出行数与预期一致（结构正确性）', () => {
  // 100 个段落，每段 10 字符，容器足够宽不软折 → 100 行 HardBreak + EndOfText
  const segs = []
  for (let i = 0; i < 100; i++) {
    segs.push('a'.repeat(10))
  }
  const text = segs.join('\n')
  const lines = layoutLines(text, 1000, mockMeasure)
  // 最后一个段落是 EndOfText，其余都是 HardBreak
  assert.equal(lines.length, 100)
  for (let i = 0; i < 99; i++) {
    assert.equal(lines[i].breakKind, LineBreakKind.HardBreak, `line ${i} should be HardBreak`)
  }
  assert.equal(lines[99].breakKind, LineBreakKind.EndOfText)
  // 验证行区间不重叠且覆盖全文（\n 本身不进 line range）
  let expected = 0
  for (let i = 0; i < 100; i++) {
    assert.equal(lines[i].start, expected, `line ${i} start`)
    assert.equal(lines[i].end, expected + 10, `line ${i} end`)
    expected += 10 + 1 // 10 字符 + 1 \n
  }
})
test('layoutLines: 大量换行段软折行输出正确', () => {
  // 50 个段落，每段 6 字符，容器 25px → 每段软折 3 行 = 150 行
  const segs = []
  for (let i = 0; i < 50; i++) {
    segs.push('abcdef')
  }
  const text = segs.join('\n')
  const lines = layoutLines(text, 25, mockMeasure)
  assert.equal(lines.length, 150)
  // 每 3 行一组：[SoftWrap] [SoftWrap] [HardBreak]，最后一组 [SoftWrap] [SoftWrap] [EndOfText]
  for (let i = 0; i < 148; i++) {
    const posInSeg = i % 3
    if (posInSeg < 2) {
      assert.equal(lines[i].breakKind, LineBreakKind.SoftWrap, `line ${i} should be SoftWrap`)
    } else {
      assert.equal(lines[i].breakKind, LineBreakKind.HardBreak, `line ${i} should be HardBreak`)
    }
  }
  assert.equal(lines[149].breakKind, LineBreakKind.EndOfText)
})
test('layoutLines: 空行密集段不会导致 bounds 溢出', () => {
  // '\n\n\n\n\n' → 5 个 HardBreak 空行 + 1 个 EndOfText 空行 = 6 行
  const lines = layoutLines('\n'.repeat(5), 1000, mockMeasure)
  assert.equal(lines.length, 6)
  for (let i = 0; i < 5; i++) {
    assert.equal(lines[i].start, i)
    assert.equal(lines[i].end, i)
    assert.equal(lines[i].breakKind, LineBreakKind.HardBreak)
  }
  assert.equal(lines[5].start, 5)
  assert.equal(lines[5].end, 5)
  assert.equal(lines[5].breakKind, LineBreakKind.EndOfText)
})

console.log('---')
console.log(`✅ editor_layout_math: ${passed} tests passed`)
