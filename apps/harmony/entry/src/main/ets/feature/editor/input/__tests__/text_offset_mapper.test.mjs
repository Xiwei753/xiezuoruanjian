// text_offset_mapper.test.mjs — 鸿蒙端唯一 UTF-8/UTF-16 换算实现的纯逻辑单测
// （Issue #1063 评论第 3 条：只保留一套规则；孤立代理项按统一协议显式处理）。
//
// 用 Node --experimental-strip-types 直接 import input/text_offset_mapper.ts（纯 TS，无 ArkUI 依赖）。
// 验证：
//   1. 统一协议：完整代理对 = 1 个 scalar（UTF-16 2 单元 / UTF-8 4 byte）；
//      孤立高/低代理项 = 3 byte（WTF-8，与 TextEncoder 的 U+FFFD 3 byte 一致）。
//   2. utf8ByteLength / utf16ToUtf8 / utf16ToUtf8Checked / utf8ToUtf16 / utf8ByteOffsetToUtf16
//      在 ASCII、中文、emoji、混合文本、孤立代理项、代理对中间位置、越界、非整数上的行为。
//   3. 严格版（utf16ToUtf8Checked / utf8ByteOffsetToUtf16）只用 null / -1 报告非法位置，
//      非严格版做方向性钳位；两者的边界口径与 utf8ByteLength 完全一致。
//
// 运行：node --experimental-strip-types --test __tests__/text_offset_mapper.test.mjs

import { strict as assert } from 'node:assert'
import {
  utf8ByteLength,
  utf16ToUtf8,
  utf16ToUtf8Checked,
  utf8ToUtf16,
  utf8ByteOffsetToUtf16,
} from '../text_offset_mapper.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

const LONE_HIGH = '\uD800'   // 孤立高代理项
const LONE_LOW = '\uDC00'    // 孤立低代理项
const EMOJI = '😀'           // U+1F600，完整代理对（4 byte / 2 unit）

console.log('text_offset_mapper：唯一 UTF-8 ↔ UTF-16 换算协议')
console.log('---')

// ── 1. utf8ByteLength ──
test('utf8ByteLength: ASCII/中文/emoji/混合', () => {
  assert.equal(utf8ByteLength(''), 0)
  assert.equal(utf8ByteLength('abc'), 3)
  assert.equal(utf8ByteLength('你好'), 6)
  assert.equal(utf8ByteLength(EMOJI), 4)
  assert.equal(utf8ByteLength('a你' + EMOJI + 'b'), 1 + 3 + 4 + 1)
})

test('utf8ByteLength: ZWJ emoji 序列按各 scalar 累加', () => {
  // 👨‍👩 = U+1F468 + U+200D + U+1F469 = 4 + 3 + 4
  assert.equal(utf8ByteLength('👨‍👩'), 11)
})

test('utf8ByteLength: 孤立代理项统一按 3 byte（WTF-8）', () => {
  assert.equal(utf8ByteLength(LONE_HIGH), 3)
  assert.equal(utf8ByteLength(LONE_LOW), 3)
  assert.equal(utf8ByteLength(LONE_HIGH + 'a'), 4)
  assert.equal(utf8ByteLength('a' + LONE_LOW), 4)
  // 两个孤立高代理项不能被当成一个完整代理对
  assert.equal(utf8ByteLength(LONE_HIGH + LONE_HIGH), 6)
})

test('utf8ByteLength: 与 TextEncoder 对孤立代理项的字节数一致（U+FFFD = 3 byte）', () => {
  const encoder = new TextEncoder()
  const samples = [LONE_HIGH, LONE_LOW, 'a' + LONE_HIGH, LONE_HIGH + 'b', LONE_HIGH + LONE_LOW, EMOJI, '你好' + LONE_HIGH]
  for (const text of samples) {
    assert.equal(utf8ByteLength(text), encoder.encode(text).length,
      `byte 长度应与 TextEncoder 一致: ${JSON.stringify(text)}`)
  }
})

