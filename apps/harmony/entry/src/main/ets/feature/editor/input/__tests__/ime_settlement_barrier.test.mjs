// ime_settlement_barrier.test.mjs — IME 结算屏障纯逻辑回归（Issue #1063 复审评论第 2 条）。
//
// 直接 import 生产模块 input/ime_settlement_barrier.ts，不伪造 ArkUI 运行时。
// 验证：
//   1. 退出屏障超时 reject IME_SETTLEMENT_TIMEOUT，绝不 resolve。
//   2. 每个 waiter 独立 deadline/timer：先注册的先超时，后注册的不会被提前结算。
//   3. 会话切换 reject 旧 session waiter 并清掉 timer，新 session waiter 不受影响。
//   4. rejectAll 给页面销毁/切章一个确定失败，不留下悬挂 timer。
//   5. preview 读取失败（unknown）不满足退出屏障；preview 非空也不满足；unknown ≠ empty。
//   6. 只有真实 commit/cancel 信号 + preview 明确为空 + 同 revision 三方一致才满足。
//   7. 普通保存屏障在 IME composing 期间可只冻结已提交 Core revision；
//      Core 快照不可用、原生 loading/read-failed、Core 会话不匹配都不得成功。
//   8. 真实 commit 回调把 waiter 的 requiredSequence 推进到提交事务序号。
//   9. 普通保存屏障挂独立 timer，超时 reject SAVE_BARRIER_TIMEOUT（可重试，不伪造成功）。
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
  SAVE_BARRIER_TIMEOUT,
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
    coreSessionId: 1,
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
    nativeEditorState: 'ready',
    aligned: true,
    coreRevision: 7,
    appliedRevision: 7,
    coreText: '正文',
    confirmedText: '正文',
    coreSessionId: 1,
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

console.log('ime_settlement_barrier：退出/普通保存屏障不得假成功（Issue #1063 复审评论第 2 条）')
console.log('---')

// ── 1. waiter 生命周期 ──

await testAsync('退出屏障超时 reject IME_SETTLEMENT_TIMEOUT，不 resolve', async () => {
  const barrier = new ImeSettlementBarrier(20)
  const reg = barrier.register(1, 1, 7, 3, 'ime-settlement', true)
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
  const first = barrier.register(1, 1, 7, 0, 'ime-settlement', true)
  let firstRejected = false
  first.promise.catch(() => { firstRejected = true })
  await sleep(20)
  const second = barrier.register(1, 1, 7, 0, 'ime-settlement', true)
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

await testAsync('普通保存屏障也挂独立 timer，超时 reject SAVE_BARRIER_TIMEOUT 而不是 resolve', async () => {
  const barrier = new ImeSettlementBarrier(200, 20)
  const reg = barrier.register(1, 1, 7, 0, 'committed-revision', false)
  let resolved = false
  reg.promise.then(() => { resolved = true }, () => {})
  await sleep(40)
  assert.equal(resolved, false)
  await assert.rejects(reg.promise, (err) => {
    assert.equal(err instanceof ImeSettlementError, true)
    assert.equal(err.errorCode, SAVE_BARRIER_TIMEOUT)
    return true
  })
  assert.equal(barrier.pendingCount(), 0)
})

await testAsync('普通保存屏障未被 settle 前保持挂起，settle 后 resolve', async () => {
  const barrier = new ImeSettlementBarrier(20, 5000)
  const reg = barrier.register(1, 1, 7, 0, 'committed-revision', false)
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
  const oldWaiter = barrier.register(1, 1, 7, 0, 'ime-settlement', true)
  const newWaiter = barrier.register(2, 2, 0, -1, 'committed-revision', false)
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
  const a = barrier.register(1, 1, 1, 0, 'ime-settlement', true)
  const b = barrier.register(1, 1, 1, 0, 'committed-revision', false)
  a.promise.catch(() => {})
  b.promise.catch(() => {})
  assert.equal(barrier.pendingCount(), 2)
  assert.equal(barrier.rejectAll(IME_SETTLEMENT_CANCELLED, 'disposed'), 2)
  assert.equal(barrier.pendingCount(), 0)
  await assert.rejects(a.promise, (err) => err.errorCode === IME_SETTLEMENT_CANCELLED)
  await assert.rejects(b.promise, (err) => err.errorCode === IME_SETTLEMENT_CANCELLED)
})

// ── 2. 结算条件 ──

test('退出屏障：preview 读取失败（unknown）不得当已结算 / 不等于 empty', () => {
  const waiter = makeWaiter({ requireImeSignal: false })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState({ previewState: 'unknown' }), 3), false)
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState({ previewState: 'empty' }), 3), true)
})

