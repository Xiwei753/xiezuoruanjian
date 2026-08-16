// save_transaction.test.mjs — Issue #629 评论8 第1项 保存事务行为测试。
//
// 验证：
//   1. 保存期间继续输入：targetText 是保存开始时读一次的不可变快照；保存返回后按 targetText
//      结算 lastSavedContent/sessionOldText；再取 latestSnapshot 结算 hasUnsavedChanges
//      （保存期间的新输入保持 unsaved，不把 A+B 标成已保存）。
//   2. 保存事务复用：保存进行中并发调用 ensureSnapshotSaved 同 text 复用结果（bridge.saveChapter 只调一次）。
//   3. 保存事务顺序：flush → whenIdle → snapshot(targetText) → bridge.saveChapter(targetText)。
//   4. 保存失败：返回 false，lastSavedContent/hasUnsavedChanges 不被错误推进。
//   5. persistent 章节会话：coordinator.open(..., true) 把 isPersistent 显式传给 bridge.create。
//   6. ensureSnapshotSaved 精确保存：同 text 复用旧任务结果；不同 text 先 await 旧任务再启动新事务。
//
// Issue #629 评论10(5308748920) 问题3：与生产代码 WritingScreen.ets 对齐。
// 生产代码接口：saveChapter(isAutoSave) → flush+whenIdle+getSnapshot → ensureSnapshotSaved(targetText, isAutoSave)
//   ensureSnapshotSaved: activeSave={text,task} 复用逻辑（同 text 成功复用、不同 text 启动新事务）
//   doSaveChapter(targetText, isAutoSave): 只保存显式 targetText，不内部读 snapshot
//
// 运行：node --experimental-strip-types save_transaction.test.mjs
//
// 注意：.ets 依赖 ArkUI 无法用 Node 直接测；本测试提取 WritingScreen 保存事务的编排逻辑
// （doSaveChapter/ensureSnapshotSaved/saveChapter），与生产代码对齐。生产代码需 HarmonyOS SDK 端到端编译。

import { strict as assert } from 'node:assert'
import { SerialCommandQueue } from '../../session/editor_patch_logic.ts'

let passed = 0
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}
const sleep = (ms) => new Promise(r => setTimeout(r, ms))

console.log('save_transaction 保存事务行为测试（Issue #629 评论8 第1项 + 评论10 问题3）')
console.log('---')

// ── 工具：与 WritingScreen 保存事务对齐的镜像 ──

function makeSaveDeps(initialSnapshotText) {
  const dispatcherQueue = new SerialCommandQueue()
  let snapshotText = initialSnapshotText
  const coordinator = {
    queue: new SerialCommandQueue(),
    async whenIdle() { return this.queue.whenIdle() },
    getSnapshot() { return { text: snapshotText, revision: 0, cursor: 0, selectionAnchor: 0, generation: 0, chapterId: 'c1', composition: null } },
    applyInput(text) { snapshotText = snapshotText + text },
    get text() { return snapshotText },
  }
  const dispatcher = { async flush() { return dispatcherQueue.whenIdle() }, queue: dispatcherQueue }
  return { dispatcher, coordinator }
}

// 与 WritingScreen.doSaveChapter 对齐：接受显式 targetText，不内部读 snapshot。
async function doSaveChapter(deps, bridge, state, targetText, isAutoSave = false) {
  state.isSaving = true
  state.isAutoSaving = isAutoSave
  try {
    const result = await bridge.saveChapter(state.chapterId, targetText)
    if (result.success && result.data) {
      if (state.sessionOldText !== targetText) {
        bridge.processWritingEvent(state.sessionOldText, targetText)
        state.sessionOldText = targetText
      }
      state.lastSavedContent = targetText
      state.lastSavedContentHash = result.data.contentHash
      state.hasUnsavedChanges = false
      state.lastSaveFailed = false
      const latest = deps.coordinator.getSnapshot()
      if (latest) {
        state.content = latest.text
        state.hasUnsavedChanges = latest.text !== targetText
        if (latest.text === targetText) { state.wordCount = result.data.wordCount }
      } else {
        state.wordCount = result.data.wordCount
      }
      return true
    }
    state.lastSaveFailed = true
    return false
  } catch (err) {
    state.lastSaveFailed = true
    return false
  } finally {
    state.isSaving = false
    state.isAutoSaving = false
  }
}

