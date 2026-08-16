// editor_session_patch.test.mjs — editor_patch_logic.ts 的纯逻辑单测。
//
// 用 Node --experimental-strip-types 直接 import editor_patch_logic.ts（纯 TS，无 ArkUI 依赖）。
// 验证：
//   1. utf8ByteOffsetToUtf16 边界映射（ASCII/中文/emoji/混合/非边界/越界/负/空）
//   2. applyPatchStrict 中文/emoji 成功路径 + 失败语义（非字符边界/越界）
//   3. applyEditResultToSnapshot — 返回 { ok, snapshot, reason } 形状
//      - applied 正常更新 text/revision/cursor/selectionAnchor
//      - composition generation 流转（begin→update→finish）
//      - staleRevision → ok=false, reason='staleRevision'
//      - invalidOffset → ok=false, reason='invalidOffset'
//      - invalidRange → ok=false, reason='invalidRange'
//      - noChange 保留 revision 但更新 selection
//      - appliedWithAdjustedSelection 正常应用
//      - patch 失败（非字符边界）→ ok=false, reason='patchFailed:...'
//      - 多 patch 中途失败 → ok=false
//   4. 删除的 deprecated applyPatch 不再存在（import 应为 undefined）
//   5. SerialCommandQueue 串行：enqueue 顺序、每条出队才执行、前一条完成才下一条
//   6. DTO 形状完整性
//
// 运行：node --experimental-strip-types editor_session_patch.test.mjs

import { strict as assert } from 'node:assert'
import * as PatchLogic from '../editor_patch_logic.ts'

const {
  applyPatchStrict,
  utf8ByteOffsetToUtf16,
  applyEditResultToSnapshot,
  SerialCommandQueue,
  APPLIED,
  APPLIED_WITH_ADJUSTED_SELECTION,
  NO_CHANGE,
  STALE_REVISION,
  INVALID_OFFSET,
  INVALID_RANGE,
} = PatchLogic

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// 异步测试 helper
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// 工具：构造一个完整 EditorEditResult（默认值便于测试）。
function makeResult(overrides) {
  const base = {
    outcome: APPLIED,
    transactionId: 1,
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [],
    oldSelectionStart: 0,
    oldSelectionEnd: 0,
    newSelectionStart: 0,
    newSelectionEnd: 0,
    visualIntent: {},
    compositionSession: null,
    contentDelta: {
      insertedChars: 0,
      deletedChars: 0,
      insertedNonWhitespaceChars: 0,
      deletedNonWhitespaceChars: 0,
    },
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
  }
  return { ...base, ...overrides }
}

console.log('editor_patch_logic 纯逻辑单测')

// ── 0. deprecated applyPatch 已删除 ──
test('deprecated applyPatch 已删除（import 为 undefined）', () => {
  assert.equal(PatchLogic.applyPatch, undefined,
    'applyPatch 应已删除；旧实现把 UTF-8 byte offset 直接当 JS UTF-16 下标，中文/emoji 会错位')
})

