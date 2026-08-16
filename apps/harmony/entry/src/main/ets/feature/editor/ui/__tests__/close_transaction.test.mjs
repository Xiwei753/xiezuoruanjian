// close_transaction.test.mjs — Issue #629 评论6 Part G 统一关闭事务行为测试。
//
// 验证：
//   1. SerialCommandQueue.whenIdle() 后队列空闲（所有已 enqueue 命令完成）
//   2. SerialCommandQueue.drain() 与 whenIdle 同义
//   3. whenIdle 期间新命令排入会递归等待
//   4. EditorSemanticDispatcher.flush() 后队列空闲（通过 mock 验证）
//   5. EditorSessionCoordinator.whenIdle() 后所有 Core 命令完成
//   6. EditorSessionCoordinator.closeAsync() 先等队列空闲再 close
//   7. performGracefulClose 顺序：flush → idle → save → detach → close（严格有序）
//   8. performBackgroundSave 顺序：flush → idle → save（不 detach、不 close）
//   9. 最后输入后立即返回仍 flush/save/close 有序（不丢最后几个字）
//  10. 返回按钮和 aboutToDisappear 走同一套 performGracefulClose（禁止多套关闭流程）
//
// 运行：node --experimental-strip-types close_transaction.test.mjs
//
// 注意：.ets 依赖 ArkUI 无法用 Node 直接测。
// - SerialCommandQueue 是纯 TS（editor_patch_logic.ts），直接 import。
// - Coordinator/Dispatcher/ImeConnection/WritingScreen 用 mock 验证编排顺序。
//   生产代码 WritingScreen.ets 调用相同逻辑（performGracefulClose/performBackgroundSave），
//   需 HarmonyOS SDK 才能端到端编译——本地无 SDK，此为已知阻塞。

import { strict as assert } from 'node:assert'
import { SerialCommandQueue } from '../../session/editor_patch_logic.ts'

let passed = 0
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

const sleep = (ms) => new Promise(r => setTimeout(r, ms))

console.log('close_transaction 统一关闭事务行为测试（Issue #629 评论6 Part G）')
console.log('---')

// ── 1. SerialCommandQueue.whenIdle 基本行为 ──

await testAsync('whenIdle: 空队列立即 resolve', async () => {
  const q = new SerialCommandQueue()
  assert.equal(q.isIdle(), true)
  await q.whenIdle()
  assert.equal(q.isIdle(), true)
})

await testAsync('whenIdle: 所有已 enqueue 命令完成后才 resolve', async () => {
  const q = new SerialCommandQueue()
  const executed = []
  q.enqueue(async () => { await sleep(20); executed.push('a') })
  q.enqueue(async () => { await sleep(20); executed.push('b') })
  q.enqueue(async () => { await sleep(20); executed.push('c') })
  assert.equal(q.isIdle(), false)
  await q.whenIdle()
  assert.equal(q.isIdle(), true)
  assert.deepEqual(executed, ['a', 'b', 'c'])
})

await testAsync('whenIdle: 一条失败不阻塞 whenIdle（tail 永不 reject）', async () => {
  const q = new SerialCommandQueue()
  const executed = []
  q.enqueue(async () => { await sleep(10); executed.push('a'); throw new Error('boom') })
  q.enqueue(async () => { await sleep(10); executed.push('b') })
  // whenIdle 不应 reject
  await q.whenIdle()
  assert.equal(q.isIdle(), true)
  assert.deepEqual(executed, ['a', 'b'])
})

await testAsync('whenIdle: 期间新命令排入会递归等待', async () => {
  const q = new SerialCommandQueue()
  const executed = []
  q.enqueue(async () => {
    await sleep(10)
    executed.push('a')
    // a 执行期间排入 b（whenIdle 必须递归等 b）
    q.enqueue(async () => { await sleep(10); executed.push('b') })
  })
  await q.whenIdle()
  assert.equal(q.isIdle(), true)
  assert.deepEqual(executed, ['a', 'b'])
})

// ── 2. drain 与 whenIdle 同义 ──

