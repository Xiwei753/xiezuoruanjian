// writing_workflow_barrier.test.mjs — 协调器在保存/同步/关闭前的冲突恢复判定
// （Issue #1063 复审评论第 2 条）。
//
// WritingWorkflowCoordinator 依赖 ArkUI/hilog，不能在 node 里 import；本测试不伪造 ArkUI，
// 只 import 生产纯逻辑 session/editor_reconcile_logic.ts 的 isConflictRecoverySatisfied，
// 并按协调器 recoverConflictForBarrier 的真实判定复现两条最小路径：
//   1. 重试失败 → 不进入原屏障、不调用保存（保存失败状态由既有 failSaveAfterBarrierFailure 承担）。
//   2. 重试成功 / NOT_IN_CONFLICT → 继续原屏障与保存。
//   3. 没有注入回调（null）→ 保持原行为。
//   4. 会话切换 / RECONCILE_DEFERRED / 不可合并冲突都按失败收尾，pendingEdits 一条不丢。
//
// 运行：node --experimental-strip-types __tests__/writing_workflow_barrier.test.mjs

import { strict as assert } from 'node:assert'
import { isConflictRecoverySatisfied } from '../editor_reconcile_logic.ts'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// 协调器最小骨架：只保留 recoverConflictForBarrier → 原屏障 → 保存的顺序判定。
// pendingEdits 用计数器表示「不可合并冲突时必须原样保留」。
function makeWorkflow(state) {
  return {
    async performBackgroundSave(retryReconcile) {
      const recovery = retryReconcile === null ? null : await retryReconcile()
      if (!isConflictRecoverySatisfied(recovery)) {
        state.saveFailed = true
        state.events.push('save-failed')
        return false
      }
      state.events.push('barrier')
      state.events.push('save')
      state.saved = true
      return true
    },
  }
}

console.log('writing_workflow_barrier：冲突恢复失败不得进入保存（Issue #1063 复审评论第 2 条）')
console.log('---')

await testAsync('重试失败 → 保存失败且不调用保存，pendingEdits 原样保留', async () => {
  const state = { events: [], saveFailed: false, saved: false, pendingEdits: 3 }
  const workflow = makeWorkflow(state)
  const ok = await workflow.performBackgroundSave(async () => ({
    recovered: false,
    errorCode: 'IME_SETTLEMENT_CONFLICT',
    detail: 'frozen edits do not reproduce native text',
  }))
  assert.equal(ok, false)
  assert.equal(state.saveFailed, true)
  assert.equal(state.saved, false)
  assert.deepEqual(state.events, ['save-failed'])
  assert.equal(state.pendingEdits, 3)
})

await testAsync('重试成功 → 继续原屏障与保存', async () => {
  const state = { events: [], saveFailed: false, saved: false, pendingEdits: 0 }
  const workflow = makeWorkflow(state)
  const ok = await workflow.performBackgroundSave(async () => ({
    recovered: true,
    errorCode: '',
    detail: '',
  }))
  assert.equal(ok, true)
  assert.equal(state.saved, true)
  assert.deepEqual(state.events, ['barrier', 'save'])
})

await testAsync('NOT_IN_CONFLICT（仍 reconciling）→ 继续原屏障，等待交给屏障与超时', async () => {
  const state = { events: [], saveFailed: false, saved: false, pendingEdits: 1 }
  const workflow = makeWorkflow(state)
  const ok = await workflow.performBackgroundSave(async () => ({
    recovered: false,
    errorCode: 'NOT_IN_CONFLICT',
    detail: 'session is reconciling, nothing to retry',
  }))
  assert.equal(ok, true)
  assert.deepEqual(state.events, ['barrier', 'save'])
})

await testAsync('RECONCILE_DEFERRED（重试期间仍有预编辑/原生变更）→ 按失败收尾', async () => {
  const state = { events: [], saveFailed: false, saved: false, pendingEdits: 2 }
  const workflow = makeWorkflow(state)
  const ok = await workflow.performBackgroundSave(async () => ({
    recovered: false,
    errorCode: 'RECONCILE_DEFERRED',
    detail: 'native change or IME composition still in flight',
  }))
  assert.equal(ok, false)
  assert.deepEqual(state.events, ['save-failed'])
  assert.equal(state.pendingEdits, 2)
})

await testAsync('会话切换 → 按失败收尾，不把未确认事件算进保存', async () => {
  const state = { events: [], saveFailed: false, saved: false, pendingEdits: 1 }
  const workflow = makeWorkflow(state)
  const ok = await workflow.performBackgroundSave(async () => ({
    recovered: false,
    errorCode: 'IME_SETTLEMENT_SESSION_CHANGED',
    detail: 'session changed during manual conflict retry',
  }))
  assert.equal(ok, false)
  assert.deepEqual(state.events, ['save-failed'])
})

await testAsync('没有注入回调（null）→ 保持原行为，直接走原屏障', async () => {
  const state = { events: [], saveFailed: false, saved: false, pendingEdits: 0 }
  const workflow = makeWorkflow(state)
  const ok = await workflow.performBackgroundSave(null)
  assert.equal(ok, true)
  assert.deepEqual(state.events, ['barrier', 'save'])
})

test('isConflictRecoverySatisfied 判定表', () => {
  assert.equal(isConflictRecoverySatisfied(null), true)
  assert.equal(isConflictRecoverySatisfied({ recovered: true, errorCode: '', detail: '' }), true)
  assert.equal(isConflictRecoverySatisfied({ recovered: false, errorCode: 'NOT_IN_CONFLICT', detail: '' }), true)
  assert.equal(isConflictRecoverySatisfied({ recovered: true, errorCode: 'NOT_IN_CONFLICT', detail: '' }), true)
  assert.equal(isConflictRecoverySatisfied({ recovered: false, errorCode: 'RECONCILE_DEFERRED', detail: '' }), false)
  assert.equal(isConflictRecoverySatisfied({ recovered: false, errorCode: 'IME_SETTLEMENT_CONFLICT', detail: '' }), false)
  assert.equal(isConflictRecoverySatisfied({ recovered: false, errorCode: 'IME_SETTLEMENT_SESSION_CHANGED', detail: '' }), false)
  assert.equal(isConflictRecoverySatisfied({ recovered: false, errorCode: '', detail: '' }), false)
})

console.log('---')
console.log(`✅ writing_workflow_barrier: ${passed} tests passed`)