test('退出屏障：preview 仍非空 / 仍在 composing 不得结算', () => {
  const waiter = makeWaiter({ requireImeSignal: false })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState({ previewState: 'present' }), 3), false)
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState({ imeComposing: true }), 3), false)
})

test('退出屏障：还没有真实 commit/cancel 信号不得结算（已知 composition 必须等真实信号）', () => {
  const waiter = makeWaiter({ requireImeSignal: true })
  // preview 已明确为空、也不在 composing，但还没有真实 commit/cancel 事件。
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState(), 3), false)
  const barrier = new ImeSettlementBarrier(5000)
  const reg = barrier.register(1, 1, 7, 0, 'ime-settlement', true)
  reg.promise.catch(() => {})
  barrier.clearImeSignal(1)
  const cleared = barrier.waiters()[0]
  assert.equal(cleared.requireImeSignal, false)
  assert.equal(isBarrierWaiterSatisfied(cleared, makeAlignment(), makeState(), 3), true)
  barrier.rejectAll(IME_SETTLEMENT_CANCELLED, 'cleanup')
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

test('公共前置：Core 快照不可用绝不能成功（两种模式）', () => {
  const exitWaiter = makeWaiter({ requireImeSignal: false })
  assert.equal(isBarrierWaiterSatisfied(exitWaiter,
    makeAlignment({ coreAvailable: false }), makeState(), 3), false)
  const saveWaiter = makeWaiter({ mode: 'committed-revision' })
  assert.equal(isBarrierWaiterSatisfied(saveWaiter,
    makeAlignment({ coreAvailable: false, aligned: false }), makeState(), 3), false)
  // 没有原生编辑器也不能绕过 Core 快照可用性。
  assert.equal(isBarrierWaiterSatisfied(saveWaiter,
    makeAlignment({ coreAvailable: false, nativeEditorState: 'unbound', aligned: false }), makeState(), 3), false)
})

test('公共前置：Core 会话不匹配不得成功（两种模式）', () => {
  const exitWaiter = makeWaiter({ requireImeSignal: false, coreSessionId: 1 })
  assert.equal(isBarrierWaiterSatisfied(exitWaiter,
    makeAlignment({ coreSessionId: 2 }), makeState(), 3), false)
  const saveWaiter = makeWaiter({ mode: 'committed-revision', coreSessionId: 1 })
  assert.equal(isBarrierWaiterSatisfied(saveWaiter,
    makeAlignment({ coreSessionId: 2 }), makeState(), 3), false)
  // 注册时没有 Core 会话（-1）也不能在未知会话上结算：两个 unknown 不互相证明。
  const unknownWaiter = makeWaiter({ mode: 'committed-revision', coreSessionId: -1 })
  assert.equal(isBarrierWaiterSatisfied(unknownWaiter,
    makeAlignment({ coreSessionId: -1, nativeEditorState: 'unbound', aligned: false }), makeState(), 3), false)
  assert.equal(isBarrierWaiterSatisfied(unknownWaiter,
    makeAlignment({ coreSessionId: -1, aligned: false }), makeState(), 3), false)
})

test('公共前置：屏障 revision / 队列序号未到达不得结算', () => {
  const waiter = makeWaiter({ requireImeSignal: false, barrierRevision: 8 })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment({ appliedRevision: 7 }), makeState(), 3), false)
  const sequenceWaiter = makeWaiter({ requireImeSignal: false, requiredSequence: 4 })
  assert.equal(isBarrierWaiterSatisfied(sequenceWaiter, makeAlignment(), makeState(), 3), false)
})

