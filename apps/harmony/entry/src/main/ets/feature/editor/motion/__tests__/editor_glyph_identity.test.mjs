// editor_glyph_identity.test.mjs — 稳定字形身份表 + 显示版→Core 版位置映射纯逻辑单测。
//
// Issue #879 最新复核评论问题2/问题3：
// - 字形身份必须随每次编辑逐笔搬运，位置平移、old/new 角色变化都不改身份。
// - 显示版坐标必须能映射到 Core 最新版坐标。
//
// 运行：
//   node apps/harmony/entry/src/main/ets/feature/editor/motion/__tests__/editor_glyph_identity.test.mjs

import { strict as assert } from 'node:assert'
import { utf16ToUtf8 } from '../../input/text_offset_mapper.ts'
import { applyPatchesToText, toUtf16CarryPatches } from '../editor_patch_carry.ts'
import { GlyphIdentityTable } from '../editor_glyph_identity.ts'
import { RevisionPositionMap } from '../editor_revision_position_map.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

/** 构造一条替换指令：replace 范围用 UTF-8 byte offset。 */
const patch = (text, utf16Start, utf16End, insertedText) => ({
  replaceStartByte: utf16ToUtf8(text, utf16Start),
  replaceEndByte: utf16ToUtf8(text, utf16End),
  insertedText,
})

console.log('editor_glyph_identity / editor_revision_position_map 纯逻辑单测（Issue #879 复核问题2+3）')
console.log('---')

// ── 字符簇切分 ──
test('create: ASCII 每个字符一个条目', () => {
  const table = GlyphIdentityTable.create('abc', 1)
  assert.equal(table.entryCount(), 3)
  assert.equal(table.entries[0].glyphId, 'orig-1-0')
  assert.equal(table.entries[2].utf16Start, 2)
})

test('create: 中文 BMP 每字一个条目', () => {
  const table = GlyphIdentityTable.create('甲乙丙', 2)
  assert.equal(table.entryCount(), 3)
  assert.equal(table.glyphIdAt(1), 'orig-2-1')
})

test('create: emoji 代理对作为一个条目', () => {
  const table = GlyphIdentityTable.create('a😀b', 3)
  assert.equal(table.entryCount(), 3)
  assert.equal(table.entries[1].utf16Start, 1)
  assert.equal(table.entries[1].utf16End, 3)
  assert.equal(table.glyphIdAt(1), 'orig-3-1')
  assert.equal(table.glyphIdAt(2), 'orig-3-1')
})

test('create: ZWJ emoji 序列作为一个条目', () => {
  const family = '👨‍👩‍👧'
  const table = GlyphIdentityTable.create(family, 4)
  assert.equal(table.entryCount(), 1)
  assert.equal(table.entries[0].utf16End, family.length)
})

test('create: 组合附加符号并入基字符', () => {
  const text = 'e\u0301x'
  const table = GlyphIdentityTable.create(text, 5)
  assert.equal(table.entryCount(), 2)
  assert.equal(table.entries[0].utf16End, 2)
})

// ── 身份搬运：插入 ──
test('applyPatches: 字前插入后原字身份不变', () => {
  const before = GlyphIdentityTable.create('xy', 10)
  const xId = before.glyphIdAt(0)
  const yId = before.glyphIdAt(1)
  const after = before.applyPatches([patch('xy', 0, 0, 'a')], 'axy', 11)
  assert.notEqual(after, null)
  assert.equal(after.text, 'axy')
  assert.equal(after.revision, 11)
  assert.notEqual(after.glyphIdAt(0), xId)
  assert.equal(after.glyphIdAt(1), xId)
  assert.equal(after.glyphIdAt(2), yId)
})

test('applyPatches: 中间插入后右侧字身份不变', () => {
  const before = GlyphIdentityTable.create('甲乙', 20)
  const yiId = before.glyphIdAt(1)
  const after = before.applyPatches([patch('甲乙', 1, 1, '丙')], '甲丙乙', 21)
  assert.equal(after.text, '甲丙乙')
  assert.equal(after.glyphIdAt(0), before.glyphIdAt(0))
  assert.equal(after.glyphIdAt(2), yiId)
  assert.notEqual(after.glyphIdAt(1), yiId)
})

// ── 身份搬运：删除 ──
test('applyPatches: 删除左侧字后右侧字身份保持', () => {
  const before = GlyphIdentityTable.create('a甲', 30)
  const jiaId = before.glyphIdAt(1)
  const after = before.applyPatches([patch('a甲', 0, 1, '')], '甲', 31)
  assert.equal(after.text, '甲')
  assert.equal(after.glyphIdAt(0), jiaId)
  assert.equal(after.entryCount(), 1)
})

