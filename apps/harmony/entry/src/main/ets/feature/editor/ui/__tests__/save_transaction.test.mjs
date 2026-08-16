// save_transaction.test.mjs — Issue #629 评论8 第1项 保存事务行为测试。
//
// 验证：
//   1. 保存期间继续输入：savedText 是保存开始时读一次的不可变快照；保存返回后按 savedText
//      结算 lastSavedContent/sessionOldText；再取 latestSnapshot 结算 hasUnsavedChanges
//      （保存期间的新输入保持 unsaved，不把 A+B 标成已保存）。
//   2. 保存事务复用：保存进行中并发调用 saveChapter 复用同一任务（bridge.saveChapter 只调一次）。
//   3. 保存事务顺序：flush → whenIdle → snapshot(savedText) → bridge.saveChapter(savedText)。
//   4. 保存失败：返回 false，lastSavedContent/hasUnsavedChanges 不被错误推进。
//   5. persistent 章节会话：coordinator.open(..., true) 把 isPersistent 显式传给 bridge.create。
//
// 运行：node --experimental-strip-types save_transaction.test.mjs
//
// 注意：.ets 依赖 ArkUI 无法用 Node 直接测；本测试提取 WritingScreen 保存事务的编排逻辑
// （doSaveChapter/saveChapter），与生产代码对齐。生产代码需 HarmonyOS SDK 端到端编译。

import { strict as assert } from 'node:assert'
import { SerialCommandQueue } from '../../session/editor_patch_logic.ts'

let passed = 0
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}
const sleep = (ms) => new Promise(r => setTimeout(r, ms))

console.log('save_transaction 保存事务行为测试（Issue #629 评论8 第1项）')
console.log('---')

// ── 工具：与 WritingScreen.doSaveChapter 对齐的保存事务镜像 ──

// 构造保存事务所需依赖。
// dispatcher：真实 SerialCommandQueue（flush 语义）；coordinator：可控 snapshot + whenIdle。
function makeSaveDeps(initialSnapshotText) {
  const dispatcherQueue = new SerialCommandQueue()
  let snapshotText = initialSnapshotText
  const coordinator = {
    // 真实串行队列：模拟 dispatcher flush 期间排入的输入
    queue: new SerialCommandQueue(),
    async whenIdle() {
      return this.queue.whenIdle()
    },
    getSnapshot() {
      return { text: snapshotText, revision: 0, cursor: 0, selectionAnchor: 0, generation: 0, chapterId: 'c1', composition: null }
    },
    // 模拟编辑输入：直接改 snapshot（保存期间的新输入）
    applyInput(text) {
      snapshotText = snapshotText + text
    },
    get text() { return snapshotText },
  }
  const dispatcher = {
    async flush() {
      return dispatcherQueue.whenIdle()
    },
    queue: dispatcherQueue,
  }
  return { dispatcher, coordinator }
}

