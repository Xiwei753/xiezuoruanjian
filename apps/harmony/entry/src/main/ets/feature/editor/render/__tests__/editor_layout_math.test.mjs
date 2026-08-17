// editor_layout_math.test.mjs — editor_layout_math.ts 的纯逻辑单测。
//
// 用 Node --experimental-strip-types 直接 import editor_layout_math.ts（纯 TS，无 ArkUI 依赖）。
// 注入确定性 mock measureFn（每 UTF-16 code unit 10px），验证折行/命中的数学性质：
// 多行、自动折行、Unicode 坐标命中、surrogate pair 不切断、越界 clamp。
//
// 运行：node --experimental-strip-types editor_layout_math.test.mjs

import { strict as assert } from 'node:assert'
import {
  layoutLines,
  hitTestPoint,
  nextCodePointBoundary,
  buildLineCaretStops,
} from '../editor_layout_math.ts'

// 确定性 mock measureFn：每 UTF-16 code unit 10px。满足 measure('')===0 与单调不减。
const mockMeasure = (s) => s.length * 10

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

console.log('editor_layout_math 纯逻辑单测')
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
test('layoutLines: 空文本返回 []', () => {
  assert.deepEqual(layoutLines('', 100, mockMeasure), [])
})
test('layoutLines: 单行不折行', () => {
  assert.deepEqual(layoutLines('abc', 1000, mockMeasure), [{ start: 0, end: 3 }])
})
test('layoutLines: 多行自动折行（每行 2 字符）', () => {
  assert.deepEqual(layoutLines('abcdef', 25, mockMeasure), [
    { start: 0, end: 2 },
    { start: 2, end: 4 },
    { start: 4, end: 6 },
  ])
})
test('layoutLines: 中文折行', () => {
  assert.deepEqual(layoutLines('你好世界', 25, mockMeasure), [
    { start: 0, end: 2 },
    { start: 2, end: 4 },
  ])
})
test('layoutLines: surrogate pair 不切断（emoji 独占一行）', () => {
  // 'a😀b' code point 边界 0,1,3,4。容器 15px：'a'(10) 'a😀'(30>15) → 'a' 独占；
  // '😀'(20>15) 强制独占；'b'(10) 独占。end 不落在 low surrogate(2) 前。
  assert.deepEqual(layoutLines('a😀b', 15, mockMeasure), [
    { start: 0, end: 1 },
    { start: 1, end: 3 },
    { start: 3, end: 4 },
  ])
})
test('layoutLines: 单字符超容器宽度独占一行', () => {
  assert.deepEqual(layoutLines('abc', 5, mockMeasure), [
    { start: 0, end: 1 },
    { start: 1, end: 2 },
    { start: 2, end: 3 },
  ])
})
test('layoutLines: containerWidth<=0 退化每行一个 code point', () => {
  assert.deepEqual(layoutLines('ab', 0, mockMeasure), [
    { start: 0, end: 1 },
    { start: 1, end: 2 },
  ])
})
test('layoutLines: 行区间首尾相接且覆盖全文', () => {
  const lines = layoutLines('abcdef', 25, mockMeasure)
  assert.equal(lines[0].start, 0)
  assert.equal(lines[lines.length - 1].end, 6)
  for (let k = 0; k < lines.length - 1; k++) {
    assert.equal(lines[k].end, lines[k + 1].start, `行 ${k} 与 ${k + 1} 不相接`)
  }
})

// ── hitTestPoint ──
test('hitTestPoint: 空文本返回 0', () => {
  assert.equal(hitTestPoint('', [], 20, 5, 5, mockMeasure), 0)
})
test('hitTestPoint: touchY 算行号（第 1 行行首）', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  assert.equal(hitTestPoint('abcdef', hl, 20, 5, 25, mockMeasure), 2)
})
test('hitTestPoint: touchX 行内命中左边界（中点取左）', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  // 第 0 行 'ab'，touchX=15 在 'a'(10) 与 'ab'(20) 中点，5<=5 取左 → offset 1
  assert.equal(hitTestPoint('abcdef', hl, 20, 15, 5, mockMeasure), 1)
})
test('hitTestPoint: touchX 行内命中右边界', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  // touchX=16，6>4 取右 → offset 2
  assert.equal(hitTestPoint('abcdef', hl, 20, 16, 5, mockMeasure), 2)
})
test('hitTestPoint: 越界 clamp 到行首', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  assert.equal(hitTestPoint('abcdef', hl, 20, -5, -5, mockMeasure), 0)
})
test('hitTestPoint: 越界 clamp 到行尾', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  assert.equal(hitTestPoint('abcdef', hl, 20, 1000, 1000, mockMeasure), 6)
})
test('hitTestPoint: 中文命中行首', () => {
  const cl = layoutLines('你好世界', 25, mockMeasure)
  assert.equal(hitTestPoint('你好世界', cl, 20, 5, 5, mockMeasure), 0)
})
test('hitTestPoint: 中文命中行内', () => {
  const cl = layoutLines('你好世界', 25, mockMeasure)
  assert.equal(hitTestPoint('你好世界', cl, 20, 15, 5, mockMeasure), 1)
})
test('hitTestPoint: surrogate pair 命中不切断（返回 emoji 起始）', () => {
  const el = layoutLines('a😀b', 15, mockMeasure)
  // 第 1 行 '😀'(1-3)，touchX=5 落在 emoji 前，命中 emoji 起始 offset 1（非 2）
  assert.equal(hitTestPoint('a😀b', el, 20, 5, 25, mockMeasure), 1)
})
test('hitTestPoint: lineSpacingPx<=0 按第 0 行处理', () => {
  const hl = layoutLines('abcdef', 25, mockMeasure)
  assert.equal(hitTestPoint('abcdef', hl, 0, 5, 25, mockMeasure), 0)
})