test('applyPatches: 删除的字不再有身份', () => {
  const before = GlyphIdentityTable.create('甲中乙', 40)
  const after = before.applyPatches([patch('甲中乙', 1, 2, '')], '甲乙', 41)
  assert.equal(after.text, '甲乙')
  assert.equal(after.glyphIdAt(1), before.glyphIdAt(2))
  // 被删的“中”没有身份，位置 1 已是“乙”
  assert.notEqual(after.glyphIdAt(1), before.glyphIdAt(1))
})

test('applyPatches: 替换（删除+插入）保两侧身份', () => {
  const before = GlyphIdentityTable.create('甲的乙', 50)
  const after = before.applyPatches([patch('甲的乙', 1, 2, '丙丁')], '甲丙丁乙', 51)
  assert.equal(after.text, '甲丙丁乙')
  assert.equal(after.glyphIdAt(0), before.glyphIdAt(0))
  assert.equal(after.glyphIdAt(3), before.glyphIdAt(2))
})

// ── 身份搬运：多笔连续编辑（问题1/3 的组合场景）──
test('applyPatches: 两笔连续编辑后同一字身份跨版本稳定', () => {
  let table = GlyphIdentityTable.create('xy', 60)
  const xId = table.glyphIdAt(0)
  const yId = table.glyphIdAt(1)

  // 第一笔：开头插入 a → axy
  table = table.applyPatches([patch('xy', 0, 0, 'a')], 'axy', 61)
  assert.equal(table.glyphIdAt(1), xId)
  assert.equal(table.glyphIdAt(2), yId)
  const aId = table.glyphIdAt(0)

  // 第二笔：删掉中间的 x → ay
  table = table.applyPatches([patch('axy', 1, 2, '')], 'ay', 62)
  assert.equal(table.text, 'ay')
  assert.equal(table.glyphIdAt(0), aId)
  assert.equal(table.glyphIdAt(1), yId)
})

test('applyPatches: 先删后插可复合', () => {
  let table = GlyphIdentityTable.create('甲乙丙', 70)
  const bingId = table.glyphIdAt(2)
  table = table.applyPatches([patch('甲乙丙', 0, 1, '')], '乙丙', 71)
  assert.equal(table.glyphIdAt(1), bingId)
  table = table.applyPatches([patch('乙丙', 0, 0, '新')], '新乙丙', 72)
  assert.equal(table.text, '新乙丙')
  assert.equal(table.glyphIdAt(2), bingId)
})

test('applyPatches: emoji 删除后其它字身份不变', () => {
  let table = GlyphIdentityTable.create('a😀b', 80)
  const bId = table.glyphIdAt(3)
  const after = table.applyPatches([patch('a😀b', 1, 3, '')], 'ab', 81)
  assert.equal(after.text, 'ab')
  assert.equal(after.glyphIdAt(1), bId)
})

// ── 非法输入必须失败 ──
test('applyPatches: 结果文本与重放不符返回 null', () => {
  const table = GlyphIdentityTable.create('xy', 90)
  assert.equal(table.applyPatches([patch('xy', 0, 0, 'a')], 'xy', 91), null)
})

test('applyPatches: 越界范围返回 null', () => {
  const table = GlyphIdentityTable.create('xy', 92)
  assert.equal(table.applyPatches([
    { replaceStartByte: 99, replaceEndByte: 99, insertedText: 'a' }
  ], 'xya', 93), null)
})

test('applyPatches: 非 UTF-8 字符边界返回 null', () => {
  const table = GlyphIdentityTable.create('甲', 94)
  assert.equal(table.applyPatches([
    { replaceStartByte: 1, replaceEndByte: 2, insertedText: '' }
  ], '甲', 95), null)
})

test('toUtf16CarryPatches: 重叠 patch 返回 null', () => {
  const ops = toUtf16CarryPatches('abcd', [
    { replaceStartByte: 0, replaceEndByte: 2, insertedText: 'x' },
    { replaceStartByte: 1, replaceEndByte: 3, insertedText: 'y' },
  ])
  assert.equal(ops, null)
})

test('applyPatchesToText: 多 patch 重放文本正确', () => {
  const result = applyPatchesToText('abcd', [
    { replaceStartByte: 0, replaceEndByte: 1, insertedText: 'X' },
    { replaceStartByte: 3, replaceEndByte: 4, insertedText: 'YZ' },
  ])
  assert.equal(result, 'XbcYZ')
})

// ── 位置映射：显示版 → Core 最新版 ──
test('positionMap: 无编辑时 build 返回 null（调用方不做映射）', () => {
  assert.equal(RevisionPositionMap.build([], 1, 1), null)
})

test('positionMap: 开头插入把后续位置整体后移', () => {
  const map = RevisionPositionMap.build([
    { beforeText: 'xy', afterText: 'axy', patches: [patch('xy', 0, 0, 'a')] },
  ], 10, 11)
  assert.notEqual(map, null)
  assert.equal(map.mapOffset(0), 0)
  assert.equal(map.mapOffset(1), 2)
  assert.equal(map.mapOffset(2), 3)
})

