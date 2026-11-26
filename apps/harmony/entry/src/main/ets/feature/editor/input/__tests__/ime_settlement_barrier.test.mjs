// ime_settlement_barrier.test.mjs — IME 结算屏障纯逻辑回归（Issue #1063 复审评论第 2 条）。
//
// 直接 import 生产模块 input/ime_settlement_barrier.ts，不伪造 ArkUI 运行时。
// 验证：
//   1. 退出屏障超时 reject IME_SETTLEMENT_TIMEOUT，绝不 resolve。
//   2. 每个 waiter 独立 deadline/timer：先注册的先超时，后注册的不会被提前结算。
//   3. 会话切换 reject 旧 session waiter 并清掉 timer，新 session waiter 不受影响。
//   4. rejectAll 给页面销毁/切章一个确定失败，不留下悬挂 timer。
//   5. preview 读取失败（unknown）不满足退出屏障；preview 非空也不满足。
//   6. 只有真实 commit/cancel 信号 + preview 明确为空 + 同 revision 三方一致才满足。
//   7. 普通保存屏障在 IME composing 期间可只冻结已提交 Core revision。
//   8. 真实 commit 回调把 waiter 的 requiredSequence 推进到提交事务序号。
//
// 运行：node --experimental-strip-types __tests__/ime_settlement_barrier.test.mjs

import { strict as assert } from 'node:assert'
import {
  ImeSettlementBarrier,
  ImeSettlementError,
  isBarrierWaiterSatisfied,
  IME_SETTLEMENT_TIMEOUT,
  IME_SETTLEMENT_SESSION_CHANGED,
  IME_SETTLEMENT_CANCELLED,
} from '../ime_settlement_barrier.ts'

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
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

function makeWaiter(overrides = {}) {
  return {
    id: 1,
    sessionId: 1,
    barrierRevision: 7,
    mode: 'ime-settlement',
    requiredSequence: 3,
    requireImeSignal: false,
    ...overrides,
  }
}

function makeAlignment(overrides = {}) {
  return {
    coreAvailable: true,
    nativeEditorAvailable: true,
    aligned: true,
    coreRevision: 7,
    appliedRevision: 7,
    coreText: '正文',
    confirmedText: '正文',
    ...overrides,
  }
}

function makeState(overrides = {}) {
  return {
    imeComposing: false,
    previewState: 'empty',
    ...overrides,
  }
}

console.log('ime_settlement_barrier：退出 IME 屏障不得假成功（Issue #1063 复审评论第 2 条）')
console.log('---')

// ── 1. waiter 生命周期 ──

await testAsync('退出屏障超时 reject IME_SETTLEMENT_TIMEOUT，不 resolve', async () => {
  const barrier = new ImeSettlementBarrier(20)
  const reg = barrier.register(1, 7, 3, 'ime-settlement', true)
  let resolved = false
  reg.promise.then(() => { resolved = true }, () => {})
  await sleep(40)
  assert.equal(resolved, false)
  await assert.rejects(reg.promise, (err) => {
    assert.equal(err instanceof ImeSettlementError, true)
    assert.equal(err.errorCode, IME_SETTLEMENT_TIMEOUT)
    return true
  })
  assert.equal(barrier.pendingCount(), 0)
})

await testAsync('每个 waiter 独立 deadline/timer：先注册的先超时，后注册的不被提前结算', async () => {
  const barrier = new ImeSettlementBarrier(40)
  const first = barrier.register(1, 7, 0, 'ime-settlement', true)
  let firstRejected = false
  first.promise.catch(() => { firstRejected = true })
  await sleep(20)
  const second = barrier.register(1, 7, 0, 'ime-settlement', true)
  let secondSettled = false
  second.promise.then(() => { secondSettled = true }, () => { secondSettled = true })
  // 先注册的按自己的 deadline 先失败；后注册的仍挂在账本里。
  await first.promise.catch(() => {})
  assert.equal(firstRejected, true)
  assert.equal(secondSettled, false)
  assert.equal(barrier.pendingCount(), 1)
  await second.promise.catch(() => {})
  assert.equal(secondSettled, true)
})

await testAsync('普通保存屏障不挂 timer，可显式 settle', async () => {
  const barrier = new ImeSettlementBarrier(10)
  const reg = barrier.register(1, 7, 0, 'committed-revision', false)
  let resolved = false
  reg.promise.then(() => { resolved = true })
  await sleep(30)
  assert.equal(resolved, false)
  assert.equal(barrier.settle(reg.id), true)
  await reg.promise
  assert.equal(resolved, true)
})

await testAsync('会话切换 reject 旧 session waiter，新 session waiter 保留', async () => {
  const barrier = new ImeSettlementBarrier(200)
  const oldWaiter = barrier.register(1, 7, 0, 'ime-settlement', true)
  const newWaiter = barrier.register(2, 0, -1, 'committed-revision', false)
  barrier.rejectSession(1, IME_SETTLEMENT_SESSION_CHANGED, 'beginSession')
  await assert.rejects(oldWaiter.promise, (err) => {
    assert.equal(err.errorCode, IME_SETTLEMENT_SESSION_CHANGED)
    return true
  })
  assert.equal(barrier.pendingCount(), 1)
  assert.equal(barrier.settle(newWaiter.id), true)
  await newWaiter.promise
})

