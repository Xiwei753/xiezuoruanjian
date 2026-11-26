// text_document_rules.test.mjs — 鸿蒙端统一段落规则（UTF-16 索引）的纯逻辑单测
// （Issue #1063 评论第 3 条：段落识别统一）。
//
// 用 Node --experimental-strip-types 直接 import input/text_document_rules.ts（纯 TS，无 ArkUI 依赖）。
// 验证：
//   1. listParagraphRanges 与 Core paragraph_byte_ranges 同规则：
//      分隔符是 `\n`、CRLF（两个字符算一个边界）、U+2029；孤立 CR 不是分隔符；
//      段落 end 不含分隔符（CRLF 两个字符都不计入）；连续分隔符保留空段落；
//      末尾分隔符产生末尾空段落；空正文返回空数组。
//   2. insertedParagraphStarts 对每个分隔符压入“分隔符结束处”的绝对 UTF-16 位置，
//      CRLF 只压一次（不是分别压 CR 和 LF 两个位置）。
//
// 运行：node --experimental-strip-types --test __tests__/text_document_rules.test.mjs

import { strict as assert } from 'node:assert'
import {
  listParagraphRanges,
  insertedParagraphStarts,
  describeLocalParagraphChange,
} from '../text_document_rules.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

console.log('text_document_rules：统一段落规则（UTF-16 索引）')
console.log('---')

// ── 1. listParagraphRanges：分隔符集合与段落 range ──
test('listParagraphRanges: CRLF 是一个段落边界，两个字符都不属于前一段', () => {
  assert.deepEqual(listParagraphRanges('ab\r\ncd'), [
    { start: 0, end: 2, hasTrailingNewline: true },
    { start: 4, end: 6, hasTrailingNewline: false },
  ])
})

test('listParagraphRanges: U+2029 是段落边界', () => {
  assert.deepEqual(listParagraphRanges('a\u2029b'), [
    { start: 0, end: 1, hasTrailingNewline: true },
    { start: 2, end: 3, hasTrailingNewline: false },
  ])
})

test('listParagraphRanges: 孤立 CR 不是分隔符，整段保留', () => {
  assert.deepEqual(listParagraphRanges('a\rcd'), [
    { start: 0, end: 4, hasTrailingNewline: false },
  ])
})

test('listParagraphRanges: 连续分隔符保留空段落，末尾分隔符产生末尾空段落', () => {
  assert.deepEqual(listParagraphRanges('a\n\nb\n'), [
    { start: 0, end: 1, hasTrailingNewline: true },
    { start: 2, end: 2, hasTrailingNewline: true },
    { start: 3, end: 4, hasTrailingNewline: true },
    { start: 5, end: 5, hasTrailingNewline: false },
  ])
})

test('listParagraphRanges: 空正文返回空数组', () => {
  assert.deepEqual(listParagraphRanges(''), [])
})

test('listParagraphRanges: 连续 CRLF 各算一个边界并保留中间空段落', () => {
  assert.deepEqual(listParagraphRanges('a\r\n\r\nb'), [
    { start: 0, end: 1, hasTrailingNewline: true },
    { start: 3, end: 3, hasTrailingNewline: true },
    { start: 5, end: 6, hasTrailingNewline: false },
  ])
})

test('listParagraphRanges: 混合 \\n / CRLF / U+2029，孤立 CR 仍不是边界', () => {
  assert.deepEqual(listParagraphRanges('a\rb\nc\r\nd\u2029e'), [
    { start: 0, end: 3, hasTrailingNewline: true },  // 'a\rb'（CR 是内容）
    { start: 4, end: 5, hasTrailingNewline: true },  // 'c'
    { start: 7, end: 8, hasTrailingNewline: true },  // 'd'
    { start: 9, end: 10, hasTrailingNewline: false }, // 'e'
  ])
})

test('listParagraphRanges: 只有 CRLF 时产生一个前空段和一个末尾空段', () => {
  assert.deepEqual(listParagraphRanges('\r\n'), [
    { start: 0, end: 0, hasTrailingNewline: true },
    { start: 2, end: 2, hasTrailingNewline: false },
  ])
})

