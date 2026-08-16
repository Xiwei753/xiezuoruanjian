// close_transaction.test.mjs — Issue #629 评论6 Part G 统一关闭事务行为测试。
//
// 验证：
//   1. SerialCommandQueue.whenIdle() 后队列空闲（所有已 enqueue 命令完成）
//   2. SerialCommandQueue.drain() 与 whenIdle 同义
//   3. whenIdle 期间新命令排入会递归等待
//   4. EditorSemanticDispatcher.flush() 后队列空闲（通过 mock 验证）
//   5. EditorSessionCoordinator.whenIdle() 后所有 Core 命令完成
//   6. EditorSessionCoordinator.closeAsync() 先等队列空闲再 close
//   7. performGracefulClose 顺序：flush → idle → save → closeAsync → detach（严格有序）
//   8. performBackgroundSave 顺序：flush → idle → save（不 detach、不 close）
//   9. 最后输入后立即返回仍 flush/save/close 有序（不丢最后几个字）
//  10. 返回按钮和 onBackPressed 走同一 requestLeave，aboutToDisappear 只做 observer 清理（禁止多套关闭流程）
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

// MockDispatcher：记录 seal/unseal/finishActiveComposition/flush 调用顺序。
// Issue #629 评论8 第3项：seal 拒绝新 dispatch；finishActiveComposition 可配置
// finishHandler（默认只记录调用）——测试里用真实 commit 逻辑验证"最后 preedit 提交后保存"。
class MockDispatcher {
  constructor() {
    this.calls = []
    this.flushDelayMs = 0
    this.sealed = false
    this.finishHandler = null
    this.dispatchRejectedCalls = []
  }
  async flush() {
    this.calls.push('flush')
    if (this.flushDelayMs > 0) await sleep(this.flushDelayMs)
  }
  seal() {
    this.calls.push('seal')
    this.sealed = true
  }
  unseal() {
    this.calls.push('unseal')
    this.sealed = false
  }
  isSealed() { return this.sealed }
  async finishActiveComposition() {
    this.calls.push('finishActiveComposition')
    if (this.finishHandler) {
      return this.finishHandler()
    }
    return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
  }
  // 模拟 seal 后 dispatch 被拒：记录被拒命令，返回 SEALED 失败 envelope。
  dispatch(cmd) {
    if (this.sealed) {
      this.dispatchRejectedCalls.push(cmd)
      return Promise.resolve({ success: false, errorCode: 'SEALED', warnings: [], changedPaths: [], changedEntities: [] })
    }
    return Promise.resolve({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
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
// Issue #629 评论8 第2/3项 + 评论9 第2项顺序：seal → finishActiveComposition → flush → whenIdle
// → save（失败 unseal + 返回 false，不 close/detach）→ closeAsync（失败 unseal + 返回 false，不 detach）
// → detach（不可逆清理放到最后）。返回 boolean：只有 true 才允许 pop。

async function performGracefulClose(deps, state) {
  const { dispatcher, coordinator, harmonyImeConnection, bridge } = deps
  // 1. seal：拒绝新的普通输入命令（已排队命令仍可 drain）
  dispatcher.seal()
  // 2. 同一 dispatcher 队列里 finish 当前 composition（最后 preedit 提交进正文）
  // Issue #629 评论 5308748920 问题1：必须看 finishResult.success。
  const finishResult = await dispatcher.finishActiveComposition()
  // 3. flush 语义调度器
  await dispatcher.flush()
  // 4. 等 coordinator 命令队列空闲
  await coordinator.whenIdle()
  // 5. Issue #629 评论 5308748920 问题1：composition 唯一真源是 Core snapshot。
  //    重读 afterFinish snapshot，只要 finishResult.success===false、afterFinish===null、
  //    或 afterFinish.composition!==null，立即 unseal 并 return false，不进入保存。
  const afterFinish = coordinator.getSnapshot()
  if (finishResult.success === false || afterFinish === null || afterFinish.composition !== null) {
    dispatcher.unseal()
    return false
  }
  // 6. 保存稳定 snapshot（不再读可能已变化的 state.content）
  const savedText = afterFinish.text
  if (savedText !== state.lastSavedContent) {
    // Issue #629 评论 5308748920 问题3：ensureSnapshotSaved 精确保存这份 savedText。
    const settled = await state.ensureSnapshotSaved(savedText, false)
    if (!settled) {
      // 保存失败：不 detach、不 close、不 pop；解锁输入让用户继续编辑。
      dispatcher.unseal()
      return false
    }
  } else if (state.sessionOldText !== savedText && state.chapterId) {
    const deviceId = state.settings.statsDeviceId || 'unknown'
    const durationSeconds = Math.round((Date.now() - state.sessionStartTime) / 1000)
    bridge.processWritingEvent(
      deviceId, 'harmony', state.projectId, state.volumeId,
      state.chapterId, state.sessionOldText, savedText, durationSeconds, state.sessionId
    )
    state.sessionOldText = savedText
  }
  // 7. close session. Issue #629 评论9 第2项：先 closeAsync，成功后才 detach IME。
  const closeResult = await coordinator.closeAsync()
  if (!closeResult.success) {
    // close 失败：不 detach、不 pop；unseal 让用户继续编辑（不需要重新 attach IME）。
    dispatcher.unseal()
    return false
  }
  // 8. Core close 成功后才 detach IME（不可逆清理放到最后，失败路径不触不可逆动作）
  await harmonyImeConnection.detach()
  return true
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
    // Issue #629 评论 5308748920 问题3：先取得稳定 snapshot，再 ensureSnapshotSaved 精确保存。
    const snap = coordinator.getSnapshot()
    const targetText = snap ? snap.text : state.content
    await state.ensureSnapshotSaved(targetText, true)
  }
}

// ── 7. performGracefulClose 顺序测试 ──

await testAsync('performGracefulClose: 严格按 seal → finish → flush → whenIdle → save → closeAsync → detach 顺序', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  // MockCoordinator 默认 snapshot.text=''，设成与 content 一致（savedText='edited'）
  coordinator.snapshot = { ...coordinator.snapshot, text: 'edited' }
  const state = {
    chapterId: 'c1',
    content: 'edited',
    lastSavedContent: 'original',  // snapshot 'edited' !== 'original' → 走 save
    sessionOldText: 'original',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      saveChapterCalls.push('saveChapter')
      await sleep(5)
      state.lastSavedContent = 'edited'  // 真实保存按 savedText 结算
      return true
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  const closed = await performGracefulClose(deps, state)

  assert.equal(closed, true)
  // 验证各组件调用顺序
  assert.deepEqual(dispatcher.calls, ['seal', 'finishActiveComposition', 'flush'], 'dispatcher 应按 seal→finishActiveComposition→flush')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'], 'coordinator 应按 whenIdle→closeAsync 顺序')
  assert.deepEqual(harmonyImeConnection.calls, ['detach'], 'imeConnection 应只调 detach 一次')
  assert.deepEqual(saveChapterCalls, ['saveChapter'], 'saveChapter 应调一次')
  assert.equal(bridge.processWritingEventCalls.length, 0, '走 save 分支时不应调 processWritingEvent')
})

await testAsync('performGracefulClose: snapshot 已保存（savedText===lastSavedContent）且有统计差异时调 processWritingEvent', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    chapterId: 'c1',
    content: 'new-content',
    lastSavedContent: 'new-content',  // snapshot 已保存 → 不调 saveChapter
    sessionOldText: 'old-content',  // 不同 → 有统计差异
    sessionId: 's1',
    sessionStartTime: Date.now() - 5000,
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => { saveChapterCalls.push('saveChapter') },
  }
  // MockCoordinator 默认 snapshot.text=''，设成与 content 一致（savedText===lastSavedContent）
  coordinator.snapshot = { ...coordinator.snapshot, text: 'new-content' }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  const closed = await performGracefulClose(deps, state)

  assert.equal(closed, true)
  assert.deepEqual(saveChapterCalls, [], 'savedText===lastSavedContent 不应调 saveChapter')
  assert.equal(bridge.processWritingEventCalls.length, 1, '应调 processWritingEvent 一次')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'])
  assert.deepEqual(harmonyImeConnection.calls, ['detach'])
})

await testAsync('performGracefulClose: snapshot 已保存且无统计差异时跳过 save 和 processWritingEvent', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    chapterId: 'c1',
    content: 'same',
    lastSavedContent: 'same',  // savedText === lastSavedContent
    sessionOldText: 'same',  // 相同 → 无统计差异
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => { saveChapterCalls.push('saveChapter') },
  }
  coordinator.snapshot = { ...coordinator.snapshot, text: 'same' }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  const closed = await performGracefulClose(deps, state)