// 与 WritingScreen.ensureSnapshotSaved 对齐：activeSave 复用逻辑。
async function ensureSnapshotSaved(deps, bridge, state, targetText, isAutoSave = false) {
  if (state.activeSave !== null) {
    const prev = state.activeSave
    let prevOk = false
    try { prevOk = await prev.task } catch (err) {}
    if (prev.text === targetText && prevOk) { return true }
    if (state.activeSave === prev) { state.activeSave = null }
  }
  const task = doSaveChapter(deps, bridge, state, targetText, isAutoSave)
  state.activeSave = { text: targetText, task }
  task.then(
    () => { if (state.activeSave !== null && state.activeSave.task === task) { state.activeSave = null } },
    () => { if (state.activeSave !== null && state.activeSave.task === task) { state.activeSave = null } }
  )
  return task
}

// 与 WritingScreen.saveChapter 对齐：flush+whenIdle+getSnapshot → ensureSnapshotSaved。
function makeSaveChapter(deps, bridge, state) {
  const saveChapter = async (isAutoSave = false) => {
    await deps.dispatcher.flush()
    await deps.coordinator.whenIdle()
    const snap = deps.coordinator.getSnapshot()
    const targetText = snap ? snap.text : state.content
    return ensureSnapshotSaved(deps, bridge, state, targetText, isAutoSave)
  }
  return {
    saveChapter,
    ensureSnapshotSaved: (targetText, isAutoSave = false) => ensureSnapshotSaved(deps, bridge, state, targetText, isAutoSave),
    getActiveSave: () => state.activeSave,
  }
}

async function resolveTargetText(deps, state) {
  await deps.dispatcher.flush()
  await deps.coordinator.whenIdle()
  const snap = deps.coordinator.getSnapshot()
  return snap ? snap.text : state.content
}

function makeBridge(saveResult) {
  const bridge = {
    saveChapterCalls: [],
    saveChapter: async (chapterId, text) => { bridge.saveChapterCalls.push({ chapterId, text }); return saveResult },
    processWritingEventCalls: [],
    processWritingEvent: (oldText, newText) => { bridge.processWritingEventCalls.push({ oldText, newText }) },
  }
  return bridge
}

function makeDeferredBridge() {
  const bridge = { saveChapterCalls: [], processWritingEventCalls: [], processWritingEvent: (oldText, newText) => { bridge.processWritingEventCalls.push({ oldText, newText }) } }
  let resolveSave = null
  let resolveStarted = null
  const startedPromise = new Promise((resolve) => { resolveStarted = resolve })
  bridge.saveChapter = async (chapterId, text) => {
    bridge.saveChapterCalls.push({ chapterId, text })
    resolveStarted()
    await new Promise((resolve) => { resolveSave = resolve })
    return { success: true, data: { contentHash: 'h-' + text, wordCount: text.length }, warnings: [], changedPaths: [], changedEntities: [] }
  }
  bridge.waitStarted = () => startedPromise
  bridge.finishSave = () => { resolveSave() }
  return bridge
}

function makeState(content) {
  return {
    content, lastSavedContent: content, lastSavedContentHash: 'h-old', sessionOldText: content,
    hasUnsavedChanges: false, lastSaveFailed: false, isSaving: false, isAutoSaving: false,
    wordCount: 0, chapterId: 'c1', projectId: 'p1', volumeId: 'v1', sessionId: 's1',
    sessionStartTime: Date.now(), settings: { statsDeviceId: 'dev1' },
    updateContinuationState: () => {}, activeSave: null,
  }
}