// ── 1. utf8ByteOffsetToUtf16 基础边界映射 ──
test('utf8ByteOffsetToUtf16: ASCII text="abc" byteOffset 0/1/2/3 → 0/1/2/3', () => {
  assert.equal(utf8ByteOffsetToUtf16('abc', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('abc', 1), 1)
  assert.equal(utf8ByteOffsetToUtf16('abc', 2), 2)
  assert.equal(utf8ByteOffsetToUtf16('abc', 3), 3)
})

test('utf8ByteOffsetToUtf16: 中文 text="你好" byteOffset 0/3/6 → 0/1/2', () => {
  assert.equal(utf8ByteOffsetToUtf16('你好', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('你好', 3), 1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 6), 2)
})

test('utf8ByteOffsetToUtf16: emoji text="😀" byteOffset 0/4 → 0/2', () => {
  assert.equal(utf8ByteOffsetToUtf16('😀', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('😀', 4), 2)
})

test('utf8ByteOffsetToUtf16: 混合 text="A你B" byteOffset 0/1/4/5 → 0/1/2/3', () => {
  assert.equal(utf8ByteOffsetToUtf16('A你B', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('A你B', 1), 1)
  assert.equal(utf8ByteOffsetToUtf16('A你B', 4), 2)
  assert.equal(utf8ByteOffsetToUtf16('A你B', 5), 3)
})

test('utf8ByteOffsetToUtf16: 中文+emoji text="你😀好" 边界 0/3/7/10 → 0/1/3/4', () => {
  assert.equal(utf8ByteOffsetToUtf16('你😀好', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('你😀好', 3), 1)
  assert.equal(utf8ByteOffsetToUtf16('你😀好', 7), 3)
  assert.equal(utf8ByteOffsetToUtf16('你😀好', 10), 4)
})

// ── 2. utf8ByteOffsetToUtf16 非字符边界/越界失败 ──
test('utf8ByteOffsetToUtf16: 非字符边界 text="你好" byteOffset 1/2/4/5 → -1', () => {
  assert.equal(utf8ByteOffsetToUtf16('你好', 1), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 2), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 4), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 5), -1)
})

test('utf8ByteOffsetToUtf16: emoji 中间 byteOffset 1/2/3 → -1', () => {
  assert.equal(utf8ByteOffsetToUtf16('😀', 1), -1)
  assert.equal(utf8ByteOffsetToUtf16('😀', 2), -1)
  assert.equal(utf8ByteOffsetToUtf16('😀', 3), -1)
})

test('utf8ByteOffsetToUtf16: 越界 byteOffset > 文本总字节 → -1', () => {
  assert.equal(utf8ByteOffsetToUtf16('你好', 7), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 100), -1)
  assert.equal(utf8ByteOffsetToUtf16('abc', 4), -1)
  assert.equal(utf8ByteOffsetToUtf16('😀', 5), -1)
})