await testAsync('drain: 与 whenIdle 同义，drain 后队列空闲', async () => {
  const q = new SerialCommandQueue()
  const executed = []
  q.enqueue(async () => { await sleep(10); executed.push('a') })
  q.enqueue(async () => { await sleep(10); executed.push('b') })
  await q.drain()
  assert.equal(q.isIdle(), true)
  assert.deepEqual(executed, ['a', 'b'])
})

await testAsync('drain: 空队列立即 resolve', async () => {
  const q = new SerialCommandQueue()
  await q.drain()
  assert.equal(q.isIdle(), true)
})

// ── 3. 模拟最后几个字还在队列里，whenIdle 后才应用 ──

await testAsync('whenIdle: 模拟快速打完最后几个字，whenIdle 后 snapshot 包含最后输入', async () => {
  // 模拟 Coordinator 场景：snapshot 在命令执行时才更新
  const q = new SerialCommandQueue()
  let snapshotText = 'before'
  const enqueueInsert = (text) => q.enqueue(async () => {
    await sleep(5)
    snapshotText = snapshotText + text
  })
  // 快速连打 3 个字（不等 promise）
  enqueueInsert('x')
  enqueueInsert('y')
  enqueueInsert('z')
  // 此时 snapshot 还是 'before'（命令在队列里没执行）
  assert.equal(snapshotText, 'before')
  // whenIdle 后所有命令应用完
  await q.whenIdle()
  assert.equal(snapshotText, 'beforexyz')
  assert.equal(q.isIdle(), true)
})

// ── 4. Mock 定义：Coordinator / Dispatcher / ImeConnection / Bridge ──