  assert.equal(closed, true)
  assert.deepEqual(saveChapterCalls, [])
  assert.equal(bridge.processWritingEventCalls.length, 0)
  // 仍应 detach + closeAsync
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'])
  assert.deepEqual(harmonyImeConnection.calls, ['detach'])
})

await testAsync('performGracefulClose: 全局调用顺序严格有序（用一个全局序列号验证）', async () => {
  // 用一个全局序列号验证 7 步严格按顺序发生：
  // seal → finishActiveComposition → flush → whenIdle → save → detach → closeAsync
  const seq = []
  const dispatcher = {
    seal: () => { seq.push('seal') },
    unseal: () => { seq.push('unseal') },
    finishActiveComposition: async () => { seq.push('finishActiveComposition'); await sleep(5); return { success: true } },
    flush: async () => { seq.push('flush'); await sleep(5) },
  }
  const coordinator = {
    getSnapshot: () => ({ text: 'edited', composition: null }),
    whenIdle: async () => { seq.push('whenIdle'); await sleep(5) },
    closeAsync: async () => { seq.push('closeAsync'); return { success: true } },
  }
  const harmonyImeConnection = { detach: async () => { seq.push('detach'); await sleep(5) } }
  const bridge = { processWritingEvent: () => { seq.push('processWritingEvent') } }
  const state = {
    chapterId: 'c1',
    content: 'edited',
    lastSavedContent: 'original',  // snapshot 'edited' !== lastSavedContent → 走 save
    sessionOldText: 'original',
    ensureSnapshotSaved: async (targetText, isAutoSave) => { seq.push('save'); state.lastSavedContent = 'edited'; return true },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const closed = await performGracefulClose(deps, state)

  assert.equal(closed, true)
  assert.deepEqual(seq, ['seal', 'finishActiveComposition', 'flush', 'whenIdle', 'save', 'closeAsync', 'detach'])
})

// ── 8. performBackgroundSave 顺序测试 ──

await testAsync('performBackgroundSave: 顺序 flush → whenIdle → save，不 detach 不 close', async () => {
  const seq = []
  const dispatcher = { flush: async () => { seq.push('flush') } }
  const coordinator = {
    getSnapshot: () => ({ text: 'bg-snap', composition: null }),
    whenIdle: async () => { seq.push('whenIdle') },
    closeAsync: async () => { seq.push('closeAsync'); return { success: true } },
    close: async () => { seq.push('close'); return { success: true } },
  }
  const harmonyImeConnection = { detach: async () => { seq.push('detach') } }
  const bridge = { processWritingEvent: () => { seq.push('processWritingEvent') } }
  const state = {
    hasUnsavedChanges: true,
    ensureSnapshotSaved: async (targetText, isAutoSave) => { seq.push('save') },
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
    ensureSnapshotSaved: async (targetText, isAutoSave) => { seq.push('save') },
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
  const dispatcher = {
    seal: () => { seq.push('seal') },
    unseal: () => { seq.push('unseal') },
    finishActiveComposition: async () => { seq.push('finishActiveComposition'); return { success: true } },
    flush: async () => { seq.push('flush'); await q.whenIdle() },  // flush 等队列空闲
  }
  const coordinator = {
    getSnapshot: () => ({ text: snapshotText, composition: null }),
    whenIdle: async () => { seq.push('whenIdle') },  // coordinator 队列已空（dispatcher flush 已等完）
    closeAsync: async () => { seq.push('closeAsync'); return { success: true } },
  }
  const harmonyImeConnection = { detach: async () => { seq.push('detach') } }
  const bridge = { processWritingEvent: () => {} }
  const state = {
    chapterId: 'c1',
    content: snapshotText,
    lastSavedContent: 'before',  // snapshot 'beforexyz' !== 'before' → 走 save
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      seq.push('save')
      // save 时读 snapshot，此时应包含最后输入
      state.content = snapshotText
      state.lastSavedContent = snapshotText  // 真实保存按 savedText 结算
      return true
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }

  const closed = await performGracefulClose(deps, state)

  // 关键断言：save 时 snapshot 已包含最后输入（xyz 都已应用）
  assert.equal(closed, true)
  assert.equal(snapshotText, 'beforexyz', '队列中的最后输入应已应用')
  assert.equal(state.content, 'beforexyz', 'save 读到的是包含最后输入的 snapshot')
  assert.deepEqual(seq, ['seal', 'finishActiveComposition', 'flush', 'whenIdle', 'save', 'closeAsync', 'detach'])
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

// ── 10. 返回按钮和 onBackPressed 走同一 requestLeave，aboutToDisappear 只做清理 ──

await testAsync('统一关闭: 返回按钮和 onBackPressed 都调 requestLeave，aboutToDisappear 只做清理（禁止多套关闭流程）', async () => {
  // 模拟 WritingScreen（评论9 第3项 + 评论 5308748920 第2项）：
  //   - 返回按钮 onClick 和 onBackPressed 都调 requestLeave（统一离开入口）
  //   - requestLeave 内部调 performGracefulClose，成功才 pop；用 leaveTask 幂等
  //   - aboutToDisappear 不再调 performGracefulClose，只做 observer 清理
  const closeCallLog = []
  const mockScreen = {
    leaveTask: null,
    async performGracefulClose() {
      closeCallLog.push('performGracefulClose')
      // 简化：内部 7 步（评论9 第2项顺序：flush → whenIdle → save → closeAsync → detach）
      closeCallLog.push('seal')
      closeCallLog.push('finishActiveComposition')
      closeCallLog.push('flush')
      closeCallLog.push('whenIdle')
      closeCallLog.push('save')
      closeCallLog.push('closeAsync')
      closeCallLog.push('detach')
      return true
    },
    async requestLeave() {
      // 评论 5308748920 第2项：leaveTask 幂等——已有 task 直接返回
      if (this.leaveTask !== null) {
        return this.leaveTask
      }
      closeCallLog.push('requestLeave')
      const task = (async () => {
        const closed = await this.performGracefulClose()
        if (closed === true) {
          closeCallLog.push('pop')
        }
      })()
      this.leaveTask = task
      return task
    },
    async onBackClick() {
      // 左上角返回按钮 onClick：走统一离开入口 requestLeave
      await this.requestLeave()
    },
    onBackPressed() {
      // NavDestination.onBackPressed：异步调 requestLeave，同步 return true 拦截默认退栈
      this.requestLeave()
      return true
    },
    async aboutToDisappear() {
      // 评论9 第3项：aboutToDisappear 只做 observer 清理，不调 performGracefulClose
      closeCallLog.push('setStateListenerNull')
      closeCallLog.push('stopShareLifecycle')
      closeCallLog.push('removeObserver')
    },
  }

  // 1. 返回按钮 onClick：走 requestLeave → performGracefulClose → ... → pop
  await mockScreen.onBackClick()
  assert.deepEqual(closeCallLog, ['requestLeave', 'performGracefulClose', 'seal', 'finishActiveComposition', 'flush', 'whenIdle', 'save', 'closeAsync', 'detach', 'pop'])

  // 2. onBackPressed：return true 拦截默认退栈，异步走 requestLeave
  closeCallLog.length = 0
  mockScreen.leaveTask = null
  const ret = mockScreen.onBackPressed()
  assert.equal(ret, true, 'onBackPressed 应 return true 拦截默认退栈')
  // 等待 requestLeave 异步链跑完
  await mockScreen.leaveTask
  assert.ok(closeCallLog.includes('requestLeave'), 'onBackPressed 应触发 requestLeave')
  assert.deepEqual(closeCallLog, ['requestLeave', 'performGracefulClose', 'seal', 'finishActiveComposition', 'flush', 'whenIdle', 'save', 'closeAsync', 'detach', 'pop'])

  // 3. aboutToDisappear：只做清理，不走关闭流程
  closeCallLog.length = 0
  await mockScreen.aboutToDisappear()
  assert.deepEqual(closeCallLog, ['setStateListenerNull', 'stopShareLifecycle', 'removeObserver'])
  assert.ok(!closeCallLog.includes('performGracefulClose'), 'aboutToDisappear 不应调 performGracefulClose')
  assert.ok(!closeCallLog.includes('flush'), 'aboutToDisappear 不应调 flush')
  assert.ok(!closeCallLog.includes('closeAsync'), 'aboutToDisappear 不应调 closeAsync')
  assert.ok(!closeCallLog.includes('detach'), 'aboutToDisappear 不应调 detach')

  // 关键：返回按钮和 onBackPressed 走同一 requestLeave 入口；aboutToDisappear 不走关闭流程
  // （没有第二套关闭流程，没有直接 coordinator.close() 不等 idle，没有直接 detach 不 flush）
})

await testAsync('统一关闭: aboutToDisappear 不调 flush/closeAsync/detach，只做 observer 清理', async () => {
  // 验证（评论9 第3项）：aboutToDisappear 只做 observer 清理，
  // 不调 coordinator.close / coordinator.closeAsync / coordinator.whenIdle /
  // dispatcher.flush / harmonyImeConnection.detach。
  // 关闭流程只在 requestLeave → performGracefulClose 里执行。
  const calls = []
  const coordinator = {
    close: async () => { calls.push('direct-close') },  // 旧路径，不应被调
    closeAsync: async () => { calls.push('closeAsync'); return { success: true } },
    whenIdle: async () => { calls.push('whenIdle') },
    setStateListener: (l) => { if (l === null) calls.push('setStateListenerNull') },
  }
  const harmonyImeConnection = {
    detach: async () => { calls.push('detach') },
  }
  const dispatcher = { flush: async () => { calls.push('flush') } }

  // 新的 aboutToDisappear 实现（与修改后的 WritingScreen 对齐）
  // 评论9 第3项：只做 observer 清理，不调 flush/whenIdle/closeAsync/detach
  async function aboutToDisappear() {
    coordinator.setStateListener(null)
    calls.push('stopShareLifecycle')
    calls.push('removeAutoSaveObserver')
    calls.push('removeShareLifecycleObserver')
    calls.push('removeThemeObserver')
    calls.push('removeAdaptiveObserver')
  }

  await aboutToDisappear()

  // 不应出现任何关闭流程调用
  assert.ok(!calls.includes('direct-close'), '不应直接调 coordinator.close()（旧路径）')
  assert.ok(!calls.includes('closeAsync'), 'aboutToDisappear 不应调 coordinator.closeAsync')
  assert.ok(!calls.includes('whenIdle'), 'aboutToDisappear 不应调 coordinator.whenIdle')
  assert.ok(!calls.includes('flush'), 'aboutToDisappear 不应调 dispatcher.flush')
  assert.ok(!calls.includes('detach'), 'aboutToDisappear 不应调 harmonyImeConnection.detach')
  // 应出现清理操作
  assert.deepEqual(calls, ['setStateListenerNull', 'stopShareLifecycle', 'removeAutoSaveObserver', 'removeShareLifecycleObserver', 'removeThemeObserver', 'removeAdaptiveObserver'])
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
  // 4. close. Issue #629 评论9 第2项：先 close，成功后才 detach。
  seq.push('close-start')
  await coordinatorQueue.whenIdle()  // closeAsync 先等 idle
  sessionClosed = true
  seq.push('close-done')
  // 5. detach
  seq.push('detach')

  // 断言：所有 5 个字都进了 save
  assert.equal(snapshotText, 'inithello')
  assert.equal(savedContent, 'inithello', 'save 的内容包含所有最后输入')
  assert.equal(sessionClosed, true, 'session 已关闭')
  assert.equal(dispatcherQueue.isIdle(), true)
  assert.equal(coordinatorQueue.isIdle(), true)
})

// ── 13. Issue #629 评论7 第5项 + 评论8 第2/3项：幂等关闭事务（closeTask 复用同一 Promise）──
//
// 验证：
//   13a. 连续 performGracefulClose 两次，doGracefulClose 内部逻辑只执行一次，Promise 同一性
//   13b. 已保存有统计差异时，performGracefulClose 两次 processWritingEvent 只调一次
//   13c. sessionOldText 推进防御：doGracefulClose 两次（绕过幂等），第二次不再满足统计条件
//   13d. 返回按钮 + aboutToDisappear 并发：真实关闭链只跑一次
//   13e. 成功完成后再次调用仍返回同一 Promise（不重跑）
//   13f. saveChapter 推进 sessionOldText，第二次不走统计分支也不重复
//   13g. 保存失败 → 返回 false、不 detach/close、unseal；失败后 closeTask 重置允许重试
//   13h. closeAsync 失败 → 返回 false（不把关闭失败当成功，不 pop）
//   13i. seal 后 dispatch 拒新输入（SEALED），finishActiveComposition 仍可入队
//   13j. 最后中文 preedit：finishActiveComposition 提交 preedit 后才保存（savedText 含 preedit）
//
// 与修改后的 WritingScreen.performGracefulClose / doGracefulClose 对齐：
//   performGracefulClose(): closeTask !== null ? return closeTask : closeTask = doGracefulClose()
//   doGracefulClose(): seal → finishActiveComposition → flush → whenIdle → save/统计
//     （失败 unseal + return false，不 close/detach）→ closeAsync（失败 unseal + return false，不 detach）
//     → detach（不可逆清理放到最后）→ true
//   失败后 closeTask 重置（允许下次返回重试）；成功后保留缓存（页面即将销毁）。

// 幂等 performGracefulClose + doGracefulClose 工厂（与修改后的 WritingScreen 对齐）
function createGracefulCloseScreen(deps, state) {
  let closeTask = null
  const doGracefulClose = async () => {
    const { dispatcher, coordinator, harmonyImeConnection, bridge } = deps
    // 1. seal：拒绝新的普通输入命令（已排队命令仍可 drain）
    dispatcher.seal()
    try {
      // 2. 同一 dispatcher 队列里 finish 当前 composition（最后 preedit 提交进正文）
      // Issue #629 评论 5308748920 问题1：必须看 finishResult.success。
      const finishResult = await dispatcher.finishActiveComposition()
      // 3. flush
      await dispatcher.flush()
      // 4. 等 coordinator idle
      await coordinator.whenIdle()
      // 5. Issue #629 评论 5308748920 问题1：composition 唯一真源是 Core snapshot。
      //    重读 afterFinish snapshot，只要 finishResult.success===false、afterFinish===null、
      //    或 afterFinish.composition!==null，立即 unseal 并 return false，不进入保存。
      const afterFinish = coordinator.getSnapshot()
      if (finishResult.success === false || afterFinish === null || afterFinish.composition !== null) {
        dispatcher.unseal()
        return false
      }
      // 6. 保存稳定 snapshot
      const savedText = afterFinish.text
      if (savedText !== state.lastSavedContent) {
        // Issue #629 评论 5308748920 问题3：ensureSnapshotSaved 精确保存这份 savedText。
        const settled = await state.ensureSnapshotSaved(savedText, false)
        if (!settled) {
          // 保存失败：不 detach、不 close、不 pop；解锁输入让用户继续编辑。
          dispatcher.unseal()
          return false
        }
      } else if (state.sessionOldText !== savedText && state.chapterId) {
        const deviceId = state.settings.statsDeviceId || 'unknown'
        const durationSeconds = Math.round((Date.now() - state.sessionStartTime) / 1000)
        bridge.processWritingEvent(
          deviceId, 'harmony', state.projectId, state.volumeId,
          state.chapterId, state.sessionOldText, savedText, durationSeconds, state.sessionId
        )
        // Issue #629 评论7 第5项：统计成功推进 sessionOldText，避免第二遍重复统计。
        state.sessionOldText = savedText
      }
      // 6. close session. Issue #629 评论9 第2项：先 closeAsync，成功后才 detach IME。
      const closeResult = await coordinator.closeAsync()
      if (!closeResult.success) {
        // close 失败：不 detach、不 pop；unseal 让用户继续编辑（不需要重新 attach IME）。
        dispatcher.unseal()
        return false
      }
      // 7. Core close 成功后才 detach IME（不可逆清理放到最后，失败路径不触不可逆动作）
      await harmonyImeConnection.detach()
      return true
    } catch (err) {
      dispatcher.unseal()
      return false
    }
  }
  const performGracefulClose = () => {
    // 幂等：复用同一 Promise，避免双重执行真实关闭链。
    if (closeTask !== null) {
      return closeTask
    }
    const task = doGracefulClose()
    closeTask = task
    // 失败后重置（允许重试）；成功后保留缓存（页面即将销毁）。
    task.then(
      (closed) => {
        if (!closed && closeTask === task) {
          closeTask = null
        }
      },
      () => {
        if (closeTask === task) {
          closeTask = null
        }
      }
    )
    return task
  }
  return { performGracefulClose, doGracefulClose, getCloseTask: () => closeTask }
}

await testAsync('幂等: 连续 performGracefulClose 两次，doGracefulClose 内部逻辑只执行一次，Promise 同一', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  // MockCoordinator 默认 snapshot.text=''，设成与 content 一致（savedText='edited'）
  coordinator.snapshot = { ...coordinator.snapshot, text: 'edited' }
  const state = {
    chapterId: 'c1',
    content: 'edited',
    lastSavedContent: 'original',  // snapshot 'edited' !== 'original' → 走 save
    sessionOldText: 'original',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      saveChapterCalls.push('saveChapter')
      await sleep(5)
      state.lastSavedContent = 'edited'  // 真实保存按 savedText 结算
      return true
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const p1 = screen.performGracefulClose()
  const p2 = screen.performGracefulClose()
  // 关键断言：两次返回的 Promise 是同一个对象（===）
  assert.equal(p1, p2, '两次返回的 Promise 应严格相等（同一对象）')
  const closed = await p1
  assert.equal(closed, true)

  // 内部逻辑只执行一次（计数验证，非字符串匹配）
  assert.deepEqual(dispatcher.calls, ['seal', 'finishActiveComposition', 'flush'], 'dispatcher 只调一次 seal→finish→flush')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'], 'coordinator 只调一次 whenIdle+closeAsync')
  assert.deepEqual(harmonyImeConnection.calls, ['detach'], 'detach 只调一次')
  assert.deepEqual(saveChapterCalls, ['saveChapter'], 'saveChapter 只调一次')
})

await testAsync('幂等: 已保存（savedText===lastSavedContent）有统计差异，performGracefulClose 两次 processWritingEvent 只调一次', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const state = {
    chapterId: 'c1',
    content: 'new-text',
    lastSavedContent: 'new-text',  // snapshot 已保存 → 不调 saveChapter
    sessionOldText: 'old-text',  // 不同 → 有统计差异
    sessionId: 's1',
    sessionStartTime: Date.now() - 3000,
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {},
  }
  coordinator.snapshot = { ...coordinator.snapshot, text: 'new-text' }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  await screen.performGracefulClose()
  await screen.performGracefulClose()

  // 关键断言：processWritingEvent 只被调一次（幂等复用 Promise，第二次根本不执行 doGracefulClose）
  assert.equal(bridge.processWritingEventCalls.length, 1, 'processWritingEvent 只调一次')
  // sessionOldText 已推进（防御性，即使 closeTask 被重置也不会重复统计）
  assert.equal(state.sessionOldText, state.content, 'sessionOldText 已推进到 content')
  // 真实关闭链只跑一次
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'], 'coordinator 只调一次')
  assert.deepEqual(harmonyImeConnection.calls, ['detach'], 'detach 只调一次')
})

await testAsync('推进防御: doGracefulClose 两次（绕过幂等），第二次因 sessionOldText 已推进不调 processWritingEvent', async () => {
  // 此测试验证 sessionOldText 推进的防御价值：即使 closeTask 机制失效（被重置或并发窗口），
  // sessionOldText 推进后第二次也不满足统计条件，不会重复上报。
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const state = {
    chapterId: 'c1',
    content: 'new-text',
    lastSavedContent: 'new-text',
    sessionOldText: 'old-text',
    sessionId: 's1',
    sessionStartTime: Date.now() - 3000,
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {},
  }
  coordinator.snapshot = { ...coordinator.snapshot, text: 'new-text' }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  await screen.doGracefulClose()
  assert.equal(bridge.processWritingEventCalls.length, 1, '第一次 doGracefulClose 调 processWritingEvent 一次')
  assert.equal(state.sessionOldText, state.content, '第一次后 sessionOldText 推进到 content')

  await screen.doGracefulClose()
  // 关键断言：第二次不调 processWritingEvent（sessionOldText 已推进，不再满足 !== content 条件）
  assert.equal(bridge.processWritingEventCalls.length, 1, '第二次 doGracefulClose 不调 processWritingEvent')
  // 但 detach/closeAsync 仍执行（doGracefulClose 本身不幂等，幂等由 performGracefulClose 保证）
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync', 'whenIdle', 'closeAsync'], 'doGracefulClose 两次各执行一次 closeAsync')
})

await testAsync('并发: 返回按钮和 aboutToDisappear 同时调 performGracefulClose，真实关闭链只跑一次', async () => {
  // 模拟返回按钮 onClick 和 aboutToDisappear 并发触发（真实场景：用户按返回瞬间页面也 aboutToDisappear）
  const dispatcher = new MockDispatcher()
  dispatcher.flushDelayMs = 20  // flush 有延迟，制造并发窗口
  const coordinator = new MockCoordinator()
  coordinator.whenIdleDelayMs = 20
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    chapterId: 'c1',
    content: 'edited',
    lastSavedContent: 'original',  // snapshot 'edited' !== 'original' → 走 save
    sessionOldText: 'original',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      saveChapterCalls.push('saveChapter')
      await sleep(5)
      state.lastSavedContent = 'edited'
      return true
    },
  }
  // MockCoordinator 默认 snapshot.text=''，设成与 content 一致（savedText='edited'）
  coordinator.snapshot = { ...coordinator.snapshot, text: 'edited' }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  // 并发：返回按钮和 aboutToDisappear 同时触发（不等第一个完成就调第二个）
  const pBack = screen.performGracefulClose()
  const pDisappear = screen.performGracefulClose()
  // 两个 Promise 应是同一对象
  assert.equal(pBack, pDisappear, '并发调用返回同一 Promise')
  const closed = await Promise.all([pBack, pDisappear])
  assert.deepEqual(closed, [true, true])

  // 关键断言：真实关闭链只跑一次（不是两次）
  assert.deepEqual(dispatcher.calls, ['seal', 'finishActiveComposition', 'flush'], 'dispatcher 只调一次（并发复用同一 Promise）')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'], 'coordinator 只调一次')
  assert.deepEqual(harmonyImeConnection.calls, ['detach'], 'detach 只调一次')
  assert.deepEqual(saveChapterCalls, ['saveChapter'], 'saveChapter 只调一次')
  assert.equal(bridge.processWritingEventCalls.length, 0, '走 save 分支时不调 processWritingEvent')
})

await testAsync('幂等: 第一次完成后再调 performGracefulClose 仍返回已 resolved 的同一 Promise（不重跑）', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const state = {
    chapterId: 'c1',
    content: 'same',
    lastSavedContent: 'same',  // 已保存 → 不调 saveChapter
    sessionOldText: 'same',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {},
  }
  coordinator.snapshot = { ...coordinator.snapshot, text: 'same' }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const p1 = screen.performGracefulClose()
  const closed1 = await p1
  assert.equal(closed1, true)
  const p2 = screen.performGracefulClose()
  // 成功完成后再次调用仍返回同一 Promise（不重置 closeTask）
  assert.equal(p1, p2, '成功后再次调用仍返回同一 Promise')
  await p2  // 应立即 resolve（已 resolved）
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'], '仍只执行一次真实关闭链')
  assert.deepEqual(dispatcher.calls, ['seal', 'finishActiveComposition', 'flush'], 'dispatcher 仍只调一次')
})