await testAsync('rejectAll 清空全部 waiter 并移除 timer', async () => {
  const barrier = new ImeSettlementBarrier(5000)
  const a = barrier.register(1, 1, 0, 'ime-settlement', true)
  const b = barrier.register(1, 1, 0, 'committed-revision', false)
  a.promise.catch(() => {})
  b.promise.catch(() => {})
  assert.equal(barrier.pendingCount(), 2)
  assert.equal(barrier.rejectAll(IME_SETTLEMENT_CANCELLED, 'disposed'), 2)
  assert.equal(barrier.pendingCount(), 0)
  await assert.rejects(a.promise, (err) => err.errorCode === IME_SETTLEMENT_CANCELLED)
  await assert.rejects(b.promise, (err) => err.errorCode === IME_SETTLEMENT_CANCELLED)
})

// ── 2. 结算条件 ──

test('退出屏障：preview 读取失败（unknown）不得当已结算', () => {
  const waiter = makeWaiter({ requireImeSignal: false })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState({ previewState: 'unknown' }), 3), false)
})

test('退出屏障：preview 仍非空 / 仍在 composing 不得结算', () => {
  const waiter = makeWaiter({ requireImeSignal: false })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState({ previewState: 'present' }), 3), false)
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState({ imeComposing: true }), 3), false)
})

test('退出屏障：还没有真实 commit/cancel 信号不得结算', () => {
  const waiter = makeWaiter({ requireImeSignal: true })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState(), 3), false)
})

test('退出屏障：preview 明确取消且正文未变（同 revision 三方一致）才结算', () => {
  const waiter = makeWaiter({ requireImeSignal: false })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState(), 3), true)
})

test('退出屏障：native 与 Core 不同 revision 不得结算', () => {
  const waiter = makeWaiter({ requireImeSignal: false })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment({ coreRevision: 8 }), makeState(), 3), false)
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment({ aligned: false }), makeState(), 3), false)
})

test('退出屏障：提交事务序号未进入 Core 不得结算', () => {
  const waiter = makeWaiter({ requireImeSignal: false, requiredSequence: 5 })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState(), 4), false)
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState(), 5), true)
})

test('普通保存屏障：composing 期间只冻结已提交 Core revision（不要求三方对齐）', () => {
  const waiter = makeWaiter({ mode: 'committed-revision', requireImeSignal: false })
  const composingState = makeState({ imeComposing: true, previewState: 'present' })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment({ aligned: false }), composingState, 3), true)
  // Core revision 与 confirmed 基线不一致时不能放行。
  assert.equal(isBarrierWaiterSatisfied(waiter,
    makeAlignment({ coreRevision: 8, appliedRevision: 8 }), composingState, 3), true)
  assert.equal(isBarrierWaiterSatisfied(waiter,
    makeAlignment({ coreRevision: 8, appliedRevision: 7 }), composingState, 3), false)
})

test('普通保存屏障：无原生编辑器时按已提交 revision 放行', () => {
  const waiter = makeWaiter({ mode: 'committed-revision' })
  const alignment = makeAlignment({ nativeEditorAvailable: false, aligned: false })
  assert.equal(isBarrierWaiterSatisfied(waiter, alignment, makeState(), 3), true)
})

// ── 3. 真实信号绑定 ──

test('真实 commit 回调推进 requiredSequence 并清除信号等待', () => {
  const barrier = new ImeSettlementBarrier(5000)
  const reg = barrier.register(1, 7, 2, 'ime-settlement', true)
  reg.promise.catch(() => {})
  barrier.bindImeCommitSequence(1, 6)
  const waiter = barrier.waiters()[0]
  assert.equal(waiter.requiredSequence, 6)
  assert.equal(waiter.requireImeSignal, false)
  // 只绑定信号不等于已结算；仍要等 Core 应用与对齐。
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment({ appliedRevision: 7 }), makeState(), 4), false)
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment({ appliedRevision: 7 }), makeState(), 6), true)
  assert.equal(barrier.pendingCount(), 1)
  barrier.rejectAll(IME_SETTLEMENT_CANCELLED, 'cleanup')
})

test('真实 cancel 回调不产生新事务，只清除信号等待', () => {
  const barrier = new ImeSettlementBarrier(5000)
  const reg = barrier.register(1, 7, 2, 'ime-settlement', true)
  reg.promise.catch(() => {})
  barrier.clearImeSignal(1)
  const waiter = barrier.waiters()[0]
  assert.equal(waiter.requiredSequence, 2)
  assert.equal(waiter.requireImeSignal, false)
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState(), 2), true)
  barrier.rejectAll(IME_SETTLEMENT_CANCELLED, 'cleanup')
})

test('bindImeCommitSequence 只影响指定 session 的退出 waiter', () => {
  const barrier = new ImeSettlementBarrier(5000)
  const a = barrier.register(1, 7, 0, 'ime-settlement', true)
  const b = barrier.register(2, 0, 0, 'ime-settlement', true)
  a.promise.catch(() => {})
  b.promise.catch(() => {})
  barrier.bindImeCommitSequence(1, 4)
  const waiters = barrier.waiters()
  const waiterA = waiters.find((w) => w.id === a.id)
  const waiterB = waiters.find((w) => w.id === b.id)
  assert.equal(waiterA.requiredSequence, 4)
  assert.equal(waiterB.requiredSequence, 0)
  assert.equal(waiterB.requireImeSignal, true)
  barrier.rejectAll(IME_SETTLEMENT_CANCELLED, 'cleanup')
})

console.log('---')
console.log(`✅ ime_settlement_barrier: ${passed} tests passed`)
