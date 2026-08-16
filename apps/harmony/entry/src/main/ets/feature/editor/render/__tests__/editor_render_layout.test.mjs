// editor_render_layout.test.mjs — editor_render_geometry.ts 的纯逻辑单测。
//
// 用 Node --experimental-strip-types 直接 import editor_render_geometry.ts（纯 TS，无 ArkUI 依赖）。
// 注入确定性 mock measureFn（每 UTF-16 code unit 10px），验证选区/光标/composition 下划线
// 矩形计算的数学性质：单行/多行/空选区/越界/自动交换/行尾归行。
//
// 运行：node --experimental-strip-types editor_render_layout.test.mjs

import { strict as assert } from 'node:assert'
import { layoutLines } from '../editor_layout_math.ts'
import {
  toLineLayouts,
  computeSelectionRects,
  computeCaretRect,
  computeCompositionUnderlineRects,
  CARET_WIDTH_PX,
  UNDERLINE_HEIGHT_PX,
} from '../editor_render_geometry.ts'

// 确定性 mock measureFn：每 UTF-16 code unit 10px。满足 measure('')===0 与单调不减。
const mockMeasure = (s) => s.length * 10

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

console.log('editor_render_geometry 纯逻辑单测')
console.log('---')

// ── 常量 ──
test('常量: CARET_WIDTH_PX === 2', () => {
  assert.equal(CARET_WIDTH_PX, 2)
})
test('常量: UNDERLINE_HEIGHT_PX === 2', () => {
  assert.equal(UNDERLINE_HEIGHT_PX, 2)
})

// ── toLineLayouts ──
test('toLineLayouts: 空数组返回 []', () => {
  assert.deepEqual(toLineLayouts([], 20), [])
})
test('toLineLayouts: 单行 y=0 height=lineSpacing', () => {
  const ls = toLineLayouts([{ start: 0, end: 3 }], 20)
  assert.deepEqual(ls, [{ startUtf16: 0, endUtf16: 3, y: 0, height: 20 }])
})
test('toLineLayouts: 多行 y = i * lineSpacing', () => {
  const ls = toLineLayouts([{ start: 0, end: 2 }, { start: 2, end: 4 }, { start: 4, end: 6 }], 20)
  assert.deepEqual(ls, [
    { startUtf16: 0, endUtf16: 2, y: 0, height: 20 },
    { startUtf16: 2, endUtf16: 4, y: 20, height: 20 },
    { startUtf16: 4, endUtf16: 6, y: 40, height: 20 },
  ])
})
test('toLineLayouts: lineSpacingPx<=0 退化 y=0 height=0', () => {
  const ls = toLineLayouts([{ start: 0, end: 2 }, { start: 2, end: 4 }], 0)
  assert.deepEqual(ls, [
    { startUtf16: 0, endUtf16: 2, y: 0, height: 0 },
    { startUtf16: 2, endUtf16: 4, y: 0, height: 0 },
  ])
})

// ── computeSelectionRects ──
test('computeSelectionRects: 空文本返回 []', () => {
  assert.deepEqual(computeSelectionRects('', [], 20, 0, 0, mockMeasure), [])
})
test('computeSelectionRects: 空选区返回 []', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  assert.deepEqual(computeSelectionRects('abcdef', lines, 20, 2, 2, mockMeasure), [])
})
test("computeSelectionRects: selStart>selEnd 自动交换成 [1,3)", () => {
  const lines = layoutLines("abcdef", 1000, mockMeasure)
  const rects = computeSelectionRects("abcdef", lines, 20, 3, 1, mockMeasure)
  assert.deepEqual(rects, [{ x: 10, y: 0, width: 20, height: 20 }])
})
test('computeSelectionRects: 单行选区', () => {
  // text='abcdef', 容器 1000px 不折行，selStart=1, selEnd=3
  // x = measure('a') = 10, width = measure('bc') = 20, y=0, height=20
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const rects = computeSelectionRects('abcdef', lines, 20, 1, 3, mockMeasure)
  assert.deepEqual(rects, [{ x: 10, y: 0, width: 20, height: 20 }])
})
test('computeSelectionRects: 多行选区每行一个 rect', () => {
  // text='abcdef', 容器 25px 每行 2 字符 → 3 行：[0,2)[2,4)[4,6)
  // selStart=1, selEnd=5：
  //   行0: [1,2) x=measure('a')=10, width=measure('b')=10, y=0
  //   行1: [2,4) x=measure('')=0,  width=measure('cd')=20, y=20
  //   行2: [4,5) x=measure('')=0,  width=measure('e')=10,  y=40
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const rects = computeSelectionRects('abcdef', lines, 20, 1, 5, mockMeasure)
  assert.deepEqual(rects, [
    { x: 10, y: 0, width: 10, height: 20 },
    { x: 0, y: 20, width: 20, height: 20 },
    { x: 0, y: 40, width: 10, height: 20 },
  ])
})
test('computeSelectionRects: selStart>selEnd 自动交换', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const r1 = computeSelectionRects('abcdef', lines, 20, 1, 3, mockMeasure)
  const r2 = computeSelectionRects('abcdef', lines, 20, 3, 1, mockMeasure)
  assert.deepEqual(r1, r2)
})
test('computeSelectionRects: 选区起点越界 clamp 到行首', () => {
  // selStart=-5, selEnd=2 → start=0, end=2 → x=0, width=20
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const rects = computeSelectionRects('abcdef', lines, 20, -5, 2, mockMeasure)
  assert.deepEqual(rects, [{ x: 0, y: 0, width: 20, height: 20 }])
})
test('computeSelectionRects: lineSpacingPx<=0 退化 y=0 height=0', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const rects = computeSelectionRects('abcdef', lines, 0, 1, 3, mockMeasure)
  assert.deepEqual(rects, [{ x: 10, y: 0, width: 20, height: 0 }])
})
test('computeSelectionRects: 中文选区', () => {
  // '你好世界' 容器 25px 每行 2 字 → 2 行：[0,2)[2,4)
  // selStart=0, selEnd=3：
  //   行0: [0,2) x=0, width=measure('你好')=20, y=0
  //   行1: [2,3) x=0, width=measure('世')=10, y=20
  const lines = layoutLines('你好世界', 25, mockMeasure)
  const rects = computeSelectionRects('你好世界', lines, 20, 0, 3, mockMeasure)
  assert.deepEqual(rects, [
    { x: 0, y: 0, width: 20, height: 20 },
    { x: 0, y: 20, width: 10, height: 20 },
  ])
})