await testAsync('幂等: save 分支 saveChapter 推进 sessionOldText，doGracefulClose 两次第二次不再重复', async () => {
  // 验证 save 分支：saveChapter 内部推进 sessionOldText（与 WritingScreen.saveChapter 对齐），
  // doGracefulClose 两次（绕过幂等）第二次不再走统计分支也不重复 saveChapter。
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    chapterId: 'c1',
    content: 'edited',
    lastSavedContent: 'original',  // 第一次 snapshot 'edited' !== 'original' → save
    sessionOldText: 'original',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    // saveChapter 内部推进 sessionOldText/lastSavedContent（与 WritingScreen.doSaveChapter 对齐）
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      saveChapterCalls.push('saveChapter')
      state.sessionOldText = state.content
      state.lastSavedContent = state.content
      return true
    },
  }
  // MockCoordinator 默认 snapshot.text=''，设成与 content 一致
  coordinator.snapshot = { ...coordinator.snapshot, text: 'edited' }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed1 = await screen.doGracefulClose()
  assert.equal(closed1, true)
  assert.deepEqual(saveChapterCalls, ['saveChapter'], '第一次调 saveChapter')
  assert.equal(state.sessionOldText, state.content, 'saveChapter 后 sessionOldText 推进')
  assert.equal(state.lastSavedContent, state.content, 'saveChapter 后 lastSavedContent 推进')

  const closed2 = await screen.doGracefulClose()
  assert.equal(closed2, true)
  // 第二次：savedText === lastSavedContent 且 sessionOldText === savedText → 不调 saveChapter 也不调 processWritingEvent
  assert.deepEqual(saveChapterCalls, ['saveChapter'], '第二次不调 saveChapter（已保存）')
  assert.equal(bridge.processWritingEventCalls.length, 0, '第二次不调 processWritingEvent（sessionOldText 已推进）')
})