// 与 WritingScreen.doSaveChapter 对齐的保存事务镜像。
// 入参：deps{dispatcher, coordinator}、bridge{saveChapter}、state{content, lastSavedContent,
// sessionOldText, settings, projectId, volumeId, chapterId, sessionId, sessionStartTime, updateContinuationState}
async function doSaveChapter(deps, bridge, state, isAutoSave = false) {
  state.isSaving = true
  state.isAutoSaving = isAutoSave
  try {
    // 1+2. 稳定 committed snapshot：flush 输入 + 等 Core 命令队列空闲
    await deps.dispatcher.flush()
    await deps.coordinator.whenIdle()
    // 3. 不可变 savedText：只读一次 Core snapshot
    const snap = deps.coordinator.getSnapshot()
    const savedText = snap ? snap.text : state.content
    // 4. 只保存这份文本
    const result = await bridge.saveChapter(state.chapterId, savedText)
    if (result.success && result.data) {
      // 5. 按 savedText 结算统计
      if (state.sessionOldText !== savedText) {
        bridge.processWritingEvent(state.sessionOldText, savedText)
        state.sessionOldText = savedText
      }
      state.lastSavedContent = savedText
      state.lastSavedContentHash = result.data.contentHash
      state.hasUnsavedChanges = false
      state.lastSaveFailed = false
      state.wordCount = result.data.wordCount
      // 6. 重新取 latestSnapshot 结算
      const latest = deps.coordinator.getSnapshot()
      if (latest) {
        state.content = latest.text
        state.hasUnsavedChanges = latest.text !== savedText
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

// 与 WritingScreen.saveChapter 对齐：saveTask 复用同一任务。
function makeSaveChapter(deps, bridge, state) {
  let saveTask = null
  const saveChapter = (isAutoSave = false) => {
    if (saveTask !== null) {
      return saveTask
    }
    const task = doSaveChapter(deps, bridge, state, isAutoSave)
    saveTask = task
    task.then(
      () => { if (saveTask === task) { saveTask = null } },
      () => { if (saveTask === task) { saveTask = null } }
    )
    return task
  }
  return { saveChapter, getSaveTask: () => saveTask }
}

function makeBridge(saveResult) {
  const bridge = {
    saveChapterCalls: [],
    saveChapter: async (chapterId, text) => {
      bridge.saveChapterCalls.push({ chapterId, text })
      return saveResult
    },
    processWritingEventCalls: [],
    processWritingEvent: (oldText, newText) => { bridge.processWritingEventCalls.push({ oldText, newText }) },
  }
  return bridge
}

// 可延迟 resolve 的保存 bridge：用于制造"保存进行中"窗口。
// - waitStarted()：等第一次 saveChapter 真正挂起（此时 savedText 已冻结）。
// - finishSave()：放行当前挂起的保存。调用方需先确保保存已挂起（await waitStarted()
//   或 await sleep(0) —— doSaveChapter 在调 bridge.saveChapter 前只有微任务，
//   一个 macrotask 后必然已挂起）。
function makeDeferredBridge() {
  const bridge = {
    saveChapterCalls: [],
    processWritingEventCalls: [],
    processWritingEvent: (oldText, newText) => { bridge.processWritingEventCalls.push({ oldText, newText }) },
  }
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
    content,
    lastSavedContent: content,
    lastSavedContentHash: 'h-old',
    sessionOldText: content,
    hasUnsavedChanges: false,
    lastSaveFailed: false,
    isSaving: false,
    isAutoSaving: false,
    wordCount: 0,
    chapterId: 'c1',
    projectId: 'p1',
    volumeId: 'v1',
    sessionId: 's1',
    sessionStartTime: Date.now(),
    settings: { statsDeviceId: 'dev1' },
    updateContinuationState: () => {},
  }
}

// ── 1. 保存期间继续输入：savedText 冻结，结算按 savedText，新输入保持 unsaved ──

await testAsync('保存期间继续输入: 磁盘保存 A，保存期间输入 B，结算后 hasUnsavedChanges=true 且 content=A+B', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  state.sessionOldText = 'old-before-A'  // 有统计差异：savedText 结算时会上报一次
  const savePromise = doSaveChapter(deps, bridge, state)
  // 等保存真正挂起：savedText 已冻结为 'A'，bridge.saveChapter 等待中
  await bridge.waitStarted()

  // 保存期间用户继续输入 B（state listener 会更新 content，但 savedText 已冻结为 'A'）
  deps.coordinator.applyInput('B')
  state.content = deps.coordinator.text  // listener 回流
  assert.equal(state.content, 'AB')

  // 保存完成（当前保存已挂起，直接放行）
  bridge.finishSave()
  const saved = await savePromise

  assert.equal(saved, true)
  // 关键断言 1：bridge 只保存 savedText='A'（不是 A+B）
  assert.equal(bridge.saveChapterCalls.length, 1)
  assert.equal(bridge.saveChapterCalls[0].text, 'A', '磁盘保存的是保存开始时冻结的 savedText')
  // 关键断言 2：lastSavedContent 按 savedText 结算，不是 content
  assert.equal(state.lastSavedContent, 'A', 'lastSavedContent 按 savedText 结算')
  // 关键断言 3：contentHash 是这次 savedText 的回执
  assert.equal(state.lastSavedContentHash, 'h-A')
  // 关键断言 4：新输入 B 保持 unsaved（不把 A+B 标成已保存）
  assert.equal(state.hasUnsavedChanges, true, '保存期间的新输入必须保持 hasUnsavedChanges=true')
  assert.equal(state.content, 'AB', 'content 是 latestSnapshot 的 A+B')
  // 统计按 savedText 结算
  assert.deepEqual(state.sessionOldText, 'A')
  assert.equal(bridge.processWritingEventCalls.length, 1)
  assert.equal(bridge.processWritingEventCalls[0].newText, 'A', 'processWritingEvent 按 savedText 结算')
})

await testAsync('保存期间无新输入: 结算后 hasUnsavedChanges=false', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  state.sessionOldText = 'old'  // 有统计差异

  const savePromise = doSaveChapter(deps, bridge, state)
  await bridge.waitStarted()  // 保存已挂起
  bridge.finishSave()
  const saved = await savePromise

  assert.equal(saved, true)
  assert.equal(state.lastSavedContent, 'A')
  assert.equal(state.hasUnsavedChanges, false, '无新输入时保存后为已保存状态')
  assert.equal(state.content, 'A')
})

// ── 2. 保存事务复用：并发调用共享同一任务 ──

await testAsync('保存事务复用: 保存进行中并发调用 saveChapter 复用同一任务（bridge 只调一次）', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  const { saveChapter, getSaveTask } = makeSaveChapter(deps, bridge, state)

  // 并发三次调用（不 await）
  const p1 = saveChapter()
  const p2 = saveChapter()
  const p3 = saveChapter()
  // 同一任务（同一 Promise 对象）
  assert.equal(p1, p2, '并发调用返回同一 Promise')
  assert.equal(p2, p3, '并发调用返回同一 Promise')
  assert.notEqual(getSaveTask(), null)

  await bridge.waitStarted()  // 保存已挂起
  bridge.finishSave()
  const results = await Promise.all([p1, p2, p3])
  assert.deepEqual(results, [true, true, true])
  // bridge.saveChapter 只调一次（复用，不重复保存）
  assert.equal(bridge.saveChapterCalls.length, 1)
  // 事务完成后释放，允许下次新保存
  await sleep(0)
  assert.equal(getSaveTask(), null, '事务完成后 saveTask 释放')
})

