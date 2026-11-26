// editor_patch_batch.test.mjs — displayPatches 作为 base 坐标原子批次（Issue #1063 评论第 2 条）。
//
// 用 Node --experimental-strip-types 直接 import editor_patch_logic.ts（纯 TS，无 ArkUI 依赖）。
// 验证：
//   1. 同一批含不同长度替换的 patch（长→短、短→长）一次性应用：正文与 revision 完全正确，
//      且 patch 的 byte offset 始终按**原始 snapshot** 解释（不是逐项执行后的中间文本）。
//   2. 非法批次（升序、重叠、同起点、非 UTF-8 字符边界、越界、倒置区间、revision 不一致）
//      一律 ok=false，且不产生部分修改（原 snapshot 文本不变、不返回 snapshot）。
//   3. undo/redo 语义：同一 base 上“正向 batch”与“逆向 batch”各应用一次，正文回到原状，
//      revision 每步各自推进。
//   4. validatePatchBatch 作为公开校验入口的结果形状。
//
// 运行：node --experimental-strip-types --test __tests__/editor_patch_batch.test.mjs

import { strict as assert } from 'node:assert'
import {
  applyEditResultToSnapshot,
  validatePatchBatch,
  APPLIED,
  NO_CHANGE,
} from '../editor_patch_logic.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// 工具：构造完整 EditorEditResult（默认字段便于测试）。
function makeResult(overrides) {
  const base = {
    outcome: APPLIED,
    transactionId: 1,
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [],
    oldSelectionAnchor: 0,
    oldSelectionHead: 0,
    newSelectionAnchor: 0,
    newSelectionHead: 0,
    compositionSession: null,
    contentDelta: {
      insertedChars: 0,
      deletedChars: 0,
      insertedNonWhitespaceChars: 0,
      deletedNonWhitespaceChars: 0,
    },
    composition: null,
  }
  return { ...base, ...overrides }
}

function makeSnapshot(overrides) {
  const base = {
    text: '',
    revision: 1,
    cursor: 0,
    selectionAnchor: 0,
    generation: 0,
    chapterId: 'c1',
    composition: null,
  }
  return { ...base, ...overrides }
}

// 一个 patch（base 正文 UTF-8 byte 坐标，同一批次共用 baseRevision）。
function makePatch(startByte, endByte, insertedText, baseRevision = 1) {
  return {
    baseRevision,
    newRevision: baseRevision + 1,
    replaceByteStart: startByte,
    replaceByteEndExclusive: endByte,
    insertedText,
    resultingSelectionStart: 0,
    resultingSelectionEnd: 0,
  }
}

// 'ab你好cd' 的 UTF-8 byte 坐标：
//   a=[0,1) b=[1,2) 你=[2,5) 好=[5,8) c=[8,9) d=[9,10)，总 10 byte。
const MIXED_TEXT = 'ab你好cd'

console.log('displayPatches 原子批次（base 坐标、降序、不重叠）')
console.log('---')

// ── 1. 不同长度替换一次性应用 ──
test('不同长度替换：删除中文 + 变长替换 ASCII，按 base 坐标拼接正确', () => {
  const snapshot = makeSnapshot({ text: MIXED_TEXT, revision: 1, cursor: 6, selectionAnchor: 6 })
  // 降序：先处理 start=8 的 'c'→'CDEF'（变长），再处理 start=5 的 '好'→''（变短）。
  const result = makeResult({
    outcome: APPLIED,
    baseRevision: 1,
    newRevision: 9,
    displayPatches: [
      makePatch(8, 9, 'CDEF'),   // c → CDEF（更长）
      makePatch(5, 8, ''),       // 好 → 删除（更短）
    ],
    newSelectionAnchor: 3,
    newSelectionHead: 7,
  })
  const applied = applyEditResultToSnapshot(snapshot, result)
  assert.equal(applied.ok, true)
  // a b 你 [好被删] c→CDEF d
  assert.equal(applied.snapshot.text, 'ab你CDEFd')
  assert.equal(applied.snapshot.revision, 9)
  assert.equal(applied.snapshot.cursor, 7)
  assert.equal(applied.snapshot.selectionAnchor, 3)
  assert.equal(applied.snapshot.chapterId, 'c1')
  assert.equal(applied.snapshot.composition, null)
  // 原 snapshot 不被就地修改
  assert.equal(snapshot.text, MIXED_TEXT)
})