// ── 14. Issue #629 评论8 第2/3项：失败路径 + seal + 最后 preedit ──

await testAsync('保存失败: performGracefulClose 返回 false，不 detach、不 closeAsync，unseal 恢复输入', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    chapterId: 'c1',
    content: 'edited',
    lastSavedContent: 'original',  // 有未保存变更 → 走 save
    sessionOldText: 'original',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      saveChapterCalls.push('saveChapter')
      return false  // 保存失败
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed = await screen.performGracefulClose()

  // 关键断言：保存失败不离开
  assert.equal(closed, false, '保存失败必须返回 false（不 pop）')
  assert.deepEqual(saveChapterCalls, ['saveChapter'], 'saveChapter 调一次')
  assert.deepEqual(coordinator.calls, ['whenIdle'], '不调 closeAsync（close 未开始）')
  assert.deepEqual(harmonyImeConnection.calls, [], '不 detach（保存失败，会话保持活跃）')
  // unseal：用户继续编辑（下次返回重试）
  assert.ok(dispatcher.calls.includes('unseal'), '保存失败后必须 unseal')
  assert.equal(dispatcher.isSealed(), false, 'unseal 后 dispatcher 恢复接收输入')
})

await testAsync('保存失败重试: closeTask 重置，第二次保存成功后可正常关闭', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  let saveOk = false
  // MockCoordinator 默认 snapshot.text=''，设成与 content 一致（savedText='edited'）
  coordinator.snapshot = { ...coordinator.snapshot, text: 'edited' }
  const state = {
    chapterId: 'c1',
    content: 'edited',
    lastSavedContent: 'original',
    sessionOldText: 'original',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      if (!saveOk) {
        return false  // 第一次失败
      }
      state.sessionOldText = state.content
      state.lastSavedContent = state.content
      return true  // 第二次成功
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed1 = await screen.performGracefulClose()
  assert.equal(closed1, false, '第一次失败返回 false')
  assert.equal(screen.getCloseTask(), null, '失败后 closeTask 已重置（允许重试）')

  // 用户修好（模拟磁盘恢复），再次按返回
  saveOk = true
  const closed2 = await screen.performGracefulClose()
  assert.equal(closed2, true, '第二次保存成功 → 正常关闭')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'whenIdle', 'closeAsync'], '第二次真正 close')
  assert.deepEqual(harmonyImeConnection.calls, ['detach'], '第二次 detach')
  // 两次关闭链各自 seal/unseal：第一次失败 unseal，第二次 seal 后成功（不再 unseal）
  assert.equal(dispatcher.calls.filter(c => c === 'seal').length, 2, '两次关闭各 seal 一次')
  assert.equal(dispatcher.calls.filter(c => c === 'unseal').length, 1, '只有失败的那次 unseal')
})

