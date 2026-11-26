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
import { listParagraphRanges, insertedParagraphStarts } from '../text_document_rules.ts'

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

console.log('---')
console.log(`✅ text_document_rules: ${passed} tests passed`)