test('utf8ByteOffsetToUtf16: 负 byteOffset → -1', () => {
  assert.equal(utf8ByteOffsetToUtf16('abc', -1), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', -5), -1)
})

test('utf8ByteOffsetToUtf16: 空文本 byteOffset 0 → 0, >0 → -1', () => {
  assert.equal(utf8ByteOffsetToUtf16('', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('', 1), -1)
})

// ── 3. applyPatchStrict 中文/emoji 成功路径 ──
test('applyPatchStrict: 中文中间插入 text="你好" [3,3) 插 "X" → "你X好"', () => {
  const patch = {
    baseRevision: 1, newRevision: 2,
    replaceByteStart: 3, replaceByteEndExclusive: 3,
    insertedText: 'X', resultingSelectionStart: 2, resultingSelectionEnd: 2,
  }
  const r = applyPatchStrict('你好', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, '你X好')
})

test('applyPatchStrict: 中文范围删除 text="你好" [3,6) 删 → "你"', () => {
  const patch = {
    baseRevision: 1, newRevision: 2,
    replaceByteStart: 3, replaceByteEndExclusive: 6,
    insertedText: '', resultingSelectionStart: 1, resultingSelectionEnd: 1,
  }
  const r = applyPatchStrict('你好', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, '你')
})

test('applyPatchStrict: 混合文本替换 text="A你B" [1,4) 替换为 "他" → "A他B"', () => {
  const patch = {
    baseRevision: 1, newRevision: 2,
    replaceByteStart: 1, replaceByteEndExclusive: 4,
    insertedText: '他', resultingSelectionStart: 1, resultingSelectionEnd: 2,
  }
  const r = applyPatchStrict('A你B', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, 'A他B')
})

test('applyPatchStrict: emoji 前插入 text="😀" [0,0) 插 "X" → "X😀"', () => {
  const patch = {
    baseRevision: 1, newRevision: 2,
    replaceByteStart: 0, replaceByteEndExclusive: 0,
    insertedText: 'X', resultingSelectionStart: 0, resultingSelectionEnd: 1,
  }
  const r = applyPatchStrict('😀', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, 'X😀')
})

test('applyPatchStrict: emoji 后插入 text="😀" [4,4) 插 "X" → "😀X"', () => {
  const patch = {
    baseRevision: 1, newRevision: 2,
    replaceByteStart: 4, replaceByteEndExclusive: 4,
    insertedText: 'X', resultingSelectionStart: 2, resultingSelectionEnd: 3,
  }
  const r = applyPatchStrict('😀', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, '😀X')
})

test('applyPatchStrict: emoji 删除 text="X😀Y" [1,5) 删 → "XY"', () => {
  const patch = {
    baseRevision: 1, newRevision: 2,
    replaceByteStart: 1, replaceByteEndExclusive: 5,
    insertedText: '', resultingSelectionStart: 1, resultingSelectionEnd: 1,
  }
  const r = applyPatchStrict('X😀Y', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, 'XY')
})

test('applyPatchStrict: 中文+emoji 混合删除 text="你😀好" [3,7) 删 emoji → "你好"', () => {
  const patch = {
    baseRevision: 1, newRevision: 2,
    replaceByteStart: 3, replaceByteEndExclusive: 7,
    insertedText: '', resultingSelectionStart: 1, resultingSelectionEnd: 1,
  }
  const r = applyPatchStrict('你😀好', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, '你好')
})

// ── 4. applyPatchStrict 失败语义 ──
test('applyPatchStrict: 非字符边界 startOffset 必须失败 text="你好" [1,3)', () => {
  const patch = {
    baseRevision: 1, newRevision: 2,
    replaceByteStart: 1, replaceByteEndExclusive: 3,
    insertedText: 'X', resultingSelectionStart: 0, resultingSelectionEnd: 0,
  }
  const r = applyPatchStrict('你好', patch)
  assert.equal(r.ok, false)
  assert.equal(typeof r.reason, 'string')
  assert.ok(r.reason.length > 0)
})

test('applyPatchStrict: 越界 byteOffset 必须失败 text="你好" [100,100)', () => {
  const patch = {
    baseRevision: 1, newRevision: 2,
    replaceByteStart: 100, replaceByteEndExclusive: 100,
    insertedText: 'X', resultingSelectionStart: 0, resultingSelectionEnd: 0,
  }
  const r = applyPatchStrict('你好', patch)
  assert.equal(r.ok, false)
  assert.equal(typeof r.reason, 'string')
})

test('applyPatchStrict: 负 offset 必须失败 text="abc" [-1,0)', () => {
  const patch = {
    baseRevision: 1, newRevision: 2,
    replaceByteStart: -1, replaceByteEndExclusive: 0,
    insertedText: 'X', resultingSelectionStart: 0, resultingSelectionEnd: 0,
  }
  const r = applyPatchStrict('abc', patch)
  assert.equal(r.ok, false)
  assert.equal(typeof r.reason, 'string')
})

// ── 5. applyEditResultToSnapshot — { ok, snapshot, reason } 形状 ──
console.log('---')
console.log('applyEditResultToSnapshot: { ok, snapshot, reason } 形状')

test('applyEditResultToSnapshot: applied 正常更新 → { ok: true, snapshot: {...} }', () => {
  const snapshot = makeSnapshot({ text: 'hello', revision: 1, cursor: 5, selectionAnchor: 5, generation: 0 })
  const result = makeResult({
    outcome: APPLIED, baseRevision: 1, newRevision: 2,
    displayPatches: [{
      baseRevision: 1, newRevision: 2,
      replaceByteStart: 5, replaceByteEndExclusive: 5,
      insertedText: '!', resultingSelectionStart: 6, resultingSelectionEnd: 6,
    }],
    newSelectionStart: 6, newSelectionEnd: 6,
    compositionSession: null,
  })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, true)
  assert.equal(r.snapshot.text, 'hello!')
  assert.equal(r.snapshot.revision, 2)
  assert.equal(r.snapshot.cursor, 6)
  assert.equal(r.snapshot.selectionAnchor, 6)
  assert.equal(r.snapshot.generation, 0)
  assert.equal(r.snapshot.chapterId, 'c1')
})

test('applyEditResultToSnapshot: 中文 patch 正常应用 → "你好世界"', () => {
  const snapshot = makeSnapshot({ text: '你好', revision: 1, cursor: 2, selectionAnchor: 2, generation: 0 })
  const result = makeResult({
    outcome: APPLIED, baseRevision: 1, newRevision: 2,
    displayPatches: [{
      baseRevision: 1, newRevision: 2,
      replaceByteStart: 6, replaceByteEndExclusive: 6,
      insertedText: '世界', resultingSelectionStart: 4, resultingSelectionEnd: 4,
    }],
    newSelectionStart: 4, newSelectionEnd: 4,
    compositionSession: null,
  })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, true)
  assert.equal(r.snapshot.text, '你好世界')
  assert.equal(r.snapshot.revision, 2)
})