await testAsync('保存事务复用: 第一次完成后再次保存是新任务（允许重新保存）', async () => {
  const deps = makeSaveDeps('A')
  const bridge = makeDeferredBridge()
  const state = makeState('A')
  const { saveChapter, getSaveTask } = makeSaveChapter(deps, bridge, state)

  const p1 = saveChapter()
  await bridge.waitStarted()  // 第一次保存已挂起
  bridge.finishSave()
  await p1
  await sleep(0)
  assert.equal(getSaveTask(), null)

  // 再次保存：新任务（不同 Promise 对象），bridge 调两次
  const p2 = saveChapter()
  assert.notEqual(p1, p2, '完成后再次保存是新任务')
  await sleep(0)  // 第二次保存已挂起
  bridge.finishSave()
  await p2
  assert.equal(bridge.saveChapterCalls.length, 2)
})

// ── 3. 保存事务顺序：flush → whenIdle → snapshot → persist ──

await testAsync('保存事务顺序: flush 等完排队输入后才读 snapshot 保存（不丢最后几个字）', async () => {
  const deps = makeSaveDeps('A')
  // 保存前 dispatcher 队列里还有最后几个字（模拟快速打字后立即点保存）
  deps.dispatcher.queue.enqueue(async () => {
    await sleep(10)
    deps.coordinator.applyInput('X')
  })
  deps.dispatcher.queue.enqueue(async () => {
    await sleep(10)
    deps.coordinator.applyInput('Y')
  })
  const bridge = makeBridge({ success: true, data: { contentHash: 'h-AXY', wordCount: 3 }, warnings: [], changedPaths: [], changedEntities: [] })
  const state = makeState('A')

  const saved = await doSaveChapter(deps, bridge, state)

  assert.equal(saved, true)
  // flush 后 snapshot 已含排队输入：保存的是 AXY
  assert.equal(bridge.saveChapterCalls[0].text, 'AXY', 'flush 等完排队输入后才取 snapshot')
  assert.equal(state.hasUnsavedChanges, false)
})

// ── 4. 保存失败：返回 false，状态不被错误推进 ──