test('positionMap: 点击被删字符映射到删除点', () => {
  const map = RevisionPositionMap.build([
    { beforeText: 'abc', afterText: 'ac', patches: [patch('abc', 1, 2, '')] },
  ], 20, 21)
  assert.equal(map.mapOffset(1), 1)
  assert.equal(map.mapOffset(2), 1)
  assert.equal(map.mapOffset(3), 2)
})

test('positionMap: 两笔连续编辑（插入+删除）映射到 Core 最新坐标', () => {
  const map = RevisionPositionMap.build([
    { beforeText: 'xy', afterText: 'axy', patches: [patch('xy', 0, 0, 'a')] },
    { beforeText: 'axy', afterText: 'ay', patches: [patch('axy', 1, 2, '')] },
  ], 10, 12)
  assert.equal(map.mapOffset(0), 0)
  assert.equal(map.mapOffset(1), 1) // 点在被删的 x 上 → 删除点
  assert.equal(map.mapOffset(2), 2) // 点在 y 上 → 仍是 y 之后
})

test('positionMap: 链中任一笔记不上返回 null', () => {
  assert.equal(RevisionPositionMap.build([
    { beforeText: 'xy', afterText: 'axy', patches: [patch('xy', 0, 0, 'a')] },
    { beforeText: 'axy', afterText: 'zzz', patches: [patch('axy', 1, 2, '')] },
  ], 10, 12), null)
})

test('positionMap: UTF-8 多字节偏移可正确映射', () => {
  const map = RevisionPositionMap.build([
    { beforeText: '甲中乙', afterText: '甲乙', patches: [patch('甲中乙', 1, 2, '')] },
  ], 30, 31)
  assert.equal(map.mapOffset(0), 0)
  assert.equal(map.mapOffset(1), 1)
  assert.equal(map.mapOffset(2), 1)
  assert.equal(map.mapOffset(3), 2)
})

test('withRevision: 正文不变时沿用同一份身份，只推进 revision 标签', () => {
  const before = GlyphIdentityTable.create('甲乙', 10)
  const after = before.withRevision(11)
  assert.equal(after.revision, 11)
  assert.equal(after.text, before.text)
  assert.equal(after.glyphIdAt(0), before.glyphIdAt(0))
  assert.equal(after.glyphIdAt(1), before.glyphIdAt(1))
  assert.equal(before.withRevision(10), before) // 同 revision 直接复用
})

test('跨修订连续性: 连着三笔编辑后，未被触碰的字仍是同一个身份', () => {
  const t10 = GlyphIdentityTable.create('甲乙丙', 10)
  const 甲 = t10.glyphIdAt(0)
  const 乙 = t10.glyphIdAt(1)
  const 丙 = t10.glyphIdAt(2)

  // 11: 开头插入「前」
  const t11 = t10.applyPatches([patch('甲乙丙', 0, 0, '前')], '前甲乙丙', 11)
  // 12: 末尾插入「后」
  const t12 = t11.applyPatches([patch('前甲乙丙', 4, 4, '后')], '前甲乙丙后', 12)
  // 13: 删掉开头的「前」
  const t13 = t12.applyPatches([patch('前甲乙丙后', 0, 1, '')], '甲乙丙后', 13)

  assert.equal(t11.glyphIdAt(1), 甲)
  assert.equal(t12.glyphIdAt(1), 甲)
  assert.equal(t12.glyphIdAt(2), 乙)
  assert.equal(t13.glyphIdAt(0), 甲) // 「甲」从 offset 1 平移到 0，身份不变
  assert.equal(t13.glyphIdAt(1), 乙)
  assert.equal(t13.glyphIdAt(2), 丙)
  assert.notEqual(t13.glyphIdAt(3), 丙) // 「后」是 12 插入的新字，不是「丙」
})

test('applyPatches: 一次事务内多条 patch 顺序搬运', () => {
  const t0 = GlyphIdentityTable.create('abcd', 20)
  const b = t0.glyphIdAt(1)
  const d = t0.glyphIdAt(3)
  // 同时：删掉 a，删掉 c
  const t1 = t0.applyPatches([
    patch('abcd', 0, 1, ''),
    patch('abcd', 2, 3, ''),
  ], 'bd', 21)
  assert.equal(t1.glyphIdAt(0), b)
  assert.equal(t1.glyphIdAt(1), d)
})

test('applyPatches: 替换后旧条目被丢弃且新条目拿到新身份', () => {
  const t0 = GlyphIdentityTable.create('甲乙', 30)
  const 乙 = t0.glyphIdAt(1)
  const t1 = t0.applyPatches([patch('甲乙', 0, 1, '丙')], '丙乙', 31)
  assert.equal(t1.glyphIdAt(1), 乙)
  assert.ok(t1.glyphIdAt(0).startsWith('ins-31-'))
})

console.log('---')
console.log(`✅ editor_glyph_identity: ${passed} tests passed`)