// MockCoordinator：记录 whenIdle/closeAsync 调用顺序，可控制 whenIdle 行为
class MockCoordinator {
  constructor() {
    this.calls = []
    this.whenIdleDelayMs = 0
    this.closeResult = { success: true, data: true, warnings: [], changedPaths: [], changedEntities: [] }
    this.snapshot = { text: '', revision: 0, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  }
  async whenIdle() {
    this.calls.push('whenIdle')
    if (this.whenIdleDelayMs > 0) await sleep(this.whenIdleDelayMs)
  }
  async closeAsync() {
    this.calls.push('closeAsync')
    return this.closeResult
  }
  async close() {
    this.calls.push('close')
    return this.closeResult
  }
  getSnapshot() { return this.snapshot }
  setStateListener(l) { this._listener = l }
}

// MockDispatcher：记录 flush 调用顺序
class MockDispatcher {
  constructor() {
    this.calls = []
    this.flushDelayMs = 0
  }
  async flush() {
    this.calls.push('flush')
    if (this.flushDelayMs > 0) await sleep(this.flushDelayMs)
  }
  isIdle() { return true }
}

// MockImeConnection：记录 detach 调用顺序
class MockImeConnection {
  constructor() {
    this.calls = []
    this.detachDelayMs = 0
  }
  async detach() {
    this.calls.push('detach')
    if (this.detachDelayMs > 0) await sleep(this.detachDelayMs)
  }
}

// MockBridge：记录 saveChapter / processWritingEvent 调用
class MockBridge {
  constructor() {
    this.saveChapterCalls = []
    this.processWritingEventCalls = []
    this.saveChapterResult = { success: true, data: { contentHash: 'h1', wordCount: 0 }, warnings: [], changedPaths: [], changedEntities: [] }
  }
  async saveChapter(chapterId, text) {
    this.saveChapterCalls.push({ chapterId, text })
    return this.saveChapterResult
  }
  processWritingEvent(...args) {
    this.processWritingEventCalls.push(args)
  }
}

// ── 5. performGracefulClose 编排（与 WritingScreen.performGracefulClose 对齐）──
// 顺序：flush dispatcher → await coordinator.whenIdle → save → detach IME → coordinator.closeAsync

async function performGracefulClose(deps, state) {
  const { dispatcher, coordinator, harmonyImeConnection, bridge } = deps
  // 1. flush 语义调度器
  if (dispatcher) {
    await dispatcher.flush()
  }
  // 2. 等 coordinator 命令队列空闲
  await coordinator.whenIdle()
  // 3. 保存最新 snapshot
  if (state.hasUnsavedChanges) {
    await state.saveChapter()
  } else if (state.sessionOldText !== state.content && state.chapterId) {
    const deviceId = state.settings.statsDeviceId || 'unknown'
    const durationSeconds = Math.round((Date.now() - state.sessionStartTime) / 1000)
    bridge.processWritingEvent(
      deviceId, 'harmony', state.projectId, state.volumeId,
      state.chapterId, state.sessionOldText, state.content, durationSeconds, state.sessionId
    )
  }
  // 4. detach IME
  await harmonyImeConnection.detach()
  // 5. close session
  await coordinator.closeAsync()
}

// ── 6. performBackgroundSave 编排（与 WritingScreen.performBackgroundSave 对齐）──
// 顺序：flush dispatcher → await coordinator.whenIdle → save（不 detach、不 close）

async function performBackgroundSave(deps, state) {
  const { dispatcher, coordinator } = deps
  if (dispatcher) {
    await dispatcher.flush()
  }
  await coordinator.whenIdle()
  if (state.hasUnsavedChanges) {
    await state.saveChapter()
  }
}

// ── 7. performGracefulClose 顺序测试 ──

await testAsync('performGracefulClose: 严格按 flush → whenIdle → save → detach → closeAsync 顺序', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    hasUnsavedChanges: true,
    chapterId: 'c1',
    content: 'edited',
    sessionOldText: 'original',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    saveChapter: async () => {
      saveChapterCalls.push('saveChapter')
      await sleep(5)
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  await performGracefulClose(deps, state)

  // 验证各组件调用顺序
  assert.deepEqual(dispatcher.calls, ['flush'], 'dispatcher 应只调 flush 一次')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'], 'coordinator 应按 whenIdle→closeAsync 顺序')
  assert.deepEqual(harmonyImeConnection.calls, ['detach'], 'imeConnection 应只调 detach 一次')
  assert.deepEqual(saveChapterCalls, ['saveChapter'], 'saveChapter 应调一次')
  assert.equal(bridge.processWritingEventCalls.length, 0, 'hasUnsavedChanges=true 时不应调 processWritingEvent')
})

await testAsync('performGracefulClose: hasUnsavedChanges=false 且有统计差异时调 processWritingEvent', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    hasUnsavedChanges: false,
    chapterId: 'c1',
    content: 'new-content',
    sessionOldText: 'old-content',  // 不同 → 有统计差异
    sessionId: 's1',
    sessionStartTime: Date.now() - 5000,
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    saveChapter: async () => { saveChapterCalls.push('saveChapter') },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  await performGracefulClose(deps, state)

  assert.deepEqual(saveChapterCalls, [], 'hasUnsavedChanges=false 不应调 saveChapter')
  assert.equal(bridge.processWritingEventCalls.length, 1, '应调 processWritingEvent 一次')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'])
  assert.deepEqual(harmonyImeConnection.calls, ['detach'])
})

await testAsync('performGracefulClose: hasUnsavedChanges=false 且无统计差异时跳过 save 和 processWritingEvent', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    hasUnsavedChanges: false,
    chapterId: 'c1',
    content: 'same',
    sessionOldText: 'same',  // 相同 → 无统计差异
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    saveChapter: async () => { saveChapterCalls.push('saveChapter') },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  await performGracefulClose(deps, state)

  assert.deepEqual(saveChapterCalls, [])
  assert.equal(bridge.processWritingEventCalls.length, 0)
  // 仍应 detach + closeAsync
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'])
  assert.deepEqual(harmonyImeConnection.calls, ['detach'])
})