test('applyEditResultToSnapshot: 多 patch 顺序应用含中文 → "大家你好世界"', () => {
  const snapshot = makeSnapshot({ text: '你好', revision: 1, cursor: 2, selectionAnchor: 2, generation: 0 })
  const result = makeResult({
    outcome: APPLIED, baseRevision: 1, newRevision: 3,
    displayPatches: [
      { baseRevision: 1, newRevision: 2, replaceByteStart: 6, replaceByteEndExclusive: 6,
        insertedText: '世界', resultingSelectionStart: 4, resultingSelectionEnd: 4 },
      { baseRevision: 2, newRevision: 3, replaceByteStart: 0, replaceByteEndExclusive: 0,
        insertedText: '大家', resultingSelectionStart: 0, resultingSelectionEnd: 2 },
    ],
    newSelectionStart: 0, newSelectionEnd: 2,
    compositionSession: null,
  })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, true)
  assert.equal(r.snapshot.text, '大家你好世界')
  assert.equal(r.snapshot.revision, 3)
})

test('applyEditResultToSnapshot: composition generation 流转 begin→update→finish', () => {
  let snapshot = makeSnapshot({ text: 'a', revision: 1, cursor: 1, selectionAnchor: 1, generation: 0 })

  const beginResult = makeResult({
    outcome: APPLIED, baseRevision: 1, newRevision: 1, displayPatches: [],
    newSelectionStart: 1, newSelectionEnd: 1,
    compositionSession: { sessionId: 42, baseRevision: 1, generation: 1 },
  })
  let r = applyEditResultToSnapshot(snapshot, beginResult)
  assert.equal(r.ok, true)
  snapshot = r.snapshot
  assert.equal(snapshot.generation, 1, 'beginComposition 后 generation 应为 1')

  const updateResult = makeResult({
    outcome: APPLIED, baseRevision: 1, newRevision: 1, displayPatches: [],
    newSelectionStart: 1, newSelectionEnd: 1,
    compositionSession: { sessionId: 42, baseRevision: 1, generation: 2 },
  })
  r = applyEditResultToSnapshot(snapshot, updateResult)
  assert.equal(r.ok, true)
  snapshot = r.snapshot
  assert.equal(snapshot.generation, 2, 'updateComposition 后 generation 应为 2')

  const finishResult = makeResult({
    outcome: APPLIED, baseRevision: 1, newRevision: 1, displayPatches: [],
    newSelectionStart: 1, newSelectionEnd: 1,
    compositionSession: null,
  })
  r = applyEditResultToSnapshot(snapshot, finishResult)
  assert.equal(r.ok, true)
  snapshot = r.snapshot
  assert.equal(snapshot.generation, 2, 'finishComposition 后 generation 应保留为 2（不重置）')
})

test('applyEditResultToSnapshot: noChange 保留 text 但更新 cursor/selectionAnchor', () => {
  const snapshot = makeSnapshot({ text: 'hello', revision: 3, cursor: 0, selectionAnchor: 0, generation: 0 })
  const result = makeResult({
    outcome: NO_CHANGE, baseRevision: 3, newRevision: 3, displayPatches: [],
    newSelectionStart: 2, newSelectionEnd: 4,
    compositionSession: null,
  })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, true)
  assert.equal(r.snapshot.text, 'hello')
  assert.equal(r.snapshot.revision, 3)
  assert.equal(r.snapshot.cursor, 4)
  assert.equal(r.snapshot.selectionAnchor, 2)
  assert.equal(r.snapshot.generation, 0)
})