test('不同长度替换：同一批次内三处替换（含 emoji 代理对），结果与 revision 正确', () => {
  // 文本 'A😀中B'：A=[0,1) 😀=[1,5) 中=[5,8) B=[8,9)，总 9 byte。
  const snapshot = makeSnapshot({ text: 'A😀中B', revision: 4, cursor: 0, selectionAnchor: 0 })
  const result = makeResult({
    outcome: APPLIED,
    baseRevision: 4,
    newRevision: 5,
    displayPatches: [
      makePatch(8, 9, 'Beta', 4),   // B → Beta（变长）
      makePatch(5, 8, '文', 4),     // 中 → 文（等长）
      makePatch(1, 5, '😀😀', 4),   // 😀 → 😀😀（变长，代理对边界）
    ],
  })
  const applied = applyEditResultToSnapshot(snapshot, result)
  assert.equal(applied.ok, true)
  assert.equal(applied.snapshot.text, 'A😀😀文Beta')
  assert.equal(applied.snapshot.revision, 5)
})

test('相邻区间（后一项 end == 前一项 start）不算重叠', () => {
  const snapshot = makeSnapshot({ text: 'abcdef', revision: 1 })
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [
      makePatch(4, 6, 'X'),  // ef → X
      makePatch(2, 4, 'Y'),  // cd → Y
    ],
  })
  const applied = applyEditResultToSnapshot(snapshot, result)
  assert.equal(applied.ok, true)
  assert.equal(applied.snapshot.text, 'abYX')
})

test('纯插入（空区间）批次：末尾插入 + 中间插入按 base 坐标生效', () => {
  const snapshot = makeSnapshot({ text: '你好', revision: 2 })
  const result = makeResult({
    baseRevision: 2,
    newRevision: 3,
    displayPatches: [
      makePatch(6, 6, '世界', 2),  // 末尾插入
      makePatch(0, 0, '序：', 2),  // 开头插入
    ],
  })
  const applied = applyEditResultToSnapshot(snapshot, result)
  assert.equal(applied.ok, true)
  assert.equal(applied.snapshot.text, '序：你好世界')
})

test('NO_CHANGE 空批次：正文不变、revision 保留 Core 返回值', () => {
  const snapshot = makeSnapshot({ text: 'abc', revision: 7 })
  const result = makeResult({ outcome: NO_CHANGE, baseRevision: 7, newRevision: 7, displayPatches: [] })
  const applied = applyEditResultToSnapshot(snapshot, result)
  assert.equal(applied.ok, true)
  assert.equal(applied.snapshot.text, 'abc')
  assert.equal(applied.snapshot.revision, 7)
})

// ── 2. 非法批次一律失败，且不产生部分修改 ──
console.log('---')
console.log('非法批次：ok=false 且不改原 snapshot')

// 断言：失败、reason 以 patchFailed: 开头、没有返回 snapshot、原文本未变。
function assertBatchRejected(snapshot, result, reasonFragment) {
  const applied = applyEditResultToSnapshot(snapshot, result)
  assert.equal(applied.ok, false, '批次应被拒绝')
  assert.equal(typeof applied.reason, 'string')
  assert.ok(applied.reason.startsWith('patchFailed:'), `reason 应以 patchFailed: 开头，实际 ${applied.reason}`)
  if (reasonFragment !== undefined) {
    assert.ok(applied.reason.includes(reasonFragment),
      `reason 应包含 ${reasonFragment}，实际 ${applied.reason}`)
  }
  assert.equal(applied.snapshot, undefined)
  assert.equal(snapshot.text, MIXED_TEXT)
}

test('升序批次（patch 顺序不满足严格降序）→ ok=false', () => {
  const snapshot = makeSnapshot({ text: MIXED_TEXT, revision: 1 })
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [
      makePatch(5, 8, ''),        // 升序：start=5 在前
      makePatch(8, 9, 'CDEF'),    // start=8 在后
    ],
  })
  assertBatchRejected(snapshot, result, 'not strictly below')
})