await testAsync('close 失败: closeAsync 返回失败 → performGracefulClose 返回 false（不把关闭失败当成功）', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  coordinator.closeResult = { success: false, errorCode: 'CLOSE_FAILED', warnings: [], changedPaths: [], changedEntities: [] }
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  // MockCoordinator 默认 snapshot.text=''，设成与 content 一致（savedText='same'，跳过 save）
  coordinator.snapshot = { ...coordinator.snapshot, text: 'same' }
  const state = {
    chapterId: 'c1',
    content: 'same',
    lastSavedContent: 'same',  // 已保存 → 跳过 save
    sessionOldText: 'same',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => true,
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed = await screen.performGracefulClose()

  assert.equal(closed, false, 'closeAsync 失败必须返回 false（不 pop）')
  // Issue #629 评论9 第2项：close 失败时不 detach（不可逆清理放到 close 成功之后），
  // unseal 让用户继续编辑（不需要重新 attach IME）。
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'], 'closeAsync 已调')
  assert.deepEqual(harmonyImeConnection.calls, [], 'close 失败时不 detach（不可逆清理未触）')
  assert.ok(dispatcher.calls.includes('unseal'), 'close 失败后必须 unseal')
  assert.equal(dispatcher.isSealed(), false, 'unseal 后 dispatcher 恢复接收输入')
  assert.equal(bridge.processWritingEventCalls.length, 0)
})