test('applyEditResultToSnapshot: appliedWithAdjustedSelection 正常应用', () => {
  const snapshot = makeSnapshot({ text: 'hello', revision: 1, cursor: 0, selectionAnchor: 0, generation: 0 })
  const result = makeResult({
    outcome: APPLIED_WITH_ADJUSTED_SELECTION, baseRevision: 1, newRevision: 2,
    displayPatches: [{
      baseRevision: 1, newRevision: 2,
      replaceByteStart: 5, replaceByteEndExclusive: 5,
      insertedText: ' world', resultingSelectionStart: 6, resultingSelectionEnd: 11,
    }],
    newSelectionStart: 3, newSelectionEnd: 8,
    compositionSession: null,
  })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, true)
  assert.equal(r.snapshot.text, 'hello world')
  assert.equal(r.snapshot.revision, 2)
  assert.equal(r.snapshot.cursor, 8)
  assert.equal(r.snapshot.selectionAnchor, 3)
})

test('applyEditResultToSnapshot: emoji patch 正常应用 text="😀" 后插 "Y" → "😀Y"', () => {
  const snapshot = makeSnapshot({ text: '😀', revision: 1, cursor: 2, selectionAnchor: 2, generation: 0 })
  const result = makeResult({
    outcome: APPLIED, baseRevision: 1, newRevision: 2,
    displayPatches: [{
      baseRevision: 1, newRevision: 2,
      replaceByteStart: 4, replaceByteEndExclusive: 4,
      insertedText: 'Y', resultingSelectionStart: 3, resultingSelectionEnd: 3,
    }],
    newSelectionStart: 3, newSelectionEnd: 3,
    compositionSession: null,
  })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, true)
  assert.equal(r.snapshot.text, '😀Y')
  assert.equal(r.snapshot.revision, 2)
})

// ── 6. applyEditResultToSnapshot — 失败语义：ok=false + reason 明确 ──
console.log('---')
console.log('applyEditResultToSnapshot: 失败语义（ok=false + reason 明确）')

test('applyEditResultToSnapshot: staleRevision → { ok: false, reason: "staleRevision" }', () => {
  const snapshot = makeSnapshot({ text: 'hello', revision: 5, cursor: 3, selectionAnchor: 3, generation: 7 })
  const result = makeResult({
    outcome: STALE_REVISION, baseRevision: 1, newRevision: 99,
    displayPatches: [{
      baseRevision: 1, newRevision: 99,
      replaceByteStart: 0, replaceByteEndExclusive: 5,
      insertedText: 'SHOULD_NOT_APPLY', resultingSelectionStart: 0, resultingSelectionEnd: 0,
    }],
    newSelectionStart: 0, newSelectionEnd: 0,
    compositionSession: { sessionId: 1, baseRevision: 1, generation: 99 },
  })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, false)
  assert.equal(r.reason, 'staleRevision')
  // 失败时不返回 snapshot 字段
  assert.equal(r.snapshot, undefined)
})

test('applyEditResultToSnapshot: invalidOffset → { ok: false, reason: "invalidOffset" }', () => {
  const snapshot = makeSnapshot({ text: 'hello', revision: 5, cursor: 3, selectionAnchor: 3, generation: 7 })
  const result = makeResult({ outcome: INVALID_OFFSET, newRevision: 99 })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, false)
  assert.equal(r.reason, 'invalidOffset')
  assert.equal(r.snapshot, undefined)
})

test('applyEditResultToSnapshot: invalidRange → { ok: false, reason: "invalidRange" }', () => {
  const snapshot = makeSnapshot({ text: 'hello', revision: 5, cursor: 3, selectionAnchor: 3, generation: 7 })
  const result = makeResult({ outcome: INVALID_RANGE, newRevision: 99 })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, false)
  assert.equal(r.reason, 'invalidRange')
  assert.equal(r.snapshot, undefined)
})