test('同起点两项（Core 应已合并）→ ok=false', () => {
  const snapshot = makeSnapshot({ text: MIXED_TEXT, revision: 1 })
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [
      makePatch(8, 9, 'X'),
      makePatch(8, 10, 'Y'),
    ],
  })
  assertBatchRejected(snapshot, result, 'not strictly below')
})

test('重叠批次（后一项 end > 前一项 start）→ ok=false', () => {
  const snapshot = makeSnapshot({ text: MIXED_TEXT, revision: 1 })
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [
      makePatch(5, 9, 'X'),   // [5,9)
      makePatch(2, 8, 'Y'),   // [2,8) 与 [5,9) 重叠
    ],
  })
  assertBatchRejected(snapshot, result, 'overlaps previous patch')
})

test('非 UTF-8 字符边界（start 落在中文中间）→ ok=false', () => {
  const snapshot = makeSnapshot({ text: MIXED_TEXT, revision: 1 })
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [makePatch(3, 5, 'X')],  // 3 是 '你' 的第二 byte
  })
  assertBatchRejected(snapshot, result, 'not a UTF-8 character boundary')
})

test('非 UTF-8 字符边界（end 落在 emoji 代理对中间）→ ok=false', () => {
  // 'A😀'：A=[0,1) 😀=[1,5)；end=3 落在 4 byte 编码单元中间。
  const snapshot = makeSnapshot({ text: 'A😀', revision: 1 })
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [makePatch(1, 3, 'X')],
  })
  const applied = applyEditResultToSnapshot(snapshot, result)
  assert.equal(applied.ok, false)
  assert.ok(applied.reason.startsWith('patchFailed:'))
  assert.ok(applied.reason.includes('not a UTF-8 character boundary'))
  assert.equal(applied.snapshot, undefined)
  assert.equal(snapshot.text, 'A😀')
})

test('越界（byte offset > 文本 UTF-8 byte 长度）→ ok=false', () => {
  const snapshot = makeSnapshot({ text: MIXED_TEXT, revision: 1 })
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [makePatch(11, 11, 'X')],  // 总长 10 byte
  })
  assertBatchRejected(snapshot, result, 'exceeds text byte length')
})

test('倒置区间（start > end）→ ok=false', () => {
  const snapshot = makeSnapshot({ text: MIXED_TEXT, revision: 1 })
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [makePatch(9, 2, 'X')],
  })
  assertBatchRejected(snapshot, result, 'invalid range')
})

test('批次 revision 不一致（patch.baseRevision 与 result.baseRevision 不同）→ ok=false', () => {
  const snapshot = makeSnapshot({ text: MIXED_TEXT, revision: 1 })
  const result = makeResult({
    baseRevision: 1,
    newRevision: 3,
    displayPatches: [
      makePatch(8, 9, 'X', 1),
      makePatch(5, 8, 'Y', 2),  // 混入了另一 revision 的 patch
    ],
  })
  assertBatchRejected(snapshot, result, 'baseRevision')
})

test('失败批次不返回 snapshot，调用方只能从 Core snapshot() 重建', () => {
  const snapshot = makeSnapshot({ text: MIXED_TEXT, revision: 1 })
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [makePatch(3, 5, 'X')],
  })
  const applied = applyEditResultToSnapshot(snapshot, result)
  assert.equal(applied.ok, false)
  assert.equal(Object.prototype.hasOwnProperty.call(applied, 'snapshot'), false)
  assert.equal(snapshot.text, MIXED_TEXT)
})

// ── 3. undo/redo 语义：正向 batch 与逆向 batch 各自推进 revision ──
console.log('---')
console.log('undo/redo：正向/逆向批次各应用一次回到原状')