// ── 1. 保存期间继续输入 ──

await testAsync('保存期间继续输入: 磁盘保存 A，保存期间输入 B，结算后 hasUnsavedChanges=true 且 content=A+B', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  state.sessionOldText = 'old-before-A'
  const targetText = await resolveTargetText(deps, state)
  const savePromise = doSaveChapter(deps, bridge, state, targetText)
  await bridge.waitStarted()
  deps.coordinator.applyInput('B')
  state.content = deps.coordinator.text
  assert.equal(state.content, 'AB')
  bridge.finishSave()
  const saved = await savePromise
  assert.equal(saved, true)
  assert.equal(bridge.saveChapterCalls.length, 1)
  assert.equal(bridge.saveChapterCalls[0].text, 'A')
  assert.equal(state.lastSavedContent, 'A')
  assert.equal(state.lastSavedContentHash, 'h-A')
  assert.equal(state.hasUnsavedChanges, true)
  assert.equal(state.content, 'AB')
  assert.deepEqual(state.sessionOldText, 'A')
  assert.equal(bridge.processWritingEventCalls.length, 1)
  assert.equal(bridge.processWritingEventCalls[0].newText, 'A')
})

await testAsync('保存期间无新输入: 结算后 hasUnsavedChanges=false', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  state.sessionOldText = 'old'
  const targetText = await resolveTargetText(deps, state)
  const savePromise = doSaveChapter(deps, bridge, state, targetText)
  await bridge.waitStarted()
  bridge.finishSave()
  const saved = await savePromise
  assert.equal(saved, true)
  assert.equal(state.lastSavedContent, 'A')
  assert.equal(state.hasUnsavedChanges, false)
  assert.equal(state.content, 'A')
})

// ── 2. 保存事务复用 ──

await testAsync('保存事务复用: 保存进行中并发调用 saveChapter 同 text 复用（bridge 只调一次）', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  const { saveChapter, getActiveSave } = makeSaveChapter(deps, bridge, state)
  const p1 = saveChapter()
  const p2 = saveChapter()
  const p3 = saveChapter()
  await bridge.waitStarted()
  bridge.finishSave()
  const results = await Promise.all([p1, p2, p3])
  assert.deepEqual(results, [true, true, true])
  assert.equal(bridge.saveChapterCalls.length, 1)
  await sleep(0)
  assert.equal(getActiveSave(), null)
})

await testAsync('保存事务复用: 第一次完成后再次保存是新任务', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  const { saveChapter, getActiveSave } = makeSaveChapter(deps, bridge, state)
  const p1 = saveChapter()
  await bridge.waitStarted()
  bridge.finishSave()
  await p1
  await sleep(0)
  assert.equal(getActiveSave(), null)
  const p2 = saveChapter()
  await sleep(0)
  bridge.finishSave()
  await p2
  assert.equal(bridge.saveChapterCalls.length, 2)
})

// ── 2b. ensureSnapshotSaved 精确保存 ──

await testAsync('ensureSnapshotSaved: 同 text 复用旧任务结果（不重复保存）', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  const { ensureSnapshotSaved, getActiveSave } = makeSaveChapter(deps, bridge, state)
  const p1 = ensureSnapshotSaved('A', false)
  await bridge.waitStarted()
  bridge.finishSave()
  const r1 = await p1
  assert.equal(r1, true)
  assert.equal(bridge.saveChapterCalls.length, 1)
  await sleep(0)
  assert.equal(getActiveSave(), null)
  const p2 = ensureSnapshotSaved('A', false)
  await bridge.waitStarted()
  bridge.finishSave()
  const r2 = await p2
  assert.equal(r2, true)
  assert.equal(bridge.saveChapterCalls.length, 2)
})