// ── 2. utf16ToUtf8 ──
test('utf16ToUtf8: 边界与钳位', () => {
  assert.equal(utf16ToUtf8('', 0), 0)
  assert.equal(utf16ToUtf8('abc', 0), 0)
  assert.equal(utf16ToUtf8('abc', 2), 2)
  assert.equal(utf16ToUtf8('abc', 10), 3)     // 越界钳到末尾
  assert.equal(utf16ToUtf8('abc', -1), 0)
  assert.equal(utf16ToUtf8('你好', 1), 3)
  assert.equal(utf16ToUtf8('你好', 2), 6)
  assert.equal(utf16ToUtf8(EMOJI, 2), 4)
  assert.equal(utf16ToUtf8(LONE_HIGH, 1), 3)
})

// ── 3. utf16ToUtf8Checked（严格） ──
test('utf16ToUtf8Checked: 合法边界返回 byte offset', () => {
  assert.equal(utf16ToUtf8Checked('abc', 0), 0)
  assert.equal(utf16ToUtf8Checked('abc', 3), 3)
  assert.equal(utf16ToUtf8Checked('你好', 1), 3)
  assert.equal(utf16ToUtf8Checked('你好', 2), 6)
  assert.equal(utf16ToUtf8Checked(EMOJI, 0), 0)
  assert.equal(utf16ToUtf8Checked(EMOJI, 2), 4)
})

test('utf16ToUtf8Checked: 完整代理对中间位置 → null', () => {
  assert.equal(utf16ToUtf8Checked(EMOJI, 1), null)
  assert.equal(utf16ToUtf8Checked('a' + EMOJI + 'b', 2), null)
})

test('utf16ToUtf8Checked: 孤立代理项前后是合法边界（3 byte）', () => {
  assert.equal(utf16ToUtf8Checked(LONE_HIGH, 0), 0)
  assert.equal(utf16ToUtf8Checked(LONE_HIGH, 1), 3)
  assert.equal(utf16ToUtf8Checked(LONE_LOW, 1), 3)
  assert.equal(utf16ToUtf8Checked(LONE_HIGH + 'a', 1), 3)
  assert.equal(utf16ToUtf8Checked(LONE_HIGH + 'a', 2), 4)
})

test('utf16ToUtf8Checked: 越界 / 负 / 非整数 → null', () => {
  assert.equal(utf16ToUtf8Checked('abc', 4), null)
  assert.equal(utf16ToUtf8Checked('abc', -1), null)
  assert.equal(utf16ToUtf8Checked('abc', 1.5), null)
  assert.equal(utf16ToUtf8Checked('', 1), null)
})

// ── 4. utf8ToUtf16（非严格，方向性钳位） ──
test('utf8ToUtf16: 边界返回对应 UTF-16 offset', () => {
  assert.equal(utf8ToUtf16('abc', 0), 0)
  assert.equal(utf8ToUtf16('abc', 2), 2)
  assert.equal(utf8ToUtf16('abc', 3), 3)
  assert.equal(utf8ToUtf16('你好', 3), 1)
  assert.equal(utf8ToUtf16('你好', 6), 2)
  assert.equal(utf8ToUtf16(EMOJI, 4), 2)
})

test('utf8ToUtf16: 编码单元中间左对齐到前一个边界', () => {
  assert.equal(utf8ToUtf16('你好', 1), 0)
  assert.equal(utf8ToUtf16('你好', 5), 1)
  assert.equal(utf8ToUtf16(EMOJI, 1), 0)
  assert.equal(utf8ToUtf16(EMOJI, 3), 0)
  assert.equal(utf8ToUtf16('a' + EMOJI, 3), 1)
})

test('utf8ToUtf16: 越界钳位到末尾，负值归 0', () => {
  assert.equal(utf8ToUtf16('abc', 4), 3)
  assert.equal(utf8ToUtf16('你好', 100), 2)
  assert.equal(utf8ToUtf16('abc', -2), 0)
})