await testAsync('保存失败: 返回 false，lastSavedContent/hasUnsavedChanges 不被推进', async () => {
  const deps = makeSaveDeps('B')
  const bridge = makeBridge({ success: false, errorCode: 'IO_ERROR', warnings: [], changedPaths: [], changedEntities: [] })
  const state = makeState('A')
  state.lastSavedContent = 'A'
  state.hasUnsavedChanges = true

  const saved = await doSaveChapter(deps, bridge, state)

  assert.equal(saved, false)
  assert.equal(state.lastSavedContent, 'A', '保存失败不推进 lastSavedContent')
  assert.equal(state.hasUnsavedChanges, true, '保存失败保持 unsaved（正文仍在活跃会话）')
  assert.equal(state.lastSaveFailed, true)
  assert.equal(state.isSaving, false, 'finally 释放 isSaving')
  assert.equal(bridge.processWritingEventCalls.length, 0, '保存失败不上报统计')
})

await testAsync('保存抛异常: 返回 false，isSaving 释放', async () => {
  const deps = makeSaveDeps('B')
  const bridge = {
    saveChapterCalls: [],
    saveChapter: async () => { throw new Error('bridge crashed') },
    processWritingEventCalls: [],
    processWritingEvent: () => {},
  }
  const state = makeState('A')

  const saved = await doSaveChapter(deps, bridge, state)

  assert.equal(saved, false)
  assert.equal(state.isSaving, false)
  assert.equal(state.lastSaveFailed, true)
})

// ── 5. persistent 章节会话：open(..., true) 显式传 bridge ──

await testAsync('persistent 会话: coordinator.open(targetId, text, true) 把 isPersistent=true 传给 bridge.create', async () => {
  const createCalls = []
  const bridge = {
    create: async (targetId, initialText, initialCursorByteOffset, isPersistent) => {
      createCalls.push({ targetId, initialText, initialCursorByteOffset, isPersistent })
      return { success: true, data: 1, warnings: [], changedPaths: [], changedEntities: [] }
    },
    snapshot: async (sessionId) => ({
      success: true,
      data: { text: 'hello', revision: 1, cursor: 5, selectionAnchor: 5, generation: 0, chapterId: 'c1', composition: null },
      warnings: [], changedPaths: [], changedEntities: [],
    }),
    close: async () => ({ success: true, data: true, warnings: [], changedPaths: [], changedEntities: [] }),
  }
  // 镜像 EditorSessionCoordinator.open
  async function open(targetId, initialText, isPersistent) {
    const createResult = await bridge.create(targetId, initialText, 0, isPersistent)
    if (!createResult.success) {
      return { success: false }
    }
    const snapResult = await bridge.snapshot(createResult.data)
    if (!snapResult.success) {
      return { success: false }
    }
    return { success: true, data: snapResult.data }
  }

  const result = await open('c1', 'hello', true)
  assert.equal(result.success, true)
  assert.equal(createCalls.length, 1)
  assert.equal(createCalls[0].targetId, 'c1')
  assert.equal(createCalls[0].isPersistent, true, '章节正文必须传 isPersistent=true（persistent session）')
})

await testAsync('persistent 会话: open(targetId, text, false) 传 isPersistent=false（短文本草稿）', async () => {
  const createCalls = []
  const bridge = {
    create: async (targetId, initialText, initialCursorByteOffset, isPersistent) => {
      createCalls.push({ isPersistent })
      return { success: true, data: 2, warnings: [], changedPaths: [], changedEntities: [] }
    },
    snapshot: async () => ({
      success: true,
      data: { text: 'x', revision: 1, cursor: 1, selectionAnchor: 1, generation: 0, chapterId: 'tmp', composition: null },
      warnings: [], changedPaths: [], changedEntities: [],
    }),
    close: async () => ({ success: true, data: true, warnings: [], changedPaths: [], changedEntities: [] }),
  }
  async function open(targetId, initialText, isPersistent) {
    const createResult = await bridge.create(targetId, initialText, 0, isPersistent)
    if (!createResult.success) {
      return { success: false }
    }
    return { success: true }
  }

  await open('tmp', 'x', false)
  assert.equal(createCalls[0].isPersistent, false, '短文本草稿会话传 isPersistent=false')
})

console.log('---')
console.log(`✅ save_transaction: ${passed} tests passed`)