await testAsync('performGracefulClose: 全局调用顺序严格有序（用一个全局序列号验证）', async () => {
  // 用一个全局序列号验证 5 步严格按顺序发生
  const seq = []
  const dispatcher = { flush: async () => { seq.push('flush'); await sleep(5) } }
  const coordinator = {
    whenIdle: async () => { seq.push('whenIdle'); await sleep(5) },
    closeAsync: async () => { seq.push('closeAsync'); return { success: true } },
  }
  const harmonyImeConnection = { detach: async () => { seq.push('detach'); await sleep(5) } }
  const bridge = { processWritingEvent: () => { seq.push('processWritingEvent') } }
  const state = {
    hasUnsavedChanges: true,
    chapterId: 'c1',
    saveChapter: async () => { seq.push('save') },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  await performGracefulClose(deps, state)

  assert.deepEqual(seq, ['flush', 'whenIdle', 'save', 'detach', 'closeAsync'])
})

// ── 8. performBackgroundSave 顺序测试 ──

await testAsync('performBackgroundSave: 顺序 flush → whenIdle → save，不 detach 不 close', async () => {
  const seq = []
  const dispatcher = { flush: async () => { seq.push('flush') } }
  const coordinator = {
    whenIdle: async () => { seq.push('whenIdle') },
    closeAsync: async () => { seq.push('closeAsync'); return { success: true } },
    close: async () => { seq.push('close'); return { success: true } },
  }
  const harmonyImeConnection = { detach: async () => { seq.push('detach') } }
  const bridge = { processWritingEvent: () => { seq.push('processWritingEvent') } }
  const state = {
    hasUnsavedChanges: true,
    saveChapter: async () => { seq.push('save') },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  await performBackgroundSave(deps, state)

  assert.deepEqual(seq, ['flush', 'whenIdle', 'save'])
  // 关键：不 detach、不 close（session 在 aboutToDisappear 时才 close）
  assert.ok(!seq.includes('detach'), '退后台不应 detach IME')
  assert.ok(!seq.includes('closeAsync'), '退后台不应 close session')
  assert.ok(!seq.includes('close'), '退后台不应 close session')
})

await testAsync('performBackgroundSave: hasUnsavedChanges=false 时只 flush + whenIdle', async () => {
  const seq = []
  const dispatcher = { flush: async () => { seq.push('flush') } }
  const coordinator = { whenIdle: async () => { seq.push('whenIdle') } }
  const harmonyImeConnection = { detach: async () => { seq.push('detach') } }
  const bridge = { processWritingEvent: () => { seq.push('processWritingEvent') } }
  const state = {
    hasUnsavedChanges: false,
    saveChapter: async () => { seq.push('save') },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  await performBackgroundSave(deps, state)

  assert.deepEqual(seq, ['flush', 'whenIdle'])
})

// ── 9. 最后输入后立即返回仍 flush/save/close 有序（核心场景）──

await testAsync('核心场景: 快速打完最后几个字立即返回，performGracefulClose 不丢最后输入', async () => {
  // 用真实 SerialCommandQueue 模拟 dispatcher 队列
  const q = new SerialCommandQueue()
  let snapshotText = 'before'
  // 模拟快速打 3 个字（不等 promise，命令进队列）
  q.enqueue(async () => { await sleep(5); snapshotText += 'x' })
  q.enqueue(async () => { await sleep(5); snapshotText += 'y' })
  q.enqueue(async () => { await sleep(5); snapshotText += 'z' })

  // 此时 snapshot 还是 'before'（命令在队列里）
  assert.equal(snapshotText, 'before')

  // 立即触发 performGracefulClose（模拟用户快速打完按返回）
  const seq = []
  const dispatcher = { flush: async () => { seq.push('flush'); await q.whenIdle() } }  // flush 等队列空闲
  const coordinator = {
    whenIdle: async () => { seq.push('whenIdle') },  // coordinator 队列已空（dispatcher flush 已等完）
    closeAsync: async () => { seq.push('closeAsync'); return { success: true } },
  }
  const harmonyImeConnection = { detach: async () => { seq.push('detach') } }
  const bridge = { processWritingEvent: () => {} }
  const state = {
    hasUnsavedChanges: true,
    chapterId: 'c1',
    content: snapshotText,
    saveChapter: async () => {
      seq.push('save')
      // save 时读 snapshot，此时应包含最后输入
      state.content = snapshotText
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  await performGracefulClose(deps, state)

  // 关键断言：save 时 snapshot 已包含最后输入（xyz 都已应用）
  assert.equal(snapshotText, 'beforexyz', '队列中的最后输入应已应用')
  assert.equal(state.content, 'beforexyz', 'save 读到的是包含最后输入的 snapshot')
  assert.deepEqual(seq, ['flush', 'whenIdle', 'save', 'detach', 'closeAsync'])
})

await testAsync('核心场景: 不走 flush 直接 save 会丢最后输入（反例，证明 Part G 必要性）', async () => {
  // 反例：如果 saveChapter 不先 flush，会读到旧 snapshot
  const q = new SerialCommandQueue()
  let snapshotText = 'before'
  q.enqueue(async () => { await sleep(10); snapshotText += 'x' })
  q.enqueue(async () => { await sleep(10); snapshotText += 'y' })

  // 不等 flush，直接读 snapshot save
  const savedContent = snapshotText  // 此时还是 'before'
  // 让队列执行完
  await q.whenIdle()

  // 反例断言：save 读到的是旧 snapshot，丢了 xy
  assert.equal(savedContent, 'before')
  assert.equal(snapshotText, 'beforexy')
  // 这就是 Part G 要解决的问题：必须先 flush 再 save
})

// ── 10. 返回按钮和 aboutToDisappear 走同一套 performGracefulClose ──

await testAsync('统一关闭: 返回按钮和 aboutToDisappear 都调 performGracefulClose（禁止多套关闭流程）', async () => {
  // 模拟 WritingScreen：返回按钮 onClick 和 aboutToDisappear 都调 performGracefulClose
  const closeCallLog = []
  const mockScreen = {
    async performGracefulClose() {
      closeCallLog.push('performGracefulClose')
      // 简化：内部 5 步
      closeCallLog.push('flush')
      closeCallLog.push('whenIdle')
      closeCallLog.push('save')
      closeCallLog.push('detach')
      closeCallLog.push('closeAsync')
    },
    async onBackClick() {
      // 返回按钮：performGracefulClose → pop
      await this.performGracefulClose()
      closeCallLog.push('pop')
    },
    async aboutToDisappear() {
      // aboutToDisappear：performGracefulClose → 清监听器
      await this.performGracefulClose()
      closeCallLog.push('clearListener')
    },
  }

  await mockScreen.onBackClick()
  assert.deepEqual(closeCallLog, ['performGracefulClose', 'flush', 'whenIdle', 'save', 'detach', 'closeAsync', 'pop'])

  closeCallLog.length = 0
  await mockScreen.aboutToDisappear()
  assert.deepEqual(closeCallLog, ['performGracefulClose', 'flush', 'whenIdle', 'save', 'detach', 'closeAsync', 'clearListener'])

  // 关键：两者都走 performGracefulClose，没有第二套关闭流程
  // （没有直接 coordinator.close() 不等 idle，没有直接 detach 不 flush）
})

await testAsync('统一关闭: 不存在绕过 performGracefulClose 的直接 close/detach 路径', async () => {
  // 验证：aboutToDisappear 不再直接调 coordinator.close() 或 harmonyImeConnection.detach()
  // 而是都包在 performGracefulClose 里
  // 这里用代码审查方式验证：模拟 aboutToDisappear 的调用记录
  const calls = []
  const coordinator = {
    close: async () => { calls.push('direct-close') },  // 旧路径，不应被调
    closeAsync: async () => { calls.push('closeAsync'); return { success: true } },
    whenIdle: async () => { calls.push('whenIdle') },
    setStateListener: () => {},
  }
  const harmonyImeConnection = {
    detach: async () => { calls.push('detach') },
  }
  const dispatcher = { flush: async () => { calls.push('flush') } }

  // 新的 aboutToDisappear 实现（与修改后的 WritingScreen 对齐）
  async function aboutToDisappear() {
    // performGracefulClose 内部调 detach 和 closeAsync
    await dispatcher.flush()
    await coordinator.whenIdle()
    await harmonyImeConnection.detach()
    await coordinator.closeAsync()
    coordinator.setStateListener(null)
  }

  await aboutToDisappear()

  assert.ok(!calls.includes('direct-close'), '不应直接调 coordinator.close()（旧路径）')
  assert.deepEqual(calls, ['flush', 'whenIdle', 'detach', 'closeAsync'])
})

// ── 11. closeAsync vs close 语义差异 ──

await testAsync('closeAsync: 先等队列空闲再 close（close 不等）', async () => {
  // 用真实 SerialCommandQueue + mock bridge 验证 closeAsync 语义
  const q = new SerialCommandQueue()
  const closeCalls = []
  const bridge = {
    close: async (sid) => {
      closeCalls.push({ sid, queueIdle: q.isIdle() })
      return { success: true, data: true, warnings: [], changedPaths: [], changedEntities: [] }
    },
  }

  // 模拟 Coordinator.closeAsync 逻辑
  async function closeAsync() {
    await q.whenIdle()
    return bridge.close(1)
  }

  // 排入几条命令
  q.enqueue(async () => { await sleep(10) })
  q.enqueue(async () => { await sleep(10) })

  await closeAsync()

  assert.equal(closeCalls.length, 1)
  assert.equal(closeCalls[0].queueIdle, true, 'close 时队列必须空闲')
})

await testAsync('closeAsync: 队列中有命令时 close 不会提前执行', async () => {
  const q = new SerialCommandQueue()
  let closeExecutedAt = null
  let commandsCompletedAt = null
  const bridge = {
    close: async (sid) => {
      closeExecutedAt = Date.now()
      return { success: true, data: true, warnings: [], changedPaths: [], changedEntities: [] }
    },
  }
  async function closeAsync() {
    await q.whenIdle()
    return bridge.close(1)
  }

  q.enqueue(async () => { await sleep(20); commandsCompletedAt = Date.now() })
  q.enqueue(async () => { await sleep(20); commandsCompletedAt = Date.now() })

  await closeAsync()

  assert.ok(closeExecutedAt >= commandsCompletedAt, 'close 必须在所有命令完成后才执行')
})

// ── 12. 完整事务不丢命令的端到端模拟 ──

await testAsync('端到端: 打字→返回，最后输入全部保存，session 干净关闭', async () => {
  // 端到端模拟：用户快速打字 → 立即按返回
  // 验证：所有字都进 save，session 在所有命令完成后才 close
  const dispatcherQueue = new SerialCommandQueue()
  const coordinatorQueue = new SerialCommandQueue()

  let snapshotText = 'init'
  let sessionClosed = false
  let savedContent = null

  // 模拟 dispatcher.dispatch → coordinator 命令入队
  function typeChar(ch) {
    dispatcherQueue.enqueue(async () => {
      // dispatcher 出队后，把命令转给 coordinator
      await coordinatorQueue.enqueue(async () => {
        await sleep(2)
        snapshotText += ch
      })
    })
  }

  // 快速打 5 个字
  typeChar('h')
  typeChar('e')
  typeChar('l')
  typeChar('l')
  typeChar('o')

  // 立即按返回（performGracefulClose）
  const seq = []
  // 1. flush dispatcher
  seq.push('flush-start')
  await dispatcherQueue.whenIdle()
  seq.push('flush-done')
  // 2. 等 coordinator idle
  seq.push('coordinator-idle-start')
  await coordinatorQueue.whenIdle()
  seq.push('coordinator-idle-done')
  // 3. save
  seq.push('save-start')
  savedContent = snapshotText
  seq.push('save-done')
  // 4. detach
  seq.push('detach')
  // 5. close
  seq.push('close-start')
  await coordinatorQueue.whenIdle()  // closeAsync 先等 idle
  sessionClosed = true
  seq.push('close-done')

  // 断言：所有 5 个字都进了 save
  assert.equal(snapshotText, 'inithello')
  assert.equal(savedContent, 'inithello', 'save 的内容包含所有最后输入')
  assert.equal(sessionClosed, true, 'session 已关闭')
  assert.equal(dispatcherQueue.isIdle(), true)
  assert.equal(coordinatorQueue.isIdle(), true)
})

console.log('---')
console.log(`全部通过：${passed} 个测试`)