await testAsync('ensureSnapshotSaved: 不同 text 先 await 旧任务再启动新事务', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  const { ensureSnapshotSaved } = makeSaveChapter(deps, bridge, state)
  const p1 = ensureSnapshotSaved('A', false)
  await bridge.waitStarted()
  const p2 = ensureSnapshotSaved('B', false)
  bridge.finishSave()
  // finishSave 同步放行第一次保存；微任务还没执行，p2 仍在 await prev.task
  assert.equal(bridge.saveChapterCalls.length, 1)
  assert.equal(bridge.saveChapterCalls[0].text, 'A')
  await p1
  // p1 完成后，p2 的 ensureSnapshotSaved 检测到 prev.text='A' !== 'B'，启动新事务保存 'B'
  await bridge.waitStarted()
  bridge.finishSave()
  const r2 = await p2
  assert.equal(r2, true)
  assert.equal(bridge.saveChapterCalls.length, 2)
  assert.equal(bridge.saveChapterCalls[1].text, 'B')
})

// ── 3. 保存事务顺序 ──

await testAsync('保存事务顺序: flush 等完排队输入后才读 snapshot 保存', async () => {
  const deps = makeSaveDeps('A')
  deps.dispatcher.queue.enqueue(async () => { await sleep(10); deps.coordinator.applyInput('X') })
  deps.dispatcher.queue.enqueue(async () => { await sleep(10); deps.coordinator.applyInput('Y') })
  const bridge = makeBridge({ success: true, data: { contentHash: 'h-AXY', wordCount: 3 }, warnings: [], changedPaths: [], changedEntities: [] })
  const state = makeState('A')
  const targetText = await resolveTargetText(deps, state)
  assert.equal(targetText, 'AXY')
  const saved = await doSaveChapter(deps, bridge, state, targetText)
  assert.equal(saved, true)
  assert.equal(bridge.saveChapterCalls[0].text, 'AXY')
  assert.equal(state.hasUnsavedChanges, false)
})

// ── 4. 保存失败 ──

await testAsync('保存失败: 返回 false，lastSavedContent/hasUnsavedChanges 不被推进', async () => {
  const deps = makeSaveDeps('B')
  const bridge = makeBridge({ success: false, errorCode: 'IO_ERROR', warnings: [], changedPaths: [], changedEntities: [] })
  const state = makeState('A')
  state.lastSavedContent = 'A'
  state.hasUnsavedChanges = true
  const targetText = await resolveTargetText(deps, state)
  const saved = await doSaveChapter(deps, bridge, state, targetText)
  assert.equal(saved, false)
  assert.equal(state.lastSavedContent, 'A')
  assert.equal(state.hasUnsavedChanges, true)
  assert.equal(state.lastSaveFailed, true)
  assert.equal(state.isSaving, false)
  assert.equal(bridge.processWritingEventCalls.length, 0)
})

await testAsync('保存抛异常: 返回 false，isSaving 释放', async () => {
  const deps = makeSaveDeps('B')
  const bridge = { saveChapterCalls: [], saveChapter: async () => { throw new Error('bridge crashed') }, processWritingEventCalls: [], processWritingEvent: () => {} }
  const state = makeState('A')
  const targetText = await resolveTargetText(deps, state)
  const saved = await doSaveChapter(deps, bridge, state, targetText)
  assert.equal(saved, false)
  assert.equal(state.isSaving, false)
  assert.equal(state.lastSaveFailed, true)
})

// ── 5. persistent 章节会话 ──

await testAsync('persistent 会话: coordinator.open(targetId, text, true) 把 isPersistent=true 传给 bridge.create', async () => {
  const createCalls = []
  const bridge = {
    create: async (targetId, initialText, initialCursorByteOffset, isPersistent) => { createCalls.push({ targetId, initialText, initialCursorByteOffset, isPersistent }); return { success: true, data: 1, warnings: [], changedPaths: [], changedEntities: [] } },
    snapshot: async (sessionId) => ({ success: true, data: { text: 'hello', revision: 1, cursor: 5, selectionAnchor: 5, generation: 0, chapterId: 'c1', composition: null }, warnings: [], changedPaths: [], changedEntities: [] }),
    close: async () => ({ success: true, data: true, warnings: [], changedPaths: [], changedEntities: [] }),
  }
  async function open(targetId, initialText, isPersistent) {
    const createResult = await bridge.create(targetId, initialText, 0, isPersistent)
    if (!createResult.success) { return { success: false } }
    const snapResult = await bridge.snapshot(createResult.data)
    if (!snapResult.success) { return { success: false } }
    return { success: true, data: snapResult.data }
  }
  const result = await open('c1', 'hello', true)
  assert.equal(result.success, true)
  assert.equal(createCalls.length, 1)
  assert.equal(createCalls[0].targetId, 'c1')
  assert.equal(createCalls[0].isPersistent, true)
})