// ── 2. insertedParagraphStarts：分隔符结束处的绝对 UTF-16 位置 ──
test('insertedParagraphStarts: 无分隔符返回空数组', () => {
  assert.deepEqual(insertedParagraphStarts('abc', 0), [])
})

test('insertedParagraphStarts: LF / CRLF / U+2029 各压一个结束位置', () => {
  assert.deepEqual(insertedParagraphStarts('a\nb', 0), [2])
  assert.deepEqual(insertedParagraphStarts('a\r\nb', 0), [3])
  assert.deepEqual(insertedParagraphStarts('a\u2029b', 0), [2])
})

test('insertedParagraphStarts: 插入起点是绝对 UTF-16 偏移', () => {
  assert.deepEqual(insertedParagraphStarts('a\r\nb', 10), [13])
})

test('insertedParagraphStarts: 连续分隔符各压一个位置', () => {
  assert.deepEqual(insertedParagraphStarts('a\n\n', 0), [2, 3])
})

test('insertedParagraphStarts: 纯 CRLF 压分隔符结束位置 2', () => {
  assert.deepEqual(insertedParagraphStarts('\r\n', 0), [2])
})

test('insertedParagraphStarts: CRLF 只产生一个起点（不是 [2, 3]）', () => {
  const starts = insertedParagraphStarts('a\r\nb', 0)
  assert.deepEqual(starts, [3])
  assert.equal(starts.length, 1)
  assert.notDeepEqual(starts, [2, 3])
})

test('insertedParagraphStarts: 孤立 CR 不产生起点', () => {
  assert.deepEqual(insertedParagraphStarts('a\rb', 0), [])
})

// ── 3. describeLocalParagraphChange：局部段落变化（Issue #1063 复审评论第 3 条） ──
// 只取编辑点前后各约 2 个 UTF-16 单元的局部窗口，用 before+inserted 构造 after 局部文本；
// 必须覆盖跨编辑边界形成/拆开 CRLF、U+2029，并且不整章重建。

test('describeLocalParagraphChange: 纯文本插入不改变段落结构', () => {
  const change = describeLocalParagraphChange('helloworld', 'x', 5, 5)
  assert.equal(change.paragraphsChanged, false)
  assert.deepEqual(change.newParagraphStarts, [])
  assert.equal(change.newlineDelta, 0)
  assert.equal(change.paragraphDelta, 0)
})

test('describeLocalParagraphChange: 插入 LF 产生一个新增段落起点', () => {
  const change = describeLocalParagraphChange('ab', '\n', 1, 1)
  assert.equal(change.paragraphsChanged, true)
  assert.deepEqual(change.newParagraphStarts, [2])
  assert.equal(change.newlineDelta, 1)
  assert.equal(change.paragraphDelta, 1)
})

test('describeLocalParagraphChange: 跨编辑边界形成 CRLF（a\\nb 在 1 插入 \\r）', () => {
  const before = 'a\nb'
  const change = describeLocalParagraphChange(before, '\r', 1, 1)
  const after = before.substring(0, 1) + '\r' + before.substring(1)
  assert.equal(after, 'a\r\nb')
  // LF 变成 CRLF：段落边界数量不变，但分隔符结构变了。
  assert.equal(change.paragraphsChanged, true)
  assert.deepEqual(change.newParagraphStarts, [])
  assert.equal(change.newlineDelta, 0)
  assert.equal(change.paragraphDelta, 0)
})

test('describeLocalParagraphChange: 跨编辑边界形成 CRLF（a\\rb 在 2 插入 \\n）', () => {
  const change = describeLocalParagraphChange('a\rb', '\n', 2, 2)
  assert.equal(change.paragraphsChanged, true)
  assert.deepEqual(change.newParagraphStarts, [3])
  assert.equal(change.newlineDelta, 1)
  assert.equal(change.paragraphDelta, 1)
})

test('describeLocalParagraphChange: 拆开 CRLF（删掉 a\\r\\nb 的 \\r）', () => {
  const change = describeLocalParagraphChange('a\r\nb', '', 1, 2)
  assert.equal(change.paragraphsChanged, true)
  assert.deepEqual(change.newParagraphStarts, [])
  assert.equal(change.newlineDelta, 0)
  assert.equal(change.paragraphDelta, 0)
})

