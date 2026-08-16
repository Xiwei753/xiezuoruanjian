// editor_session_patch.test.mjs — editor_patch_logic.ts 的纯逻辑单测。
//
// 用 Node --experimental-strip-types 直接 import editor_patch_logic.ts（纯 TS，无 ArkUI 依赖）。
// 验证：
//   1. applyPatch 基础：纯插入/纯删除/替换/全替换/空文本插入
//   2. applyPatch 多 patch 顺序应用
//   3. applyEditResultToSnapshot — applied 正常更新 text/revision/cursor/selectionAnchor
//   4. applyEditResultToSnapshot — composition generation 流转（begin→update→finish）
//   5. applyEditResultToSnapshot — staleRevision 不更新
//   6. applyEditResultToSnapshot — invalidOffset 不更新
//   7. applyEditResultToSnapshot — invalidRange 不更新
//   8. applyEditResultToSnapshot — noChange 保留 revision 但更新 selection
//   9. applyEditResultToSnapshot — appliedWithAdjustedSelection 正常应用
//  10. DTO 形状完整性：EditorEditResult / DisplayPatch 所有字段可访问
//
// 运行：node --experimental-strip-types editor_session_patch.test.mjs

import { strict as assert } from 'node:assert'
import {
  applyPatch,
  applyPatchStrict,
  utf8ByteOffsetToUtf16,
  applyEditResultToSnapshot,
  APPLIED,
  APPLIED_WITH_ADJUSTED_SELECTION,
  NO_CHANGE,
  STALE_REVISION,
  INVALID_OFFSET,
  INVALID_RANGE,
} from '../editor_patch_logic.ts'

let passed = 0
const test = (name, fn) => {
  fn()
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
// ── 11. utf8ByteOffsetToUtf16 基础边界映射 ──
test('utf8ByteOffsetToUtf16: ASCII text="abc" byteOffset 0/1/2/3 → 0/1/2/3', () => {
  assert.equal(utf8ByteOffsetToUtf16('abc', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('abc', 1), 1)
  assert.equal(utf8ByteOffsetToUtf16('abc', 2), 2)
  assert.equal(utf8ByteOffsetToUtf16('abc', 3), 3)
})

test('utf8ByteOffsetToUtf16: 中文 text="你好" byteOffset 0/3/6 → 0/1/2', () => {
  // 你 = UTF-8 3 字节, UTF-16 1 code unit；好 同理
  assert.equal(utf8ByteOffsetToUtf16('你好', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('你好', 3), 1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 6), 2)
})

test('utf8ByteOffsetToUtf16: emoji text="😀" byteOffset 0/4 → 0/2', () => {
  // 😀 = U+1F600, UTF-8 4 字节, UTF-16 surrogate pair 2 code unit
  assert.equal(utf8ByteOffsetToUtf16('😀', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('😀', 4), 2)
})

test('utf8ByteOffsetToUtf16: 混合 text="A你B" byteOffset 0/1/4/5 → 0/1/2/3', () => {
  // A=1B, 你=3B, B=1B → 总 5 字节；UTF-16: A=1, 你=1, B=1 → 总 3 code unit
  assert.equal(utf8ByteOffsetToUtf16('A你B', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('A你B', 1), 1)
  assert.equal(utf8ByteOffsetToUtf16('A你B', 4), 2)
  assert.equal(utf8ByteOffsetToUtf16('A你B', 5), 3)
})

test('utf8ByteOffsetToUtf16: 中文+emoji text="你😀好" 边界 0/3/7/10 → 0/1/3/4', () => {
  // 你=3B/1u, 😀=4B/2u, 好=3B/1u → 总 10 字节, 4 code unit
  assert.equal(utf8ByteOffsetToUtf16('你😀好', 0), 0)
  assert.equal(utf8ByteOffsetToUtf16('你😀好', 3), 1)
  assert.equal(utf8ByteOffsetToUtf16('你😀好', 7), 3)
  assert.equal(utf8ByteOffsetToUtf16('你😀好', 10), 4)
})

// ── 12. utf8ByteOffsetToUtf16 非字符边界/越界失败 ──
test('utf8ByteOffsetToUtf16: 非字符边界 text="你好" byteOffset 1/2/4/5 → -1', () => {
  assert.equal(utf8ByteOffsetToUtf16('你好', 1), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 2), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 4), -1)
  assert.equal(utf8ByteOffsetToUtf16('你好', 5), -1)
})

test('utf8ByteOffsetToUtf16: emoji 中间 byteOffset 1/2/3 → -1（落在 surrogate pair 中间）', () => {
  // 😀 UTF-8 4 字节，byteOffset 1/2/3 落在 emoji 中间
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

// ── 13. applyPatchStrict 中文/emoji 成功路径 ──
test('applyPatchStrict: 中文中间插入 text="你好" [3,3) 插 "X" → "你X好"', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 3,
    replaceByteEndExclusive: 3,
    insertedText: 'X',
    resultingSelectionStart: 2,
    resultingSelectionEnd: 2,
  }
  const r = applyPatchStrict('你好', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, '你X好')
})

test('applyPatchStrict: 中文范围删除 text="你好" [3,6) 删 → "你"', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 3,
    replaceByteEndExclusive: 6,
    insertedText: '',
    resultingSelectionStart: 1,
    resultingSelectionEnd: 1,
  }
  const r = applyPatchStrict('你好', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, '你')
})