test('utf8ToUtf16: 孤立代理项按 3 byte 推进', () => {
  assert.equal(utf8ToUtf16(LONE_HIGH, 1), 0)
  assert.equal(utf8ToUtf16(LONE_HIGH, 3), 1)
  assert.equal(utf8ToUtf16(LONE_HIGH + 'a', 3), 1)
  assert.equal(utf8ToUtf16(LONE_HIGH + 'a', 4), 2)
})

// ── 5. utf8ByteOffsetToUtf16（严格，-1 表示非法） ──
test('utf8ByteOffsetToUtf16: 合法边界', () => {
  assert.equal(utf8ByteOffsetToUtf16('', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('abc', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('abc', 1), 1)
  assert.equal(utf8ByteOffsetToUtf16('abc', 3), 3)
  assert.equal(utf8ByteOffsetToUtf16('你好', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('你好', 3), 1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 6), 2)
  assert.equal(utf8ByteOffsetToUtf16(EMOJI, 4), 2)
  assert.equal(utf8ByteOffsetToUtf16('a你' + EMOJI + 'b', 4), 2)
  assert.equal(utf8ByteOffsetToUtf16('a你' + EMOJI + 'b', 8), 4)
})

test('utf8ByteOffsetToUtf16: 编码单元中间/越界/负/非整数 → -1', () => {
  assert.equal(utf8ByteOffsetToUtf16('你好', 1), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 2), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 4), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 5), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 7), -1)
  assert.equal(utf8ByteOffsetToUtf16(EMOJI, 1), -1)
  assert.equal(utf8ByteOffsetToUtf16(EMOJI, 2), -1)
  assert.equal(utf8ByteOffsetToUtf16(EMOJI, 3), -1)
  assert.equal(utf8ByteOffsetToUtf16(EMOJI, 5), -1)
  assert.equal(utf8ByteOffsetToUtf16('abc', 4), -1)
  assert.equal(utf8ByteOffsetToUtf16('abc', -1), -1)
  assert.equal(utf8ByteOffsetToUtf16('abc', 1.5), -1)
  assert.equal(utf8ByteOffsetToUtf16('', 1), -1)
})

test('utf8ByteOffsetToUtf16: 孤立代理项（3 byte 单元）边界', () => {
  assert.equal(utf8ByteOffsetToUtf16(LONE_HIGH, 3), 1)
  assert.equal(utf8ByteOffsetToUtf16(LONE_HIGH, 1), -1)
  assert.equal(utf8ByteOffsetToUtf16(LONE_LOW, 3), 1)
  // 两个孤立高代理项不是完整代理对，各按 3 byte 计。
  assert.equal(utf8ByteOffsetToUtf16(LONE_HIGH + LONE_HIGH, 3), 1)
  assert.equal(utf8ByteOffsetToUtf16(LONE_HIGH + LONE_HIGH, 6), 2)
})

test('一致性：utf8ByteOffsetToUtf16(text, utf8ByteLength(text)) === text.length', () => {
  const samples = ['', 'abc', '你好', EMOJI, 'a你' + EMOJI + 'b', LONE_HIGH, LONE_HIGH + 'a', LONE_HIGH + LONE_LOW, '👨‍👩']
  for (const text of samples) {
    assert.equal(utf8ByteOffsetToUtf16(text, utf8ByteLength(text)), text.length,
      `末尾必须是合法边界: ${JSON.stringify(text)}`)
  }
})

test('一致性：utf8ToUtf16 与严格版在合法边界上结果相同', () => {
  const text = 'a你' + EMOJI + 'b'
  let byte = 0
  let utf16 = 0
  assert.equal(utf8ToUtf16(text, byte), utf16)
  while (utf16 < text.length) {
    const code = text.codePointAt(utf16)
    byte += utf8ByteLength(String.fromCodePoint(code))
    utf16 += code > 0xFFFF ? 2 : 1
    assert.equal(utf8ByteOffsetToUtf16(text, byte), utf16)
    assert.equal(utf8ToUtf16(text, byte), utf16)
  }
})

console.log('---')
console.log(`✅ text_offset_mapper: ${passed} tests passed`)