// ── 端到端：坐标 → UTF-16 offset → 字符验证 ──
test('端到端: 中文多行点击第 2 行第 1 字命中"世"', () => {
  // '你好世界'，容器 25px，每字 10px → 2 行：'你好'/'世界'。lineSpacing=20。
  // 点击第 2 行第 1 字（touchY=25, touchX=5）→ offset 2 → '世'
  const cl = layoutLines('你好世界', 25, mockMeasure)
  const offset = hitTestPoint('你好世界', cl, 20, 5, 25, mockMeasure)
  assert.equal('你好世界'.substring(offset, offset + 1), '世')
})
test('端到端: emoji 点击命中完整 emoji 不切断', () => {
  // 'a😀b'，点击 emoji 区域 → offset 1，substring(1,3)==='😀'
  const el = layoutLines('a😀b', 15, mockMeasure)
  const offset = hitTestPoint('a😀b', el, 20, 10, 25, mockMeasure)
  assert.equal('a😀b'.substring(offset, offset + 2), '😀')
})

// ── buildLineCaretStops ──
// Issue #629 评论17 第4项：caret stop 生成下沉到 editor_layout_math.ts。
test('buildLineCaretStops: 空行返回只有行首的 stop', () => {
  const stops = buildLineCaretStops('abc', { start: 0, end: 0 }, mockMeasure)
  assert.equal(stops.length, 1)
  assert.equal(stops[0].utf16Offset, 0)
  assert.equal(stops[0].x, 0)
})
test('buildLineCaretStops: ASCII 行生成每个 code point 边界的 stop', () => {
  const stops = buildLineCaretStops('abc', { start: 0, end: 3 }, mockMeasure)
  assert.equal(stops.length, 4) // 0, 1, 2, 3
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
  assert.equal(stops.length, 3) // 0, 1, 2
  assert.equal(stops[0].utf16Offset, 0)
  assert.equal(stops[1].utf16Offset, 1)
  assert.equal(stops[2].utf16Offset, 2)
})
test('buildLineCaretStops: surrogate pair 不切断（emoji 两个 code unit 算一个 stop）', () => {
  const stops = buildLineCaretStops('a😀b', { start: 0, end: 4 }, mockMeasure)
  // stops: 0(a), 1(😀起始), 3(b), 4(行尾) — 跳过 offset 2（surrogate pair 低代理）
  assert.equal(stops.length, 4)
  assert.equal(stops[0].utf16Offset, 0)
  assert.equal(stops[1].utf16Offset, 1)
  assert.equal(stops[2].utf16Offset, 3)
  assert.equal(stops[3].utf16Offset, 4)
})
test('buildLineCaretStops: 行内偏移（从行首开始测量）', () => {
  // 'abc' 第二行 start=3 end=6，x 从 0 开始（相对于行首）
  const stops = buildLineCaretStops('abc', { start: 3, end: 3 }, mockMeasure)
  assert.equal(stops.length, 1)
  assert.equal(stops[0].utf16Offset, 3)
  assert.equal(stops[0].x, 0)
})
test('buildLineCaretStops: 行末 stop 的 x 等于行宽', () => {
  const stops = buildLineCaretStops('你好世界', { start: 0, end: 2 }, mockMeasure)
  // 最后一个 stop 在 offset=2，measureText('你好') = 20px
  assert.equal(stops[stops.length - 1].utf16Offset, 2)
  assert.equal(stops[stops.length - 1].x, 20)
})

console.log('---')
console.log(`✅ editor_layout_math: ${passed} tests passed`)