test('describeLocalParagraphChange: 拆开 CRLF（删掉 a\\r\\nb 的 \\n）后只剩孤立 CR', () => {
  const change = describeLocalParagraphChange('a\r\nb', '', 2, 3)
  assert.equal(change.paragraphsChanged, true)
  assert.deepEqual(change.newParagraphStarts, [])
  assert.equal(change.paragraphDelta, -1)
})

test('describeLocalParagraphChange: 删除整段分隔符，段落边界减少', () => {
  const change = describeLocalParagraphChange('ab\ncd\nef', '', 2, 5)
  assert.equal(change.paragraphsChanged, true)
  // 后一个段落边界随删除整体前移，映射后仍存在，不算新增。
  assert.deepEqual(change.newParagraphStarts, [])
  assert.equal(change.newlineDelta, -1)
  assert.equal(change.paragraphDelta, -1)
})

test('describeLocalParagraphChange: U+2029 是段落边界并产生新增起点', () => {
  const change = describeLocalParagraphChange('ab', '\u2029', 1, 1)
  assert.equal(change.paragraphsChanged, true)
  assert.deepEqual(change.newParagraphStarts, [2])
  assert.equal(change.newlineDelta, 0)
  assert.equal(change.paragraphDelta, 1)
})

test('describeLocalParagraphChange: 多段粘贴按插入文本顺序给出全部新增起点', () => {
  const change = describeLocalParagraphChange('abc', 'x\ny\nz', 1, 1)
  assert.equal(change.paragraphsChanged, true)
  assert.deepEqual(change.newParagraphStarts, [3, 5])
  assert.equal(change.newlineDelta, 2)
  assert.equal(change.paragraphDelta, 2)
})

test('describeLocalParagraphChange: 孤立 CR 插入不改变段落结构', () => {
  const change = describeLocalParagraphChange('ab', '\r', 1, 1)
  assert.equal(change.paragraphsChanged, false)
  assert.deepEqual(change.newParagraphStarts, [])
  assert.equal(change.newlineDelta, 0)
  assert.equal(change.paragraphDelta, 0)
})

test('describeLocalParagraphChange: 替换整段分隔符为 LF（CRLF → LF）', () => {
  const change = describeLocalParagraphChange('a\r\nb', '\n', 1, 3)
  assert.equal(change.paragraphsChanged, true)
  assert.deepEqual(change.newParagraphStarts, [])
  assert.equal(change.newParagraphStarts.length, 0)
})

test('describeLocalParagraphChange: 代理对相邻不 panic，边界按 code point 钳位', () => {
  const emoji = '\u{1F600}' // 2 个 UTF-16 单元
  const text = `${emoji}${emoji}`
  // 在两个代理对之间插入换行。
  const between = describeLocalParagraphChange(text, '\n', 2, 2)
  assert.equal(between.paragraphsChanged, true)
  assert.deepEqual(between.newParagraphStarts, [3])
  assert.equal(between.newlineDelta, 1)
  // 编辑点落在代理对内部（非法原生范围）也不能抛异常。
  const inside = describeLocalParagraphChange(text, 'x', 1, 1)
  assert.equal(typeof inside.newlineDelta, 'number')
  // 删除整个代理对：正文长度变化，但段落结构不变。
  const removed = describeLocalParagraphChange(`${emoji}a`, '', 0, 2)
  assert.equal(removed.paragraphsChanged, false)
  assert.equal(removed.newlineDelta, 0)
  assert.equal(removed.paragraphDelta, 0)
})

test('describeLocalParagraphChange: 空正文首次插入换行', () => {
  const change = describeLocalParagraphChange('', '\n', 0, 0)
  assert.equal(change.paragraphsChanged, true)
  assert.deepEqual(change.newParagraphStarts, [1])
  assert.equal(change.newlineDelta, 1)
  assert.equal(change.paragraphDelta, 1)
})

test('describeLocalParagraphChange: 非法范围（end < start）按纯插入处理且不抛异常', () => {
  const change = describeLocalParagraphChange('abc', '\n', 2, 1)
  assert.equal(change.newlineDelta, 1)
  assert.equal(change.paragraphsChanged, true)
  assert.deepEqual(change.newParagraphStarts, [3])
})

console.log('---')
console.log(`✅ text_document_rules: ${passed} tests passed`)