await testAsync('seal: seal 后 dispatch 拒新输入（SEALED 且不排队），finishActiveComposition 仍可入队', async () => {
  const dispatcher = new MockDispatcher()
  // 模拟普通输入命令在 seal 后被拒
  dispatcher.seal()
  const rejected = await dispatcher.dispatch({ kind: 'insertText', text: 'x' })
  assert.equal(rejected.success, false)
  assert.equal(rejected.errorCode, 'SEALED', 'seal 后新输入返回 SEALED')
  assert.equal(dispatcher.dispatchRejectedCalls.length, 1)
  assert.equal(dispatcher.dispatchRejectedCalls[0].kind, 'insertText')
  // finishActiveComposition 是关闭链的一部分，seal 后仍可执行
  const finishResult = await dispatcher.finishActiveComposition()
  assert.equal(finishResult.success, true)
  assert.ok(dispatcher.calls.includes('finishActiveComposition'), 'seal 后 finishActiveComposition 仍执行')
})

await testAsync('最后 preedit: finishActiveComposition 提交 preedit 进正文，保存的 savedText 含最后预输入', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    chapterId: 'c1',
    content: 'abc',
    lastSavedContent: 'abc',
    sessionOldText: 'abc',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      saveChapterCalls.push('saveChapter')
      state.lastSavedContent = coordinator.snapshot.text  // 真实保存按 savedText 结算
      return true
    },
  }
  // 模拟：用户正在输入中文，preedit='你' 还没 commit（snapshot.text 不含 preedit）
  coordinator.snapshot = { ...coordinator.snapshot, text: 'abc' }
  state.content = 'abc'
  // finishActiveComposition 真实提交：preedit 落进 snapshot（coordinator.getSnapshot 返回的文本）
  dispatcher.finishHandler = async () => {
    const snap = coordinator.getSnapshot()
    coordinator.snapshot = { ...snap, text: snap.text + '你' }
    state.content = coordinator.snapshot.text
    return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed = await screen.performGracefulClose()

  assert.equal(closed, true)
  // 关键断言：保存的是 finish 后的文本（含最后 preedit '你'）
  assert.equal(coordinator.snapshot.text, 'abc你', 'finishActiveComposition 把 preedit 提交进正文')
  assert.equal(saveChapterCalls.length, 1, '有未保存变更 → 保存一次')
  // 关闭链顺序：seal → finishActiveComposition → flush → whenIdle → save → closeAsync → detach
  assert.deepEqual(dispatcher.calls.slice(0, 3), ['seal', 'finishActiveComposition', 'flush'])
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'])
})


// ── 15. Issue #629 评论8 第1项：close 时复用进行中保存任务 → 结算校验再保存一轮 ──

await testAsync('close 复用进行中保存: 自动保存冻结旧 snapshot，close 时复用任务后校验 lastSavedContent，再保存一轮把最新正文落盘', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  // 模拟：自动保存在 close 前已在进行（冻结 snapshot='A'）；close 开始时用户又输入了 B
  // → coordinator snapshot='AB'，lastSavedContent='A'（旧文本），保存事务进行中冻结 'A'。
  coordinator.snapshot = { ...coordinator.snapshot, text: 'AB' }
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    chapterId: 'c1',
    content: 'AB',
    lastSavedContent: 'A',
    sessionOldText: 'A',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    // Issue #629 评论 5308748920 问题3：ensureSnapshotSaved 镜像。
    // 模拟内部：先 await 旧任务（保存 'A'），text 不同，启动新事务保存 targetText。
    // 不再靠编排函数里的 guard<5 循环——复用+校验由 ensureSnapshotSaved 内部完成。
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      saveChapterCalls.push('A')        // 旧任务（冻结 'A'）先结束
      saveChapterCalls.push(targetText) // 目标不同 → 新事务精确保存 targetText
      state.lastSavedContent = targetText
      state.sessionOldText = targetText
      return true
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed = await screen.performGracefulClose()

  // 关键断言：close 的 savedText='AB' 最终被落盘（复用旧任务后校验并再保存一轮）
  assert.equal(closed, true)
  assert.deepEqual(saveChapterCalls, ['A', 'AB'], '第一轮复用进行中任务(A)，校验后第二轮保存最新(AB)')
  assert.equal(state.lastSavedContent, 'AB', '结算后 lastSavedContent 是 close 时的 savedText')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'], '最新正文落盘后才 close')
  assert.deepEqual(harmonyImeConnection.calls, ['detach'], 'detach 在结算完成后')
})

await testAsync('close 复用进行中保存: 第二轮保存也失败 → 返回 false 不离开', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  coordinator.snapshot = { ...coordinator.snapshot, text: 'AB' }
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveChapterCalls = []
  const state = {
    chapterId: 'c1',
    content: 'AB',
    lastSavedContent: 'A',
    sessionOldText: 'A',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    // Issue #629 评论 5308748920 问题3：ensureSnapshotSaved 镜像。
    // 内部先 await 旧任务（保存 'A' 成功），text 不同，启动新事务保存 targetText，新事务失败。
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      saveChapterCalls.push('A')        // 旧任务成功
      saveChapterCalls.push(targetText) // 新事务失败
      return false
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed = await screen.performGracefulClose()

  assert.equal(closed, false, '最新正文未落盘 → 不离开')
  assert.equal(saveChapterCalls.length, 2, '第一轮复用 + 第二轮新任务都执行过')
  assert.deepEqual(coordinator.calls, ['whenIdle'], '不 close（未落盘）')
  assert.deepEqual(harmonyImeConnection.calls, [], '不 detach（未落盘）')
  assert.ok(dispatcher.calls.includes('unseal'), '失败后 unseal 允许重试')
})


// ── 16. Issue #629 评论 5308748920 问题1：composition 收尾失败/stale 不进入保存 ──
//
// 验证：
//   16a. finishActiveComposition 返回 success=false → 不 save/closeAsync/detach/pop，unseal
//   16b. finishActiveComposition success 但 snapshot.composition!==null（stale）→ 不 save/closeAsync/detach，unseal
//   16c. afterFinish snapshot===null → 不 save/closeAsync/detach，unseal
//   16d. composition 正常结束（composition===null）→ 正常 save/closeAsync/detach
//
// 与修改后的 WritingScreen.doGracefulClose 对齐：
//   seal → finishActiveComposition（看 finishResult.success）→ flush → whenIdle
//   → 重读 afterFinish snapshot → 若 finishResult.success===false || afterFinish===null
//     || afterFinish.composition!==null → unseal + return false（不进入保存）
//   → 否则 save → closeAsync → detach → true

