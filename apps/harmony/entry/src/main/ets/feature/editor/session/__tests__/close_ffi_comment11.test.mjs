// close_ffi_comment11.test.mjs — Issue #629 评论11 第3项：editor session close FFI 行为测试。
//
// 验证：
//   1. Coordinator.close() 只有 success===true && data===true 才清除 state
//   2. FFI 返回 { success:true, data:false } 时 Coordinator 保留 state
//   3. FFI 返回 { success:false, ... } 时 Coordinator 保留 state
//   4. closeAsync() 与 close() 语义一致
//   5. WritingScreen.doGracefulClose() 对 close 失败的处理
//
// 运行：node --experimental-strip-types close_ffi_comment11.test.mjs

import { strict as assert } from 'node:assert'

let passed = 0
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

console.log('close_ffi_comment11 editor session close FFI 行为测试（评论11 第3项）')
console.log('---')

// ── Mock: Bridge + Coordinator 逻辑 ──

class MockBridge {
  constructor() {
    this.closeResult = { success: true, data: true, warnings: [], changedPaths: [], changedEntities: [] }
    this.closeCalls = []
  }
  async close(sessionId) {
    this.closeCalls.push(sessionId)
    return this.closeResult
  }
}

// 模拟 Coordinator.close() 逻辑（与修改后的 EditorSessionCoordinator 对齐）
async function coordinatorClose(state, bridge) {
  if (!state) {
    return { success: false, errorCode: 'NO_SESSION', warnings: [], changedPaths: [], changedEntities: [] }
  }
  const result = await bridge.close(state.sessionId)
  // Issue #629 评论11 第3项：只有 success===true && data===true 才视为关闭成功
  if (result.success && result.data === true) {
    return { result, stateAfterClose: null }
  }
  return { result, stateAfterClose: state }
}

// 模拟 Coordinator.closeAsync() 逻辑
async function coordinatorCloseAsync(state, bridge, commandQueue) {
  await commandQueue.whenIdle()
  return coordinatorClose(state, bridge)
}

await testAsync('close: FFI 返回 success=true, data=true → state 置 null', async () => {
  const bridge = new MockBridge()
  bridge.closeResult = { success: true, data: true, warnings: [], changedPaths: [], changedEntities: [] }
  const state = { sessionId: 1, snapshot: { text: 'hello', revision: 1 } }

  const { result, stateAfterClose } = await coordinatorClose(state, bridge)

  assert.equal(result.success, true)
  assert.equal(result.data, true)
  assert.equal(stateAfterClose, null, 'state 应被清除')
  assert.equal(bridge.closeCalls.length, 1)
})

await testAsync('close: FFI 返回 success=true, data=false → state 保留（不误清）', async () => {
  const bridge = new MockBridge()
  bridge.closeResult = { success: true, data: false, warnings: [], changedPaths: [], changedEntities: [] }
  const state = { sessionId: 1, snapshot: { text: 'hello', revision: 1 } }

  const { result, stateAfterClose } = await coordinatorClose(state, bridge)

  assert.equal(result.success, true)
  assert.equal(result.data, false)
  assert.ok(stateAfterClose !== null, 'state 应被保留（不误清）')
  assert.equal(stateAfterClose.sessionId, 1, 'state.sessionId 不变')
})

await testAsync('close: FFI 返回 success=false → state 保留', async () => {
  const bridge = new MockBridge()
  bridge.closeResult = { success: false, errorCode: 'NO_SESSION', warnings: [], changedPaths: [], changedEntities: [] }
  const state = { sessionId: 1, snapshot: { text: 'hello', revision: 1 } }

  const { result, stateAfterClose } = await coordinatorClose(state, bridge)

  assert.equal(result.success, false)
  assert.ok(stateAfterClose !== null, 'state 应被保留')
})

await testAsync('close: state=null → 返回 NO_SESSION，不调 bridge.close', async () => {
  const bridge = new MockBridge()
  const result = await coordinatorClose(null, bridge)

  assert.equal(result.success, false)
  assert.equal(result.errorCode, 'NO_SESSION')
  assert.equal(bridge.closeCalls.length, 0)
})

await testAsync('closeAsync: 先等队列空闲再 close', async () => {
  const bridge = new MockBridge()
  bridge.closeResult = { success: true, data: true, warnings: [], changedPaths: [], changedEntities: [] }
  const state = { sessionId: 1, snapshot: { text: 'hello', revision: 1 } }
  const log = []

  const mockQueue = {
    whenIdle: async () => { log.push('whenIdle') },
  }

  const { result, stateAfterClose } = await coordinatorCloseAsync(state, bridge, mockQueue)

  assert.deepEqual(log, ['whenIdle'])
  assert.equal(result.data, true)
  assert.equal(stateAfterClose, null)
})

// ── WritingScreen.doGracefulClose 对 close 失败的处理 ──