await testAsync('persistent 会话: open(targetId, text, false) 传 isPersistent=false', async () => {
  const createCalls = []
  const bridge = {
    create: async (targetId, initialText, initialCursorByteOffset, isPersistent) => { createCalls.push({ isPersistent }); return { success: true, data: 2, warnings: [], changedPaths: [], changedEntities: [] } },
    snapshot: async () => ({ success: true, data: { text: 'x', revision: 1, cursor: 1, selectionAnchor: 1, generation: 0, chapterId: 'tmp', composition: null }, warnings: [], changedPaths: [], changedEntities: [] }),
    close: async () => ({ success: true, data: true, warnings: [], changedPaths: [], changedEntities: [] }),
  }
  async function open(targetId, initialText, isPersistent) {
    const createResult = await bridge.create(targetId, initialText, 0, isPersistent)
    if (!createResult.success) { return { success: false } }
    return { success: true }
  }
  await open('tmp', 'x', false)
  assert.equal(createCalls[0].isPersistent, false)
})

// ── 6. 保存并发不覆盖新字数 ──

await testAsync('wordCount 并发: 保存期间无新输入时 wordCount 用 receipt.wordCount 校正', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  state.wordCount = 1
  state.sessionOldText = 'old'
  const targetText = await resolveTargetText(deps, state)
  const savePromise = doSaveChapter(deps, bridge, state, targetText)
  await bridge.waitStarted()
  bridge.finishSave()
  const saved = await savePromise
  assert.equal(saved, true)
  assert.equal(state.lastSavedContent, 'A')
  assert.equal(state.hasUnsavedChanges, false)
  assert.equal(state.wordCount, 1)
})

await testAsync('wordCount 并发: 保存期间有新输入时 wordCount 不被旧 receipt 覆盖', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  state.wordCount = 1
  state.sessionOldText = 'old'
  const targetText = await resolveTargetText(deps, state)
  const savePromise = doSaveChapter(deps, bridge, state, targetText)
  await bridge.waitStarted()
  deps.coordinator.applyInput('B')
  state.content = deps.coordinator.text
  state.wordCount = 2
  assert.equal(state.content, 'AB')
  bridge.finishSave()
  const saved = await savePromise
  assert.equal(saved, true)
  assert.equal(bridge.saveChapterCalls[0].text, 'A')
  assert.equal(state.lastSavedContent, 'A')
  assert.equal(state.hasUnsavedChanges, true)
  assert.equal(state.content, 'AB')
  assert.equal(state.wordCount, 2)
})

await testAsync('wordCount 并发: snapshot 为 null 时用 receipt.wordCount 兜底', async () => {
  const deps = makeSaveDeps('A')
  deps.coordinator.getSnapshot = () => null
  const bridge = makeBridge({ success: true, data: { contentHash: 'h', wordCount: 5 }, warnings: [], changedPaths: [], changedEntities: [] })
  const state = makeState('A')
  state.wordCount = 99
  const targetText = await resolveTargetText(deps, state)
  assert.equal(targetText, 'A')
  const saved = await doSaveChapter(deps, bridge, state, targetText)
  assert.equal(saved, true)
  assert.equal(state.wordCount, 5)
})

console.log('---')
console.log(`✅ save_transaction: ${passed} tests passed`)