test('原生状态未知（loading/read-failed）不得成功（两种模式）', () => {
  const saveWaiter = makeWaiter({ mode: 'committed-revision' })
  assert.equal(isBarrierWaiterSatisfied(saveWaiter,
    makeAlignment({ nativeEditorState: 'loading', aligned: false }), makeState(), 3), false)
  assert.equal(isBarrierWaiterSatisfied(saveWaiter,
    makeAlignment({ nativeEditorState: 'read-failed', aligned: false }), makeState(), 3), false)
  const exitWaiter = makeWaiter({ requireImeSignal: false })
  assert.equal(isBarrierWaiterSatisfied(exitWaiter,
    makeAlignment({ nativeEditorState: 'loading', aligned: false }), makeState(), 3), false)
  assert.equal(isBarrierWaiterSatisfied(exitWaiter,
    makeAlignment({ nativeEditorState: 'read-failed', aligned: false }), makeState(), 3), false)
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
  // confirmed 文本与 Core 文本不一致（预编辑已进 Core 但基线没跟上）也不能放行。
  assert.equal(isBarrierWaiterSatisfied(waiter,
    makeAlignment({ coreText: '正文x', confirmedText: '正文' }), composingState, 3), false)
})

test('普通保存屏障：unbound 只要求 coreRevision === appliedRevision', () => {
  const waiter = makeWaiter({ mode: 'committed-revision' })
  assert.equal(isBarrierWaiterSatisfied(waiter,
    makeAlignment({ nativeEditorState: 'unbound', aligned: false }), makeState(), 3), true)
  assert.equal(isBarrierWaiterSatisfied(waiter,
    makeAlignment({ nativeEditorState: 'unbound', aligned: false, coreRevision: 8 }), makeState(), 3), false)
})

test('普通保存屏障：ready 且不在 composing 时要求三方对齐', () => {
  const waiter = makeWaiter({ mode: 'committed-revision' })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState(), 3), true)
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment({ aligned: false }), makeState(), 3), false)
})

test('普通保存屏障：preview unknown/present 不阻塞已提交 revision 的放行', () => {
  const waiter = makeWaiter({ mode: 'committed-revision' })
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState({ previewState: 'unknown' }), 3), true)
  assert.equal(isBarrierWaiterSatisfied(waiter, makeAlignment(), makeState({ previewState: 'present' }), 3), true)
})

test('退出屏障：unbound 仍需队列序号已到达（不无条件 true）', () => {
  const waiter = makeWaiter({ requireImeSignal: false, requiredSequence: 3 })
  assert.equal(isBarrierWaiterSatisfied(waiter,
    makeAlignment({ nativeEditorState: 'unbound', aligned: false }), makeState(), 2), false)
  assert.equal(isBarrierWaiterSatisfied(waiter,
    makeAlignment({ nativeEditorState: 'unbound', aligned: false }), makeState(), 3), true)
})

// ── 3. 真实信号绑定 ──

test('真实 commit 回调推进 requiredSequence 并清除信号等待', () => {
  const barrier = new ImeSettlementBarrier(5000)
  const reg = barrier.register(1, 1, 7, 2, 'ime-settlement', true)
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
  const reg = barrier.register(1, 1, 7, 2, 'ime-settlement', true)
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
  const a = barrier.register(1, 1, 7, 0, 'ime-settlement', true)
  const b = barrier.register(2, 2, 0, 0, 'ime-settlement', true)
  a.promise.catch(() => {})
  b.promise.catch(() => {})
  barrier.bindImeCommitSequence(1, 4)
  const waiters = barrier.waiters()
  const waiterA = waiters.find((w) => w.id === a.id)
  const waiterB = waiters.find((w) => w.id === b.id)
  assert.equal(waiterA.requiredSequence, 4)
  assert.equal(waiterB.requiredSequence, 0)
  assert.equal(waiterB.requireImeSignal, true)
  // Core 会话身份也随注册固定：跨会话事实不能结算别的 waiter。
  assert.equal(isBarrierWaiterSatisfied(waiterA, makeAlignment({ coreSessionId: 2 }), makeState(), 4), false)
  barrier.rejectAll(IME_SETTLEMENT_CANCELLED, 'cleanup')
})

console.log('---')
console.log(`✅ ime_settlement_barrier: ${passed} tests passed`)