// ── computeCaretRect ──
test('computeCaretRect: 空文本返回 null', () => {
  assert.equal(computeCaretRect('', [], 20, 0, mockMeasure), null)
})
test('computeCaretRect: 光标在行首 x=0', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 0, mockMeasure)
  assert.deepEqual(caret, { x: 0, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 光标在行中', () => {
  // cursor=2 → x=measure('ab')=20
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 2, mockMeasure)
  assert.deepEqual(caret, { x: 20, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 光标在行尾（单行）', () => {
  // cursor=6 → x=measure('abcdef')=60
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 6, mockMeasure)
  assert.deepEqual(caret, { x: 60, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 多行光标在行尾归到该行', () => {
  // text='abcdef' 容器 25px → 3 行 [0,2)[2,4)[4,6)
  // cursor=2 → line.end=2>=2 归到行0，x=measure('ab')=20, y=0
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 2, mockMeasure)
  assert.deepEqual(caret, { x: 20, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 多行光标在行中归到对应行', () => {
  // cursor=3 → line.end=4>=3 归到行1，x=measure(text.substring(2,3))=measure('c')=10, y=20
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 3, mockMeasure)
  assert.deepEqual(caret, { x: 10, y: 20, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: cursor=4 归到行1行尾', () => {
  // cursor=4 → line.end=4>=4 归到行1，x=measure('cd')=20, y=20
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 4, mockMeasure)
  assert.deepEqual(caret, { x: 20, y: 20, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: cursor=5 归到行2', () => {
  // cursor=5 → line.end=6>=5 归到行2，x=measure('e')=10, y=40
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 5, mockMeasure)
  assert.deepEqual(caret, { x: 10, y: 40, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 越界 cursor<0 clamp 到 0', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, -5, mockMeasure)
  assert.deepEqual(caret, { x: 0, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: 越界 cursor>text.length clamp 到行尾', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 100, mockMeasure)
  assert.deepEqual(caret, { x: 60, y: 0, width: CARET_WIDTH_PX, height: 20 })
})
test('computeCaretRect: lineSpacingPx<=0 退化 y=0 height=0', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 0, 2, mockMeasure)
  assert.deepEqual(caret, { x: 20, y: 0, width: CARET_WIDTH_PX, height: 0 })
})

// ── computeCompositionUnderlineRects ──
test('computeCompositionUnderlineRects: 空文本返回 []', () => {
  assert.deepEqual(computeCompositionUnderlineRects('', [], 20, 0, 0, mockMeasure), [])
})
test('computeCompositionUnderlineRects: null start 返回 []', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  assert.deepEqual(computeCompositionUnderlineRects('abcdef', lines, 20, null, 3, mockMeasure), [])
})
test('computeCompositionUnderlineRects: null end 返回 []', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  assert.deepEqual(computeCompositionUnderlineRects('abcdef', lines, 20, 1, null, mockMeasure), [])
})
test('computeCompositionUnderlineRects: 空范围返回 []', () => {
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  assert.deepEqual(computeCompositionUnderlineRects('abcdef', lines, 20, 2, 2, mockMeasure), [])
})
test("computeCompositionUnderlineRects: compStart>compEnd 自动交换成 [1,3)", () => {
  const lines = layoutLines("abcdef", 1000, mockMeasure)
  const rects = computeCompositionUnderlineRects("abcdef", lines, 20, 3, 1, mockMeasure)
  assert.deepEqual(rects, [{ x: 10, y: 18, width: 20, height: UNDERLINE_HEIGHT_PX }])
})
test('computeCompositionUnderlineRects: 单行 composition 下划线在行底', () => {
  // text='abcdef' 容器 1000px 单行，compStart=1, compEnd=3, lineSpacing=20
  // x=measure('a')=10, width=measure('bc')=20
  // y = 0*20 + 20 - 2 = 18, height = 2
  const lines = layoutLines('abcdef', 1000, mockMeasure)
  const rects = computeCompositionUnderlineRects('abcdef', lines, 20, 1, 3, mockMeasure)
  assert.deepEqual(rects, [{ x: 10, y: 18, width: 20, height: UNDERLINE_HEIGHT_PX }])
})
test('computeCompositionUnderlineRects: 多行 composition 每行一个下划线', () => {
  // text='abcdef' 容器 25px → 3 行 [0,2)[2,4)[4,6)
  // compStart=1, compEnd=5, lineSpacing=20：
  //   行0: x=10, y=0+20-2=18,  width=10
  //   行1: x=0,  y=20+20-2=38, width=20
  //   行2: x=0,  y=40+20-2=58, width=10
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const rects = computeCompositionUnderlineRects('abcdef', lines, 20, 1, 5, mockMeasure)
  assert.deepEqual(rects, [
    { x: 10, y: 18, width: 10, height: UNDERLINE_HEIGHT_PX },
    { x: 0, y: 38, width: 20, height: UNDERLINE_HEIGHT_PX },
    { x: 0, y: 58, width: 10, height: UNDERLINE_HEIGHT_PX },
  ])
})
test("computeCompositionUnderlineRects: compStart>compEnd 自动交换", () => {
  const lines = layoutLines("abcdef", 1000, mockMeasure)
  const r1 = computeCompositionUnderlineRects("abcdef", lines, 20, 1, 3, mockMeasure)
  const r2 = computeCompositionUnderlineRects("abcdef", lines, 20, 3, 1, mockMeasure)
  // compStart>compEnd 自动交换，r1 和 r2 应相等
  assert.deepEqual(r1, r2)
  assert.deepEqual(r1, [{ x: 10, y: 18, width: 20, height: UNDERLINE_HEIGHT_PX }])
})
test('computeCompositionUnderlineRects: 中文 composition', () => {
  // '你好世界' 容器 25px → 2 行 [0,2)[2,4)
  // compStart=0, compEnd=3, lineSpacing=20：
  //   行0: x=0, y=18, width=20
  //   行1: x=0, y=38, width=10
  const lines = layoutLines('你好世界', 25, mockMeasure)
  const rects = computeCompositionUnderlineRects('你好世界', lines, 20, 0, 3, mockMeasure)
  assert.deepEqual(rects, [
    { x: 0, y: 18, width: 20, height: UNDERLINE_HEIGHT_PX },
    { x: 0, y: 38, width: 10, height: UNDERLINE_HEIGHT_PX },
  ])
})

// ── 端到端：snapshot 几何一致性 ──
test('端到端: 选区/光标/composition 共存于多行文本', () => {
  // text='abcdef' 容器 25px → 3 行
  // 选区 [1,5)，光标 cursor=5，composition [1,5)
  const lines = layoutLines('abcdef', 25, mockMeasure)
  const sel = computeSelectionRects('abcdef', lines, 20, 1, 5, mockMeasure)
  const caret = computeCaretRect('abcdef', lines, 20, 5, mockMeasure)
  const comp = computeCompositionUnderlineRects('abcdef', lines, 20, 1, 5, mockMeasure)
  // 选区 3 个 rect（每行一个）
  assert.equal(sel.length, 3)
  // 光标在行2，y=40
  assert.equal(caret.y, 40)
  // composition 3 个下划线
  assert.equal(comp.length, 3)
  // composition 下划线 y = 选区 y + lineSpacing - UNDERLINE_HEIGHT
  for (let i = 0; i < sel.length; i++) {
    assert.equal(comp[i].y, sel[i].y + 20 - UNDERLINE_HEIGHT_PX)
    assert.equal(comp[i].x, sel[i].x)
    assert.equal(comp[i].width, sel[i].width)
  }
})

test('端到端: emoji surrogate pair 选区不切断', () => {
  // 'a😀b' 容器 15px → 3 行 [0,1)[1,3)[3,4)
  // 选区 [0,3) 跨 emoji：行0 [0,1)，行1 [1,3)（含完整 emoji）
  const lines = layoutLines('a😀b', 15, mockMeasure)
  const sel = computeSelectionRects('a😀b', lines, 20, 0, 3, mockMeasure)
  assert.equal(sel.length, 2)
  // 行0: x=0, width=measure('a')=10
  assert.equal(sel[0].x, 0)
  assert.equal(sel[0].width, 10)
  // 行1: x=0, width=measure('😀')=20（2 code units * 10）
  assert.equal(sel[1].x, 0)
  assert.equal(sel[1].width, 20)
})

console.log('---')
console.log(`✅ editor_render_geometry: ${passed} tests passed`)