test('undo/redo：正向 batch 应用后逆向 batch 还原正文，revision 1→2→3', () => {
  // 正向：'ab你好cd' 中 你好 → 他，末尾追加 '!'。
  const baseSnapshot = makeSnapshot({ text: MIXED_TEXT, revision: 1, cursor: 2, selectionAnchor: 2 })
  const forwardResult = makeResult({
    outcome: APPLIED,
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [
      makePatch(10, 10, '!', 1),   // 末尾插入
      makePatch(2, 8, '他', 1),    // 你好 → 他
    ],
    newSelectionAnchor: 3,
    newSelectionHead: 3,
  })
  const forward = applyEditResultToSnapshot(baseSnapshot, forwardResult)
  assert.equal(forward.ok, true)
  assert.equal(forward.snapshot.text, 'ab他cd!')
  assert.equal(forward.snapshot.revision, 2)

  // 逆向：在正向结果上把 '!' 删掉、'他' 换回 '你好'。
  // 'ab他cd!' 的 UTF-8 byte 坐标：a=[0,1) b=[1,2) 他=[2,5) c=[5,6) d=[6,7) !=[7,8)。
  const backwardResult = makeResult({
    outcome: APPLIED,
    baseRevision: 2,
    newRevision: 3,
    displayPatches: [
      makePatch(7, 8, '', 2),      // 删除 '!'
      makePatch(2, 5, '你好', 2),  // 他 → 你好
    ],
    newSelectionAnchor: 2,
    newSelectionHead: 2,
  })
  const backward = applyEditResultToSnapshot(forward.snapshot, backwardResult)
  assert.equal(backward.ok, true)
  assert.equal(backward.snapshot.text, MIXED_TEXT)
  assert.equal(backward.snapshot.revision, 3)
})

test('undo/redo：多步替换链每步 revision 各自推进且正文可逆', () => {
  const original = '一二三四五'
  // 每步只改一个汉字（每个 3 byte）。
  let snapshot = makeSnapshot({ text: original, revision: 10 })
  const steps = [
    { patches: [makePatch(3, 6, 'X', 10)], expected: '一X三四五', revision: 11 },   // 二 → X
    { patches: [makePatch(7, 10, 'Y', 11)], expected: '一X三Y五', revision: 12 },  // 四 → Y
    // 回滚两处：'一X三Y五' 的字节坐标 一=[0,3) X=[3,4) 三=[4,7) Y=[7,8) 五=[8,11)
    { patches: [makePatch(7, 8, '四', 12), makePatch(3, 4, '二', 12)],
      expected: '一二三四五', revision: 13 },
  ]
  for (const step of steps) {
    const applied = applyEditResultToSnapshot(snapshot, makeResult({
      baseRevision: snapshot.revision,
      newRevision: step.revision,
      displayPatches: step.patches,
    }))
    assert.equal(applied.ok, true)
    assert.equal(applied.snapshot.text, step.expected)
    assert.equal(applied.snapshot.revision, step.revision)
    snapshot = applied.snapshot
  }
  assert.equal(snapshot.text, original)
  assert.equal(snapshot.revision, 13)
})

// ── 4. validatePatchBatch 公开校验入口 ──
console.log('---')
console.log('validatePatchBatch 结果形状')

test('validatePatchBatch: 合法批次 → { ok: true }', () => {
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [
      makePatch(8, 9, 'CDEF'),
      makePatch(2, 5, '你'),
    ],
  })
  const validation = validatePatchBatch(MIXED_TEXT, result)
  assert.equal(validation.ok, true)
  assert.deepEqual(validation, { ok: true })
})

test('validatePatchBatch: 空批次 → { ok: true }', () => {
  assert.deepEqual(validatePatchBatch('', makeResult({ displayPatches: [] })), { ok: true })
})

test('validatePatchBatch: 非法批次 → { ok: false, reason } 且 reason 非空', () => {
  const result = makeResult({
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [
      makePatch(2, 5, 'X'),
      makePatch(8, 9, 'Y'),  // 升序
    ],
  })
  const validation = validatePatchBatch(MIXED_TEXT, result)
  assert.equal(validation.ok, false)
  assert.equal(typeof validation.reason, 'string')
  assert.ok(validation.reason.length > 0)
})

test('validatePatchBatch: 不修改传入的 result 批次', () => {
  const patches = [makePatch(8, 9, 'X'), makePatch(2, 5, 'Y')]
  const result = makeResult({ baseRevision: 1, newRevision: 2, displayPatches: patches })
  validatePatchBatch(MIXED_TEXT, result)
  assert.equal(result.displayPatches, patches)
  assert.equal(result.displayPatches.length, 2)
})

console.log('---')
console.log(`✅ editor_patch_batch: ${passed} tests passed`)