test('applyPatchStrict: 混合文本替换 text="A你B" [1,4) 替换为 "他" → "A他B"', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 1,
    replaceByteEndExclusive: 4,
    insertedText: '他',
    resultingSelectionStart: 1,
    resultingSelectionEnd: 2,
  }
  const r = applyPatchStrict('A你B', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, 'A他B')
})

test('applyPatchStrict: emoji 前插入 text="😀" [0,0) 插 "X" → "X😀"', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 0,
    replaceByteEndExclusive: 0,
    insertedText: 'X',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 1,
  }
  const r = applyPatchStrict('😀', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, 'X😀')
})

test('applyPatchStrict: emoji 后插入 text="😀" [4,4) 插 "X" → "😀X"', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 4,
    replaceByteEndExclusive: 4,
    insertedText: 'X',
    resultingSelectionStart: 2,
    resultingSelectionEnd: 3,
  }
  const r = applyPatchStrict('😀', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, '😀X')
})

test('applyPatchStrict: emoji 删除 text="X😀Y" [1,5) 删 → "XY"', () => {
  // X=1B, 😀=4B, Y=1B → 总 6 字节；[1,5) 删除 😀
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 1,
    replaceByteEndExclusive: 5,
    insertedText: '',
    resultingSelectionStart: 1,
    resultingSelectionEnd: 1,
  }
  const r = applyPatchStrict('X😀Y', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, 'XY')
})

test('applyPatchStrict: 中文+emoji 混合删除 text="你😀好" [3,7) 删 emoji → "你好"', () => {
  // 你=3B, 😀=4B, 好=3B → [3,7) 是 😀
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 3,
    replaceByteEndExclusive: 7,
    insertedText: '',
    resultingSelectionStart: 1,
    resultingSelectionEnd: 1,
  }
  const r = applyPatchStrict('你😀好', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, '你好')
})

test('applyPatchStrict: 中文+emoji 混合替换 text="你😀好" [3,7) 替换为 "X" → "你X好"', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 3,
    replaceByteEndExclusive: 7,
    insertedText: 'X',
    resultingSelectionStart: 1,
    resultingSelectionEnd: 2,
  }
  const r = applyPatchStrict('你😀好', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, '你X好')
})

test('applyPatchStrict: emoji 整体替换 text="😀" [0,4) 替换为 "AB" → "AB"', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 0,
    replaceByteEndExclusive: 4,
    insertedText: 'AB',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 2,
  }
  const r = applyPatchStrict('😀', patch)
  assert.equal(r.ok, true)
  assert.equal(r.text, 'AB')
})

// ── 14. applyPatchStrict 失败语义（非字符边界/越界） ──
test('applyPatchStrict: 非字符边界 startOffset 必须失败 text="你好" [1,3)', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 1,
    replaceByteEndExclusive: 3,
    insertedText: 'X',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 0,
  }
  const r = applyPatchStrict('你好', patch)
  assert.equal(r.ok, false)
  assert.equal(typeof r.reason, 'string')
  assert.ok(r.reason.length > 0)
})

test('applyPatchStrict: 非字符边界 endOffset 必须失败 text="你好" [0,4)', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 0,
    replaceByteEndExclusive: 4,
    insertedText: 'X',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 0,
  }
  const r = applyPatchStrict('你好', patch)
  assert.equal(r.ok, false)
  assert.equal(typeof r.reason, 'string')
})

