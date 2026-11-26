// editor_reconcile_logic.test.mjs — 冲突会话内可恢复的纯逻辑回归
// （Issue #1063 复审评论第 2 条）。
//
// 直接 import 生产模块 session/editor_reconcile_logic.ts，不伪造 ArkUI 运行时。
// 验证：
//   1. 冲突重试成功：attemptFrozenRebase 可把冻结 pendingEdits 映射到新的 Core 基线，
//      返回 consumedEdits；调用方据此清理，状态机 confirmHealthy 后清 conflict。
//   2. 重试失败：模拟返回 consumedEdits=0；调用方保持 conflict 与全部 pendingEdits，
//      不丢事件。
//   3. conflict 不会被自动错误降级成 reconciling；只有 beginManualRetry 能重新尝试。
//   4. 映射到基线：edit 前的 Core 变化可平移范围；与 edit 重叠的差异明确失败。
//
// 运行：node --experimental-strip-types __tests__/editor_reconcile_logic.test.mjs

import { strict as assert } from 'node:assert'
import {
  attemptFrozenRebase,
  mapEditRangeToBaseline,
  ReconcileSessionState,
} from '../editor_reconcile_logic.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// 冻结的 pending edit：结构与 NativeRichEditorBridge.PendingEdit 兼容。
function edit(localSequence, beforeText, startUtf16, endUtf16, insertedText, oldText) {
  return { localSequence, beforeText, startUtf16, endUtf16, insertedText, oldText }
}

// 模拟 bridge 的 retryReconcile：只有 recovered 才丢弃已确认前缀；失败一条不丢。
function runRetry(state, frozen, coreText, nativeText) {
  const pending = frozen.slice()
  state.beginManualRetry('manual conflict retry')
  const attempt = attemptFrozenRebase(pending, coreText)
  if (!attempt.recovered || attempt.text !== nativeText) {
    state.enterConflict(attempt.recovered
      ? 'frozen edits do not reproduce native text'
      : attempt.reason)
    return { state, pending, attempt }
  }
  pending.splice(0, attempt.consumedEdits)
  state.confirmHealthy()
  return { state, pending, attempt }
}

console.log('editor_reconcile_logic：Conflict 是会话内可恢复状态（Issue #1063 复审评论第 2 条）')
console.log('---')

const frozenEdits = [
  edit(1, 'hello world', 0, 5, '你好', 'hello'),
  edit(2, '你好 world', 3, 8, '世界', 'world'),
]

// ── 1. 可验证 rebase ──

test('baseline 未变时 edit 范围原样映射', () => {
  const mapped = mapEditRangeToBaseline(frozenEdits[0], 'hello world')
  assert.deepEqual(mapped, { startUtf16: 0, endUtf16: 5 })
})

test('Core 在 edit 之后追加内容：edit 范围保持不变并成功重放', () => {
  const attempt = attemptFrozenRebase(frozenEdits, 'hello world!!')
  assert.equal(attempt.recovered, true)
  assert.equal(attempt.consumedEdits, frozenEdits.length)
  assert.equal(attempt.text, '你好 世界!!')
})

test('Core 在 edit 之前插入内容：edit 范围按前缀长度平移', () => {
  const shifted = [edit(1, 'world', 0, 5, '世界', 'world')]
  const mapped = mapEditRangeToBaseline(shifted[0], 'hello world')
  assert.deepEqual(mapped, { startUtf16: 6, endUtf16: 11 })
  const attempt = attemptFrozenRebase(shifted, 'hello world')
  assert.equal(attempt.recovered, true)
  assert.equal(attempt.text, 'hello 世界')
})

test('与 edit 重叠的 Core 变化明确失败，consumedEdits=0', () => {
  const attempt = attemptFrozenRebase(frozenEdits, 'HELLO WORLD')
  assert.equal(attempt.recovered, false)
  assert.equal(attempt.consumedEdits, 0)
  assert.match(attempt.reason, /cannot map frozen edit #1/)
})

test('模拟结果必须与原生全文一致，否则仍按冲突处理（不吞事件）', () => {
  const state = new ReconcileSessionState()
  state.enterConflict('original conflict')
  const result = runRetry(state, frozenEdits, 'hello world!!', '你好 世界')
  assert.equal(result.attempt.recovered, true)
  assert.equal(result.pending.length, frozenEdits.length)
  assert.equal(state.getHealth(), 'conflict')
  assert.match(state.getConflictDetail(), /do not reproduce native text/)
})

// ── 2. 冲突重试状态机 ──

test('conflict 重试成功：清 conflict 并只清理已确认事件', () => {
  const state = new ReconcileSessionState()
  state.enterConflict('overlapping frozen edit')
  assert.equal(state.isConflict(), true)

  const result = runRetry(state, frozenEdits, 'hello world!!', '你好 世界!!')
  assert.equal(result.attempt.recovered, true)
  assert.equal(result.pending.length, 0)
  assert.equal(state.getHealth(), 'healthy')
  assert.equal(state.getConflictDetail(), null)
  assert.equal(state.getReconcileReason(), '')
})

test('conflict 重试失败：保持 conflict 与全部 pendingEdits（一条不丢）', () => {
  const state = new ReconcileSessionState()
  state.enterConflict('overlapping frozen edit')
  const result = runRetry(state, frozenEdits, 'HELLO WORLD', '你好 世界')
  assert.equal(result.attempt.recovered, false)
  assert.equal(result.pending.length, frozenEdits.length)
  assert.deepEqual(result.pending, frozenEdits)
  assert.equal(state.getHealth(), 'conflict')
  assert.match(state.getConflictDetail(), /cannot map frozen edit/)
})

test('conflict 不会被自动失败降级成 reconciling', () => {
  const state = new ReconcileSessionState()
  state.enterConflict('terminal-looking conflict')
  assert.equal(state.enterReconciling('automatic retry attempt'), false)
  assert.equal(state.getHealth(), 'conflict')
  assert.equal(state.getConflictDetail(), 'terminal-looking conflict')
})

test('只有 beginManualRetry 能把 conflict 切回 reconciling', () => {
  const state = new ReconcileSessionState()
  state.enterConflict('needs manual retry')
  assert.equal(state.beginManualRetry('user retry'), true)
  assert.equal(state.getHealth(), 'reconciling')
  // 重试期间自动错误仍不能把它拉回 healthy；失败要重新进入 conflict。
  state.enterConflict('retry failed again')
  assert.equal(state.getHealth(), 'conflict')
  assert.equal(state.beginManualRetry('second retry'), true)
  state.confirmHealthy()
  assert.equal(state.getHealth(), 'healthy')
})

test('无冲突时 beginManualRetry 不改变状态', () => {
  const state = new ReconcileSessionState()
  assert.equal(state.beginManualRetry('nothing to retry'), false)
  assert.equal(state.getHealth(), 'healthy')
})

console.log('---')
console.log(`✅ editor_reconcile_logic: ${passed} tests passed`)