test('applyEditResultToSnapshot: 未知 outcome → { ok: false, reason: "unknownOutcome:..." }', () => {
  const snapshot = makeSnapshot({ text: 'hello', revision: 5, cursor: 3, selectionAnchor: 3, generation: 7 })
  const result = makeResult({ outcome: 'somethingWeird', newRevision: 99 })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, false)
  assert.ok(r.reason.startsWith('unknownOutcome:'))
  assert.ok(r.reason.includes('somethingWeird'))
})

test('applyEditResultToSnapshot: patch 失败（非字符边界）→ { ok: false, reason: "patchFailed:..." }', () => {
  const snapshot = makeSnapshot({ text: '你好', revision: 5, cursor: 1, selectionAnchor: 1, generation: 3 })
  const result = makeResult({
    outcome: APPLIED, baseRevision: 5, newRevision: 6,
    displayPatches: [{
      baseRevision: 5, newRevision: 6,
      replaceByteStart: 1, replaceByteEndExclusive: 3,  // 1 非字符边界
      insertedText: 'X', resultingSelectionStart: 0, resultingSelectionEnd: 0,
    }],
    newSelectionStart: 0, newSelectionEnd: 0,
    compositionSession: null,
  })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, false)
  assert.ok(r.reason.startsWith('patchFailed:'), `reason 应以 patchFailed: 开头，实际: ${r.reason}`)
  assert.ok(r.reason.length > 'patchFailed:'.length)
  assert.equal(r.snapshot, undefined)
})

test('applyEditResultToSnapshot: 多 patch 中途失败 → { ok: false, reason: "patchFailed:..." }', () => {
  const snapshot = makeSnapshot({ text: '你好', revision: 1, cursor: 2, selectionAnchor: 2, generation: 0 })
  const result = makeResult({
    outcome: APPLIED, baseRevision: 1, newRevision: 3,
    displayPatches: [
      { baseRevision: 1, newRevision: 2, replaceByteStart: 6, replaceByteEndExclusive: 6,
        insertedText: '世界', resultingSelectionStart: 4, resultingSelectionEnd: 4 },
      { baseRevision: 2, newRevision: 3, replaceByteStart: 1, replaceByteEndExclusive: 3,  // 非边界
        insertedText: 'X', resultingSelectionStart: 0, resultingSelectionEnd: 0 },
    ],
    newSelectionStart: 0, newSelectionEnd: 0,
    compositionSession: null,
  })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, false)
  assert.ok(r.reason.startsWith('patchFailed:'))
  assert.equal(r.snapshot, undefined)
})

test('applyEditResultToSnapshot: compositionSession=null 保留原 generation', () => {
  const snapshot = makeSnapshot({ generation: 5 })
  const result = makeResult({ compositionSession: null })
  const r = applyEditResultToSnapshot(snapshot, result)
  assert.equal(r.ok, true)
  assert.equal(r.snapshot.generation, 5)
})

// ── 7. DTO 形状完整性 ──
console.log('---')
console.log('DTO 形状完整性')

test('DTO 形状: EditorEditResult 所有字段可访问', () => {
  const result = makeResult({
    outcome: APPLIED, transactionId: 42, baseRevision: 1, newRevision: 2,
    displayPatches: [{
      baseRevision: 1, newRevision: 2,
      replaceByteStart: 0, replaceByteEndExclusive: 0,
      insertedText: 'x', resultingSelectionStart: 0, resultingSelectionEnd: 1,
    }],
    oldSelectionStart: 0, oldSelectionEnd: 0,
    newSelectionStart: 0, newSelectionEnd: 1,
    visualIntent: { kind: 'Insert' },
    compositionSession: { sessionId: 7, baseRevision: 1, generation: 3 },
    contentDelta: { insertedChars: 1, deletedChars: 0, insertedNonWhitespaceChars: 1, deletedNonWhitespaceChars: 0 },
  })
  assert.equal(result.outcome, 'applied')
  assert.equal(result.transactionId, 42)
  assert.equal(result.baseRevision, 1)
  assert.equal(result.newRevision, 2)
  assert.equal(Array.isArray(result.displayPatches), true)
  assert.equal(result.displayPatches.length, 1)
  assert.equal(result.visualIntent.kind, 'Insert')
  assert.equal(result.compositionSession.sessionId, 7)
  assert.equal(result.compositionSession.generation, 3)
  assert.equal(result.contentDelta.insertedChars, 1)
})