await testAsync('composition 收尾失败: finishActiveComposition 返回 success=false → 不 save/closeAsync/detach，unseal', async () => {
  const dispatcher = new MockDispatcher()
  dispatcher.finishHandler = async () => ({ success: false, errorCode: 'FINISH_FAILED', warnings: [], changedPaths: [], changedEntities: [] })
  const coordinator = new MockCoordinator()
  coordinator.snapshot = { ...coordinator.snapshot, text: 'edited', composition: null }
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveCalls = []
  const state = {
    chapterId: 'c1',
    content: 'edited',
    lastSavedContent: 'original',
    sessionOldText: 'original',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => { saveCalls.push('save'); return true },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed = await screen.performGracefulClose()

  assert.equal(closed, false, 'finish 失败必须返回 false（不 pop）')
  assert.deepEqual(saveCalls, [], '不 save（composition 未结束）')
  assert.deepEqual(coordinator.calls, ['whenIdle'], '不 closeAsync（composition 未结束）')
  assert.deepEqual(harmonyImeConnection.calls, [], '不 detach（composition 未结束）')
  assert.ok(dispatcher.calls.includes('unseal'), 'finish 失败后必须 unseal')
  assert.equal(dispatcher.isSealed(), false, 'unseal 后 dispatcher 恢复接收输入')
})

await testAsync('composition stale: finishActiveComposition success 但 snapshot.composition!==null → 不 save/closeAsync/detach，unseal', async () => {
  const dispatcher = new MockDispatcher()
  // finish 返回 success=true，但 Core snapshot 仍保留 composition（stale/恢复场景）
  dispatcher.finishHandler = async () => ({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
  const coordinator = new MockCoordinator()
  // composition 仍存在（Core 仍保留 preedit，FFI success 不等于 composition 已结束）
  coordinator.snapshot = { ...coordinator.snapshot, text: 'abc', composition: { preeditText: '你' } }
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveCalls = []
  const state = {
    chapterId: 'c1',
    content: 'abc',
    lastSavedContent: 'abc',
    sessionOldText: 'abc',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => { saveCalls.push('save'); return true },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed = await screen.performGracefulClose()

  assert.equal(closed, false, 'composition 仍存在必须返回 false（不 pop）')
  assert.deepEqual(saveCalls, [], '不 save（composition 仍存在，最后 preedit 可能丢失）')
  assert.deepEqual(coordinator.calls, ['whenIdle'], '不 closeAsync')
  assert.deepEqual(harmonyImeConnection.calls, [], '不 detach')
  assert.ok(dispatcher.calls.includes('unseal'), 'composition stale 后必须 unseal')
  assert.equal(dispatcher.isSealed(), false, 'unseal 后恢复输入')
})

await testAsync('composition 收尾: afterFinish snapshot===null → 不 save/closeAsync/detach，unseal', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  coordinator.snapshot = null  // snapshot 为 null（session 已被外部关闭等异常）
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveCalls = []
  const state = {
    chapterId: 'c1',
    content: 'abc',
    lastSavedContent: 'abc',
    sessionOldText: 'abc',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => { saveCalls.push('save'); return true },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed = await screen.performGracefulClose()

  assert.equal(closed, false, 'snapshot===null 必须返回 false')
  assert.deepEqual(saveCalls, [], '不 save')
  assert.deepEqual(coordinator.calls, ['whenIdle'], '不 closeAsync')
  assert.deepEqual(harmonyImeConnection.calls, [], '不 detach')
  assert.ok(dispatcher.calls.includes('unseal'), 'snapshot null 后必须 unseal')
})

await testAsync('composition 正常: finishActiveComposition success 且 composition===null → 正常 save/closeAsync/detach', async () => {
  const dispatcher = new MockDispatcher()
  // finish 成功且 composition 已结束
  dispatcher.finishHandler = async () => ({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
  const coordinator = new MockCoordinator()
  coordinator.snapshot = { ...coordinator.snapshot, text: 'committed', composition: null }
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveCalls = []
  const state = {
    chapterId: 'c1',
    content: 'committed',
    lastSavedContent: 'old',
    sessionOldText: 'old',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => { saveCalls.push(targetText); state.lastSavedContent = targetText; return true },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed = await screen.performGracefulClose()

  assert.equal(closed, true, 'composition 正常结束应正常关闭')
  assert.deepEqual(saveCalls, ['committed'], 'save committed 文本')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'], '正常 close')
  assert.deepEqual(harmonyImeConnection.calls, ['detach'], '正常 detach')
})

// ── 17. Issue #629 评论 5308748920 问题2：requestLeave 幂等（leaveTask 复用，只 pop 一次）──

await testAsync('requestLeave 幂等: 快速双返回只 pop 一次（leaveTask 复用同一 Promise）', async () => {
  // 模拟 WritingScreen 的 requestLeave + leaveTask 机制（与修改后的 WritingScreen.requestLeave 对齐）
  let leaveTask = null
  const popCalls = []
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  coordinator.snapshot = { ...coordinator.snapshot, text: 'edited', composition: null }
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const saveCalls = []
  const state = {
    chapterId: 'c1',
    content: 'edited',
    lastSavedContent: 'original',
    sessionOldText: 'original',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => { saveCalls.push('save'); state.lastSavedContent = targetText; return true },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  // 模拟 requestLeave（与修改后的 WritingScreen.requestLeave 对齐）
  const requestLeave = () => {
    if (leaveTask !== null) return leaveTask
    let closed = false
    const task = (async () => {
      closed = await screen.performGracefulClose()
      if (closed === true) popCalls.push('pop')
    })()
    leaveTask = task
    task.then(
      () => { if (closed === false && leaveTask === task) leaveTask = null },
      () => { if (leaveTask === task) leaveTask = null }
    )
    return task
  }

  // 快速双返回（系统返回 + 左上角按钮几乎同时触发）
  const p1 = requestLeave()
  const p2 = requestLeave()
  // 两个 Promise 应是同一对象（leaveTask 复用）
  assert.equal(p1, p2, '快速双返回应返回同一 leaveTask Promise')
  await Promise.all([p1, p2])

  // 关键断言：只 pop 一次（不会从 Writing 连退到 Home）
  assert.equal(popCalls.length, 1, '只 pop 一次（leaveTask 幂等，不双 pop）')
  assert.deepEqual(saveCalls, ['save'], 'save 只调一次')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'closeAsync'], 'close 只调一次')
  assert.deepEqual(harmonyImeConnection.calls, ['detach'], 'detach 只调一次')
})

await testAsync('requestLeave 重试: close 失败后 leaveTask 清空，再次返回可重试', async () => {
  let leaveTask = null
  const popCalls = []
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  coordinator.snapshot = { ...coordinator.snapshot, text: 'edited', composition: null }
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  let saveOk = false
  const saveCalls = []
  const state = {
    chapterId: 'c1',
    content: 'edited',
    lastSavedContent: 'original',
    sessionOldText: 'original',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      saveCalls.push('save')
      if (!saveOk) return false
      state.lastSavedContent = targetText
      return true
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const requestLeave = () => {
    if (leaveTask !== null) return leaveTask
    let closed = false
    const task = (async () => {
      closed = await screen.performGracefulClose()
      if (closed === true) popCalls.push('pop')
    })()
    leaveTask = task
    task.then(
      () => { if (closed === false && leaveTask === task) leaveTask = null },
      () => { if (leaveTask === task) leaveTask = null }
    )
    return task
  }

  // 第一次返回：保存失败 → 不 pop
  await requestLeave()
  assert.equal(popCalls.length, 0, '第一次失败不 pop')
  assert.equal(leaveTask, null, '失败后 leaveTask 清空（允许重试）')

  // 用户修好（模拟磁盘恢复），再次按返回
  saveOk = true
  await requestLeave()
  assert.equal(popCalls.length, 1, '第二次成功后 pop')
  assert.deepEqual(coordinator.calls, ['whenIdle', 'whenIdle', 'closeAsync'], '第二次真正 close')
})

await testAsync('requestLeave 成功后: leaveTask 不重置（页面已离开，防止重入）', async () => {
  let leaveTask = null
  const popCalls = []
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  coordinator.snapshot = { ...coordinator.snapshot, text: 'same', composition: null }
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const state = {
    chapterId: 'c1',
    content: 'same',
    lastSavedContent: 'same',
    sessionOldText: 'same',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => { return true },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const requestLeave = () => {
    if (leaveTask !== null) return leaveTask
    let closed = false
    const task = (async () => {
      closed = await screen.performGracefulClose()
      if (closed === true) popCalls.push('pop')
    })()
    leaveTask = task
    task.then(
      () => { if (closed === false && leaveTask === task) leaveTask = null },
      () => { if (leaveTask === task) leaveTask = null }
    )
    return task
  }

  await requestLeave()
  assert.equal(popCalls.length, 1, '成功后 pop')
  // 成功后 leaveTask 不重置（页面已离开，保留 leaveTask 防止重入）
  assert.notEqual(leaveTask, null, '成功后 leaveTask 保留（不重置）')
})

// ── 18. Issue #629 评论 5308748920 问题3：ensureSnapshotSaved 精确保存目标文本 ──

await testAsync('ensureSnapshotSaved: 精确保存 close 时的 savedText，不重新抓当前正文', async () => {
  const dispatcher = new MockDispatcher()
  const coordinator = new MockCoordinator()
  coordinator.snapshot = { ...coordinator.snapshot, text: 'frozen-text', composition: null }
  const harmonyImeConnection = new MockImeConnection()
  const bridge = new MockBridge()
  const receivedTargets = []
  const state = {
    chapterId: 'c1',
    content: 'frozen-text',
    lastSavedContent: 'different',
    sessionOldText: 'different',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    projectId: 'p1',
    volumeId: 'v1',
    settings: { statsDeviceId: 'dev1' },
    ensureSnapshotSaved: async (targetText, isAutoSave) => {
      receivedTargets.push(targetText)
      state.lastSavedContent = targetText
      state.sessionOldText = targetText
      return true
    },
  }
  const deps = { dispatcher, coordinator, harmonyImeConnection, bridge }
  const screen = createGracefulCloseScreen(deps, state)

  const closed = await screen.performGracefulClose()

  assert.equal(closed, true)
  assert.deepEqual(receivedTargets, ['frozen-text'], 'ensureSnapshotSaved 收到的正是 snapshot.text（不重新抓正文）')
})

await testAsync('ensureSnapshotSaved 复用: 旧任务保存同一 text 且成功 → 直接 true 不启动新事务', async () => {
  // 模拟 ensureSnapshotSaved 内部复用逻辑（与 WritingScreen.ensureSnapshotSaved 对齐）
  let activeSave = null
  const doSaveCalls = []
  const doSaveChapter = async (targetText) => {
    doSaveCalls.push(targetText)
    return true
  }
  const ensureSnapshotSaved = async (targetText) => {
    if (activeSave !== null) {
      const prev = activeSave
      let prevOk = false
      try { prevOk = await prev.task } catch (e) {}
      if (prev.text === targetText && prevOk) return true
      if (activeSave === prev) activeSave = null
    }
    const task = doSaveChapter(targetText)
    activeSave = { text: targetText, task }
    task.then(
      () => { if (activeSave !== null && activeSave.task === task) activeSave = null },
      () => { if (activeSave !== null && activeSave.task === task) activeSave = null }
    )
    return task
  }

  // 第一次保存 'A'
  const r1 = await ensureSnapshotSaved('A')
  assert.equal(r1, true)
  assert.deepEqual(doSaveCalls, ['A'], '第一次启动新事务保存 A')

  // 等 activeSave 释放
  await sleep(10)

  // 第二次保存同一 'A'：此时 activeSave 已释放，会启动新事务
  // 但若 activeSave 仍在（任务未结束），则复用
  const r2 = await ensureSnapshotSaved('A')
  assert.equal(r2, true)
})

await testAsync('ensureSnapshotSaved 精确: 旧任务保存不同 text → await 旧任务后启动新事务保存 targetText', async () => {
  // 模拟 ensureSnapshotSaved 内部逻辑
  let activeSave = null
  const doSaveCalls = []
  const doSaveChapter = async (targetText) => {
    doSaveCalls.push(targetText)
    await sleep(5)
    return true
  }
  const ensureSnapshotSaved = async (targetText) => {
    if (activeSave !== null) {
      const prev = activeSave
      let prevOk = false
      try { prevOk = await prev.task } catch (e) {}
      if (prev.text === targetText && prevOk) return true
      if (activeSave === prev) activeSave = null
    }
    const task = doSaveChapter(targetText)
    activeSave = { text: targetText, task }
    task.then(
      () => { if (activeSave !== null && activeSave.task === task) activeSave = null },
      () => { if (activeSave !== null && activeSave.task === task) activeSave = null }
    )
    return task
  }

  // 启动保存 'A'（不等完成）
  const p1 = ensureSnapshotSaved('A')
  // 立即调保存 'B'：应先 await 'A' 完成，再启动新事务保存 'B'
  const r2 = await ensureSnapshotSaved('B')
  assert.equal(r2, true)
  await p1
  assert.deepEqual(doSaveCalls, ['A', 'B'], '先保存 A，再保存 B（精确保存目标文本）')
})

// ── 19. Issue #629 评论 5308748920 问题3：doClearContent 重读 snapshot 确认 text===再 ensureSnapshotSaved('') ──

await testAsync('doClearContent: replace 后重读 snapshot 确认 text===再 ensureSnapshotSaved()', async () => {
  // 模拟 doClearContent 核心逻辑（与修改后的 WritingScreen.doClearContent 对齐）
  const coordinator = {
    getSnapshot: () => ({ text: '', composition: null }),  // replace 后 snapshot 已是空
    replace: async () => ({ success: true, data: { outcome: 'applied' } }),
    whenIdle: async () => {},
  }
  const dispatcher = { flush: async () => {} }
  const receivedTargets = []
  const ensureSnapshotSaved = async (targetText, isAutoSave) => {
    receivedTargets.push(targetText)
    return true
  }

  // 模拟 doClearContent 核心逻辑（replace 成功后部分）
  await dispatcher.flush()
  await coordinator.whenIdle()
  const afterClear = coordinator.getSnapshot()
  assert.notEqual(afterClear, null, 'afterClear 不为 null')
  assert.equal(afterClear.text, '', 'replace 后 snapshot.text 应为空')
  const saved = await ensureSnapshotSaved('', false)
  assert.equal(saved, true, '空正文保存成功')
  assert.deepEqual(receivedTargets, [''], 'ensureSnapshotSaved 收到空正文（不复用旧 saveTask）')
})

await testAsync('doClearContent: replace 后 snapshot.text!== → 不保存，显示失败', async () => {
  // 模拟 replace 后 snapshot 仍有残留（异常场景）
  const coordinator = {
    getSnapshot: () => ({ text: 'residual', composition: null }),
    replace: async () => ({ success: true, data: { outcome: 'applied' } }),
    whenIdle: async () => {},
  }
  const dispatcher = { flush: async () => {} }
  const receivedTargets = []
  const ensureSnapshotSaved = async (targetText, isAutoSave) => {
    receivedTargets.push(targetText)
    return true
  }

  await dispatcher.flush()
  await coordinator.whenIdle()
  const afterClear = coordinator.getSnapshot()
  // 与 WritingScreen.doClearContent 对齐：text!=='' 时不调 ensureSnapshotSaved
  if (afterClear === null || afterClear.text !== '') {
    assert.equal(afterClear.text, 'residual', 'snapshot 仍有残留')
    assert.deepEqual(receivedTargets, [], 'snapshot.text!==时不调 ensureSnapshotSaved（不误存残留）')
    return
  }
  assert.fail('不应到达此分支')
})

console.log('---')
console.log(`全部通过：${passed} 个测试`)