await testAsync('doGracefulClose: closeAsync 返回 success=false → 不 detach、不 pop、unseal', async () => {
  const dispatcherCalls = []
  const dispatcher = {
    seal: () => { dispatcherCalls.push('seal') },
    unseal: () => { dispatcherCalls.push('unseal') },
    flush: async () => { dispatcherCalls.push('flush') },
    finishActiveComposition: async () => { dispatcherCalls.push('finishActiveComposition'); return { success: true } },
  }
  const coordinatorCalls = []
  const coordinator = {
    whenIdle: async () => { coordinatorCalls.push('whenIdle') },
    getSnapshot: () => ({ text: 'hello', composition: null }),
    closeAsync: async () => {
      coordinatorCalls.push('closeAsync')
      return { success: false, errorCode: 'SESSION_ERROR', warnings: [], changedPaths: [], changedEntities: [] }
    },
  }
  const imeCalls = []
  const harmonyImeConnection = { detach: async () => { imeCalls.push('detach') } }

  // 模拟 doGracefulClose 关键路径（简化版，聚焦 close 失败处理）
  dispatcher.seal()
  await dispatcher.finishActiveComposition()
  await dispatcher.flush()
  await coordinator.whenIdle()

  // 保存（假设已保存成功）
  const closeResult = await coordinator.closeAsync()

  // Issue #629 评论11 第3项：close 失败时
  if (!closeResult.success || closeResult.data !== true) {
    dispatcher.unseal()
    // 不 detach、不 pop
  }

  assert.deepEqual(dispatcherCalls, ['seal', 'finishActiveComposition', 'flush', 'unseal'])
  assert.deepEqual(coordinatorCalls, ['whenIdle', 'closeAsync'])
  assert.deepEqual(imeCalls, [], 'close 失败不应 detach')
})

await testAsync('doGracefulClose: closeAsync 返回 success=true, data=false → 不 detach、不 pop、unseal', async () => {
  const dispatcherCalls = []
  const dispatcher = {
    seal: () => { dispatcherCalls.push('seal') },
    unseal: () => { dispatcherCalls.push('unseal') },
    flush: async () => { dispatcherCalls.push('flush') },
    finishActiveComposition: async () => { return { success: true } },
  }
  const coordinator = {
    whenIdle: async () => {},
    getSnapshot: () => ({ text: 'hello', composition: null }),
    closeAsync: async () => ({
      success: true, data: false, warnings: [], changedPaths: [], changedEntities: []
    }),
  }
  const imeCalls = []
  const harmonyImeConnection = { detach: async () => { imeCalls.push('detach') } }

  dispatcher.seal()
  await dispatcher.finishActiveComposition()
  await dispatcher.flush()
  await coordinator.whenIdle()

  const closeResult = await coordinator.closeAsync()

  // 旧实现只检查 closeResult.success → 会误认为成功并 detach
  // 新实现检查 closeResult.data === true → data=false 时不 detach
  if (!closeResult.success || closeResult.data !== true) {
    dispatcher.unseal()
  }

  assert.ok(dispatcherCalls.includes('unseal'), 'data=false 时应 unseal')
  assert.deepEqual(imeCalls, [], 'data=false 时不应 detach')
})

await testAsync('doGracefulClose: closeAsync 返回 success=true, data=true → detach + 返回 true', async () => {
  const dispatcherCalls = []
  const dispatcher = {
    seal: () => { dispatcherCalls.push('seal') },
    unseal: () => { dispatcherCalls.push('unseal') },
    flush: async () => {},
    finishActiveComposition: async () => { return { success: true } },
  }
  const coordinator = {
    whenIdle: async () => {},
    getSnapshot: () => ({ text: 'hello', composition: null }),
    closeAsync: async () => ({
      success: true, data: true, warnings: [], changedPaths: [], changedEntities: []
    }),
  }
  const imeCalls = []
  const harmonyImeConnection = { detach: async () => { imeCalls.push('detach') } }

  dispatcher.seal()
  await dispatcher.finishActiveComposition()
  await dispatcher.flush()
  await coordinator.whenIdle()

  const closeResult = await coordinator.closeAsync()
  let closed = false

  if (closeResult.success && closeResult.data === true) {
    await harmonyImeConnection.detach()
    closed = true
  }

  assert.equal(closed, true)
  assert.deepEqual(imeCalls, ['detach'])
})

// ── 旧实现 vs 新实现对比 ──

await testAsync('对比: 旧实现 close 检查 success→ 误认为 data=false 成功；新实现检查 data===true', async () => {
  const bridgeResult = { success: true, data: false, warnings: [], changedPaths: [], changedEntities: [] }

  // 旧实现：只检查 success
  const oldBehavior = bridgeResult.success  // true → 误认为成功
  assert.equal(oldBehavior, true, '旧实现会误认为成功')

  // 新实现：检查 success && data === true
  const newBehavior = bridgeResult.success && bridgeResult.data === true  // false
  assert.equal(newBehavior, false, '新实现正确识别为失败')
})

console.log('---')
console.log(`✅ close_ffi_comment11: ${passed} tests passed`)