test('枚举值: 与 Rust EditorEditOutcome 变体字符串对齐', () => {
  assert.equal(APPLIED, 'applied')
  assert.equal(APPLIED_WITH_ADJUSTED_SELECTION, 'appliedWithAdjustedSelection')
  assert.equal(NO_CHANGE, 'noChange')
  assert.equal(STALE_REVISION, 'staleRevision')
  assert.equal(INVALID_OFFSET, 'invalidOffset')
  assert.equal(INVALID_RANGE, 'invalidRange')
  const all = [APPLIED, APPLIED_WITH_ADJUSTED_SELECTION, NO_CHANGE, STALE_REVISION, INVALID_OFFSET, INVALID_RANGE]
  assert.equal(new Set(all).size, 6)
})

// ── 8. SerialCommandQueue 串行行为 ──
console.log('---')
console.log('SerialCommandQueue 串行行为')

await testAsync('SerialCommandQueue: enqueue 顺序保持（3 条命令按顺序执行）', async () => {
  const q = new SerialCommandQueue()
  const order = []
  await Promise.all([
    q.enqueue(async () => { order.push('a') }),
    q.enqueue(async () => { order.push('b') }),
    q.enqueue(async () => { order.push('c') }),
  ])
  assert.deepEqual(order, ['a', 'b', 'c'])
})

await testAsync('SerialCommandQueue: 每条出队才执行（thunk 读到的 revision 是当时的，不是 enqueue 时的）', async () => {
  // 模拟 Coordinator 串行命令：每条命令读当前 revision，执行后 revision+1
  const q = new SerialCommandQueue()
  let revision = 1
  const readRevisions = []
  // 并发 enqueue 3 条命令；每条在 thunk 内读 revision
  const results = await Promise.all([
    q.enqueue(async () => { readRevisions.push(revision); revision += 1; return revision }),
    q.enqueue(async () => { readRevisions.push(revision); revision += 1; return revision }),
    q.enqueue(async () => { readRevisions.push(revision); revision += 1; return revision }),
  ])
  // 每条出队时读到的 revision 是前一条执行后的：1, 2, 3
  assert.deepEqual(readRevisions, [1, 2, 3])
  // 返回值：每条执行后 revision+1：2, 3, 4
  assert.deepEqual(results, [2, 3, 4])
})

await testAsync('SerialCommandQueue: 前一条完成才下一条（async thunk 有延迟，顺序仍保持）', async () => {
  const q = new SerialCommandQueue()
  const order = []
  // 第一条命令延迟 50ms，第二条不延迟；仍应先执行第一条
  await Promise.all([
    q.enqueue(async () => { await new Promise(r => setTimeout(r, 50)); order.push('slow') }),
    q.enqueue(async () => { order.push('fast') }),
  ])
  assert.deepEqual(order, ['slow', 'fast'])
})

await testAsync('SerialCommandQueue: 一条失败不阻塞后续', async () => {
  const q = new SerialCommandQueue()
  const order = []
  // 第一条 reject，第二条应仍执行
  const p1 = q.enqueue(async () => { order.push('a'); throw new Error('boom') })
  const p2 = q.enqueue(async () => { order.push('b'); return 'ok' })
  await p1.catch(() => {})  // 吞掉第一条的 reject
  const r2 = await p2
  assert.deepEqual(order, ['a', 'b'])
  assert.equal(r2, 'ok')
})

await testAsync('SerialCommandQueue: size/isIdle 在执行前后正确', async () => {
  const q = new SerialCommandQueue()
  assert.equal(q.size(), 0)
  assert.equal(q.isIdle(), true)
  const p = q.enqueue(async () => { await new Promise(r => setTimeout(r, 30)); return 1 })
  // enqueue 后未执行完：size>=0, isIdle=false
  assert.equal(q.isIdle(), false)
  await p
  // 执行完后：isIdle=true
  assert.equal(q.isIdle(), true)
})

console.log('---')
console.log(`✅ editor_patch_logic: ${passed} tests passed`)