test('applyPatchStrict: 越界 byteOffset 必须失败 text="你好" [100,100)', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 100,
    replaceByteEndExclusive: 100,
    insertedText: 'X',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 0,
  }
  const r = applyPatchStrict('你好', patch)
  assert.equal(r.ok, false)
  assert.equal(typeof r.reason, 'string')
})

test('applyPatchStrict: emoji 中间边界必须失败 text="😀" [2,2)', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 2,
    replaceByteEndExclusive: 2,
    insertedText: 'X',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 0,
  }
  const r = applyPatchStrict('😀', patch)
  assert.equal(r.ok, false)
  assert.equal(typeof r.reason, 'string')
})

test('applyPatchStrict: 负 offset 必须失败 text="abc" [-1,0)', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: -1,
    replaceByteEndExclusive: 0,
    insertedText: 'X',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 0,
  }
  const r = applyPatchStrict('abc', patch)
  assert.equal(r.ok, false)
  assert.equal(typeof r.reason, 'string')
})

// ── 15. applyEditResultToSnapshot 中文/emoji 与失败恢复 ──
test('applyEditResultToSnapshot: 中文 patch 正常应用 text="你好" 末尾插 "世界" → "你好世界"', () => {
  const snapshot = makeSnapshot({
    text: '你好',
    revision: 1,
    cursor: 2,
    selectionAnchor: 2,
    generation: 0,
    chapterId: 'c1',
  })
  const result = makeResult({
    outcome: APPLIED,
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [{
      baseRevision: 1,
      newRevision: 2,
      replaceByteStart: 6,
      replaceByteEndExclusive: 6,
      insertedText: '世界',
      resultingSelectionStart: 4,
      resultingSelectionEnd: 4,
    }],
    newSelectionStart: 4,
    newSelectionEnd: 4,
    compositionSession: null,
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  assert.equal(next.text, '你好世界')
  assert.equal(next.revision, 2)
  assert.equal(next.cursor, 4)
  assert.equal(next.selectionAnchor, 4)
})

test('applyEditResultToSnapshot: 多 patch 顺序应用含中文 → "大家你好世界"', () => {
  const snapshot = makeSnapshot({
    text: '你好',
    revision: 1,
    cursor: 2,
    selectionAnchor: 2,
    generation: 0,
    chapterId: 'c1',
  })
  const result = makeResult({
    outcome: APPLIED,
    baseRevision: 1,
    newRevision: 3,
    displayPatches: [
      {
        // 末尾插 "世界"：基于 "你好"（6 字节），[6,6) 插
        baseRevision: 1,
        newRevision: 2,
        replaceByteStart: 6,
        replaceByteEndExclusive: 6,
        insertedText: '世界',
        resultingSelectionStart: 4,
        resultingSelectionEnd: 4,
      },
      {
        // 开头插 "大家"：基于 "你好世界"（12 字节），[0,0) 插
        baseRevision: 2,
        newRevision: 3,
        replaceByteStart: 0,
        replaceByteEndExclusive: 0,
        insertedText: '大家',
        resultingSelectionStart: 0,
        resultingSelectionEnd: 2,
      },
    ],
    newSelectionStart: 0,
    newSelectionEnd: 2,
    compositionSession: null,
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  assert.equal(next.text, '大家你好世界')
  assert.equal(next.revision, 3)
})

test('applyEditResultToSnapshot: patch 失败时返回原 snapshot（非字符边界）', () => {
  const snapshot = makeSnapshot({
    text: '你好',
    revision: 5,
    cursor: 1,
    selectionAnchor: 1,
    generation: 3,
    chapterId: 'c1',
  })
  const result = makeResult({
    outcome: APPLIED,
    baseRevision: 5,
    newRevision: 6,
    displayPatches: [{
      // replaceByteStart=1 非字符边界
      baseRevision: 5,
      newRevision: 6,
      replaceByteStart: 1,
      replaceByteEndExclusive: 3,
      insertedText: 'X',
      resultingSelectionStart: 0,
      resultingSelectionEnd: 0,
    }],
    newSelectionStart: 0,
    newSelectionEnd: 0,
    compositionSession: null,
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  // 返回原 snapshot 引用，让上层从 Core snapshot() 恢复
  assert.equal(next, snapshot)
  assert.equal(next.text, '你好')
  assert.equal(next.revision, 5)
  assert.equal(next.generation, 3)
})

test('applyEditResultToSnapshot: 多 patch 中途失败返回原 snapshot', () => {
  const snapshot = makeSnapshot({
    text: '你好',
    revision: 1,
    cursor: 2,
    selectionAnchor: 2,
    generation: 0,
    chapterId: 'c1',
  })
  const result = makeResult({
    outcome: APPLIED,
    baseRevision: 1,
    newRevision: 3,
    displayPatches: [
      {
        // 第一个 patch 正常：末尾插 "世界"
        baseRevision: 1,
        newRevision: 2,
        replaceByteStart: 6,
        replaceByteEndExclusive: 6,
        insertedText: '世界',
        resultingSelectionStart: 4,
        resultingSelectionEnd: 4,
      },
      {
        // 第二个 patch 失败：基于 "你好世界"（12 字节），replaceByteStart=1 非边界
        baseRevision: 2,
        newRevision: 3,
        replaceByteStart: 1,
        replaceByteEndExclusive: 3,
        insertedText: 'X',
        resultingSelectionStart: 0,
        resultingSelectionEnd: 0,
      },
    ],
    newSelectionStart: 0,
    newSelectionEnd: 0,
    compositionSession: null,
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  // 中途失败 → 返回原 snapshot
  assert.equal(next, snapshot)
  assert.equal(next.text, '你好')
  assert.equal(next.revision, 1)
})

test('applyEditResultToSnapshot: emoji patch 正常应用 text="😀" 后插 "Y" → "😀Y"', () => {
  const snapshot = makeSnapshot({
    text: '😀',
    revision: 1,
    cursor: 2,
    selectionAnchor: 2,
    generation: 0,
    chapterId: 'c1',
  })
  const result = makeResult({
    outcome: APPLIED,
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [{
      baseRevision: 1,
      newRevision: 2,
      replaceByteStart: 4,
      replaceByteEndExclusive: 4,
      insertedText: 'Y',
      resultingSelectionStart: 3,
      resultingSelectionEnd: 3,
    }],
    newSelectionStart: 3,
    newSelectionEnd: 3,
    compositionSession: null,
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  assert.equal(next.text, '😀Y')
  assert.equal(next.revision, 2)
})


console.log('---')

// ── 1. applyPatch 基础 ──
test('applyPatch: 纯插入 text="hello" 末尾插 " world" → "hello world"', () => {
  const text = 'hello'
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 5,
    replaceByteEndExclusive: 5,
    insertedText: ' world',
    resultingSelectionStart: 6,
    resultingSelectionEnd: 11,
  }
  assert.equal(applyPatch(text, patch), 'hello world')
})

test('applyPatch: 纯删除 text="hello world" 删 [5,11) → "hello"', () => {
  const text = 'hello world'
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 5,
    replaceByteEndExclusive: 11,
    insertedText: '',
    resultingSelectionStart: 5,
    resultingSelectionEnd: 5,
  }
  assert.equal(applyPatch(text, patch), 'hello')
})

test('applyPatch: 替换 text="hello" 替换 [1,3) 为 "EL" → "hELlo"', () => {
  const text = 'hello'
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 1,
    replaceByteEndExclusive: 3,
    insertedText: 'EL',
    resultingSelectionStart: 1,
    resultingSelectionEnd: 3,
  }
  assert.equal(applyPatch(text, patch), 'hELlo')
})

test('applyPatch: 全替换 text="abc" 替换 [0,3) 为 "xyz" → "xyz"', () => {
  const text = 'abc'
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 0,
    replaceByteEndExclusive: 3,
    insertedText: 'xyz',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 3,
  }
  assert.equal(applyPatch(text, patch), 'xyz')
})

test('applyPatch: 空文本插入 text="" [0,0) 插 "abc" → "abc"', () => {
  const text = ''
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 0,
    replaceByteEndExclusive: 0,
    insertedText: 'abc',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 3,
  }
  assert.equal(applyPatch(text, patch), 'abc')
})

// ── 2. applyPatch 多 patch 顺序应用 ──
test('applyPatch: 多 patch 顺序应用 text="abc" 先末尾插 "def" 再开头插 "XYZ" → "XYZabcdef"', () => {
  const text = 'abc'
  const patch1 = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 3,
    replaceByteEndExclusive: 3,
    insertedText: 'def',
    resultingSelectionStart: 3,
    resultingSelectionEnd: 6,
  }
  const patch2 = {
    baseRevision: 2,
    newRevision: 3,
    replaceByteStart: 0,
    replaceByteEndExclusive: 0,
    insertedText: 'XYZ',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 3,
  }
  const after1 = applyPatch(text, patch1)
  assert.equal(after1, 'abcdef')
  const after2 = applyPatch(after1, patch2)
  assert.equal(after2, 'XYZabcdef')
})

test('applyPatch: 多 patch 独立应用 — patch2 应用到 patch1 结果上等价于顺序应用', () => {
  const text = 'abc'
  const patch1 = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 3,
    replaceByteEndExclusive: 3,
    insertedText: 'def',
    resultingSelectionStart: 3,
    resultingSelectionEnd: 6,
  }
  const patch2 = {
    baseRevision: 2,
    newRevision: 3,
    replaceByteStart: 0,
    replaceByteEndExclusive: 0,
    insertedText: 'XYZ',
    resultingSelectionStart: 0,
    resultingSelectionEnd: 3,
  }
  const sequential = applyPatch(applyPatch(text, patch1), patch2)
  // 直接构造期望结果
  assert.equal(sequential, 'XYZabcdef')
  // 长度守恒：原 3 + 插 3 + 插 3 = 9
  assert.equal(sequential.length, 9)
})

// ── 3. applyEditResultToSnapshot — applied 正常更新 ──
test('applyEditResultToSnapshot: applied 正常更新 text/revision/cursor/selectionAnchor', () => {
  const snapshot = makeSnapshot({
    text: 'hello',
    revision: 1,
    cursor: 5,
    selectionAnchor: 5,
    generation: 0,
    chapterId: 'c1',
  })
  const result = makeResult({
    outcome: APPLIED,
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [{
      baseRevision: 1,
      newRevision: 2,
      replaceByteStart: 5,
      replaceByteEndExclusive: 5,
      insertedText: '!',
      resultingSelectionStart: 6,
      resultingSelectionEnd: 6,
    }],
    newSelectionStart: 6,
    newSelectionEnd: 6,
    compositionSession: null,
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  assert.equal(next.text, 'hello!')
  assert.equal(next.revision, 2)
  assert.equal(next.cursor, 6)
  assert.equal(next.selectionAnchor, 6)
  // compositionSession null → 保留原 generation
  assert.equal(next.generation, 0)
  assert.equal(next.chapterId, 'c1')
})

// ── 4. applyEditResultToSnapshot — composition generation 流转 ──
test('applyEditResultToSnapshot: composition generation 流转 begin→update→finish', () => {
  // 初始 generation=0
  let snapshot = makeSnapshot({
    text: 'a',
    revision: 1,
    cursor: 1,
    selectionAnchor: 1,
    generation: 0,
    chapterId: 'c1',
  })

  // beginComposition: compositionSession={sessionId:42, baseRevision:1, generation:1}
  const beginResult = makeResult({
    outcome: APPLIED,
    baseRevision: 1,
    newRevision: 1,
    displayPatches: [],
    newSelectionStart: 1,
    newSelectionEnd: 1,
    compositionSession: { sessionId: 42, baseRevision: 1, generation: 1 },
  })
  snapshot = applyEditResultToSnapshot(snapshot, beginResult)
  assert.equal(snapshot.generation, 1, 'beginComposition 后 generation 应为 1')

  // updateComposition: compositionSession generation=2
  const updateResult = makeResult({
    outcome: APPLIED,
    baseRevision: 1,
    newRevision: 1,
    displayPatches: [],
    newSelectionStart: 1,
    newSelectionEnd: 1,
    compositionSession: { sessionId: 42, baseRevision: 1, generation: 2 },
  })
  snapshot = applyEditResultToSnapshot(snapshot, updateResult)
  assert.equal(snapshot.generation, 2, 'updateComposition 后 generation 应为 2')

  // finishComposition: compositionSession=null → generation 保留为 2
  const finishResult = makeResult({
    outcome: APPLIED,
    baseRevision: 1,
    newRevision: 1,
    displayPatches: [],
    newSelectionStart: 1,
    newSelectionEnd: 1,
    compositionSession: null,
  })
  snapshot = applyEditResultToSnapshot(snapshot, finishResult)
  assert.equal(snapshot.generation, 2, 'finishComposition 后 generation 应保留为 2（不重置）')
})

// ── 5. applyEditResultToSnapshot — staleRevision 不更新 ──
test('applyEditResultToSnapshot: staleRevision 返回原 snapshot 不变', () => {
  const snapshot = makeSnapshot({
    text: 'hello',
    revision: 5,
    cursor: 3,
    selectionAnchor: 3,
    generation: 7,
    chapterId: 'c1',
  })
  const result = makeResult({
    outcome: STALE_REVISION,
    baseRevision: 1,
    newRevision: 99,  // 这些字段不应被采用
    displayPatches: [{
      baseRevision: 1,
      newRevision: 99,
      replaceByteStart: 0,
      replaceByteEndExclusive: 5,
      insertedText: 'SHOULD_NOT_APPLY',
      resultingSelectionStart: 0,
      resultingSelectionEnd: 0,
    }],
    newSelectionStart: 0,
    newSelectionEnd: 0,
    compositionSession: { sessionId: 1, baseRevision: 1, generation: 99 },
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  // 引用相等：返回的就是原 snapshot
  assert.equal(next, snapshot)
  assert.equal(next.text, 'hello')
  assert.equal(next.revision, 5)
  assert.equal(next.generation, 7)
})

// ── 6. applyEditResultToSnapshot — invalidOffset 不更新 ──
test('applyEditResultToSnapshot: invalidOffset 返回原 snapshot 不变', () => {
  const snapshot = makeSnapshot({
    text: 'hello',
    revision: 5,
    cursor: 3,
    selectionAnchor: 3,
    generation: 7,
    chapterId: 'c1',
  })
  const result = makeResult({
    outcome: INVALID_OFFSET,
    baseRevision: 1,
    newRevision: 99,
    displayPatches: [],
    newSelectionStart: 0,
    newSelectionEnd: 0,
    compositionSession: null,
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  assert.equal(next, snapshot)
  assert.equal(next.revision, 5)
})

// ── 7. applyEditResultToSnapshot — invalidRange 不更新 ──
test('applyEditResultToSnapshot: invalidRange 返回原 snapshot 不变', () => {
  const snapshot = makeSnapshot({
    text: 'hello',
    revision: 5,
    cursor: 3,
    selectionAnchor: 3,
    generation: 7,
    chapterId: 'c1',
  })
  const result = makeResult({
    outcome: INVALID_RANGE,
    baseRevision: 1,
    newRevision: 99,
    displayPatches: [],
    newSelectionStart: 0,
    newSelectionEnd: 0,
    compositionSession: null,
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  assert.equal(next, snapshot)
  assert.equal(next.revision, 5)
})

// ── 8. applyEditResultToSnapshot — noChange 保留 revision 但更新 selection ──
test('applyEditResultToSnapshot: noChange 保留 text 但更新 cursor/selectionAnchor', () => {
  const snapshot = makeSnapshot({
    text: 'hello',
    revision: 3,
    cursor: 0,
    selectionAnchor: 0,
    generation: 0,
    chapterId: 'c1',
  })
  // noChange: newRevision 同 baseRevision（不前进），displayPatches=[]
  const result = makeResult({
    outcome: NO_CHANGE,
    baseRevision: 3,
    newRevision: 3,
    displayPatches: [],
    newSelectionStart: 2,
    newSelectionEnd: 4,
    compositionSession: null,
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  // text 不变
  assert.equal(next.text, 'hello')
  // revision 用 newRevision（=3，未前进）
  assert.equal(next.revision, 3)
  // cursor/selectionAnchor 用 newSelectionEnd/newSelectionStart 更新
  assert.equal(next.cursor, 4)
  assert.equal(next.selectionAnchor, 2)
  // generation 保留（compositionSession null）
  assert.equal(next.generation, 0)
})

// ── 9. applyEditResultToSnapshot — appliedWithAdjustedSelection ──
test('applyEditResultToSnapshot: appliedWithAdjustedSelection 正常应用 patch 和更新 selection', () => {
  const snapshot = makeSnapshot({
    text: 'hello',
    revision: 1,
    cursor: 0,
    selectionAnchor: 0,
    generation: 0,
    chapterId: 'c1',
  })
  const result = makeResult({
    outcome: APPLIED_WITH_ADJUSTED_SELECTION,
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [{
      baseRevision: 1,
      newRevision: 2,
      replaceByteStart: 5,
      replaceByteEndExclusive: 5,
      insertedText: ' world',
      resultingSelectionStart: 6,
      resultingSelectionEnd: 11,
    }],
    newSelectionStart: 3,
    newSelectionEnd: 8,
    compositionSession: null,
  })
  const next = applyEditResultToSnapshot(snapshot, result)
  assert.equal(next.text, 'hello world')
  assert.equal(next.revision, 2)
  // selection 用 newSelectionStart/End（adjusted 后的）
  assert.equal(next.cursor, 8)
  assert.equal(next.selectionAnchor, 3)
})

// ── 10. DTO 形状完整性 ──
test('DTO 形状: EditorEditResult 所有字段可访问', () => {
  const result = makeResult({
    outcome: APPLIED,
    transactionId: 42,
    baseRevision: 1,
    newRevision: 2,
    displayPatches: [{
      baseRevision: 1,
      newRevision: 2,
      replaceByteStart: 0,
      replaceByteEndExclusive: 0,
      insertedText: 'x',
      resultingSelectionStart: 0,
      resultingSelectionEnd: 1,
    }],
    oldSelectionStart: 0,
    oldSelectionEnd: 0,
    newSelectionStart: 0,
    newSelectionEnd: 1,
    visualIntent: { kind: 'Insert' },
    compositionSession: { sessionId: 7, baseRevision: 1, generation: 3 },
    contentDelta: {
      insertedChars: 1,
      deletedChars: 0,
      insertedNonWhitespaceChars: 1,
      deletedNonWhitespaceChars: 0,
    },
  })
  // 全部字段可读
  assert.equal(result.outcome, 'applied')
  assert.equal(result.transactionId, 42)
  assert.equal(result.baseRevision, 1)
  assert.equal(result.newRevision, 2)
  assert.equal(Array.isArray(result.displayPatches), true)
  assert.equal(result.displayPatches.length, 1)
  assert.equal(result.oldSelectionStart, 0)
  assert.equal(result.oldSelectionEnd, 0)
  assert.equal(result.newSelectionStart, 0)
  assert.equal(result.newSelectionEnd, 1)
  assert.equal(result.visualIntent.kind, 'Insert')
  assert.equal(result.compositionSession.sessionId, 7)
  assert.equal(result.compositionSession.baseRevision, 1)
  assert.equal(result.compositionSession.generation, 3)
  assert.equal(result.contentDelta.insertedChars, 1)
  assert.equal(result.contentDelta.deletedChars, 0)
  assert.equal(result.contentDelta.insertedNonWhitespaceChars, 1)
  assert.equal(result.contentDelta.deletedNonWhitespaceChars, 0)
})

test('DTO 形状: DisplayPatch 所有字段可访问', () => {
  const patch = {
    baseRevision: 1,
    newRevision: 2,
    replaceByteStart: 3,
    replaceByteEndExclusive: 7,
    insertedText: 'replacement',
    resultingSelectionStart: 3,
    resultingSelectionEnd: 13,
  }
  assert.equal(patch.baseRevision, 1)
  assert.equal(patch.newRevision, 2)
  assert.equal(patch.replaceByteStart, 3)
  assert.equal(patch.replaceByteEndExclusive, 7)
  assert.equal(patch.insertedText, 'replacement')
  assert.equal(patch.resultingSelectionStart, 3)
  assert.equal(patch.resultingSelectionEnd, 13)
})

test('DTO 形状: compositionSession=null 表示非 composition 命令', () => {
  const result = makeResult({ compositionSession: null })
  assert.equal(result.compositionSession, null)
  // applyEditResultToSnapshot 在 null 时保留原 generation
  const snapshot = makeSnapshot({ generation: 5 })
  const next = applyEditResultToSnapshot(snapshot, result)
  assert.equal(next.generation, 5)
})

test('枚举值: 与 Rust EditorEditOutcome 变体字符串对齐', () => {
  assert.equal(APPLIED, 'applied')
  assert.equal(APPLIED_WITH_ADJUSTED_SELECTION, 'appliedWithAdjustedSelection')
  assert.equal(NO_CHANGE, 'noChange')
  assert.equal(STALE_REVISION, 'staleRevision')
  assert.equal(INVALID_OFFSET, 'invalidOffset')
  assert.equal(INVALID_RANGE, 'invalidRange')
  // 6 个互异值
  const all = [APPLIED, APPLIED_WITH_ADJUSTED_SELECTION, NO_CHANGE, STALE_REVISION, INVALID_OFFSET, INVALID_RANGE]
  assert.equal(new Set(all).size, 6)
})

console.log('---')
console.log(`✅ editor_patch_logic: ${passed} tests passed`)
