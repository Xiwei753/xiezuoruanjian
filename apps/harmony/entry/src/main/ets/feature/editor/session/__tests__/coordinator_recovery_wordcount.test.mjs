// coordinator_recovery_wordcount.test.mjs — Issue #629 评论 5308748920 第 4 项纯逻辑单测。
//
// 验证：patch 恢复成功后实时 wordCount 不漏掉本次已应用的 contentDelta。
//
// 场景：Core 已 Applied 一条编辑命令（contentDelta 有值），但 ArkTS 本地 displayPatch
// 应用失败。Coordinator 从 Core snapshot() 恢复正文后，listener 仍应收到
// { snapshot, editResult: sourceEditResult }（editResult !== null），WritingScreen
// 能继续消费真实 Core contentDelta 实时更新 wordCount。
//
// 对比旧行为（recovery 传 null）：listener 收到 editResult=null，wordCount 不更新，
// 会少算本次已应用的字符。
//
// 本测试是纯逻辑测试（不 import .ets 文件），用 mock 对象模拟 coordinator 的
// enqueueEdit + recoverFromCoreSnapshot + notifyListener 流程，以及 WritingScreen
// 的 applyStateUpdate listener。模拟实现参考 EditorSessionCoordinator.ets 第 153-208 行。
//
// 运行：node coordinator_recovery_wordcount.test.mjs

import { strict as assert } from 'node:assert'

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

// ── DTO 构造工具 ──

const APPLIED = 'applied'
const STALE_REVISION = 'staleRevision'

function makeEditResult(overrides) {
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
    composition: null,
}
  return { ...base, ...overrides }
}

function makeSnapshot(overrides) {
const base = {
    text: '',
    revision: 1,
    cursor: 0,
    selectionAnchor: 0,
    generation: 1,
    chapterId: 'c1',
    composition: null,
}
  return { ...base, ...overrides }
}

// ── mock: bridge ──
class MockBridge {
  constructor() {
    this.insertResult = null
    this.deleteResult = null
    this.snapshotResult = null
    this.snapshotCalls = []
}
  async insert(sessionId, byteOffset, text, cause, rev) {
    return this.insertResult
}
  async delete(sessionId, start, end, cause, rev) {
    return this.deleteResult
}
  async snapshot(sessionId) {
    this.snapshotCalls.push(sessionId)
    return this.snapshotResult
}
}

// ── mock: 编辑会话 state ──
function makeState(snapshot, opts = {}) {
  return {
    sessionId: 's1',
    snapshot: snapshot,
    patchApplyFail: opts.patchApplyFail || false,
    withEditResult(editResult) {
      if (this.patchApplyFail) {
        return { ok: false, state: this, reason: 'patchFailed:nonCharBoundary' }
}
const newSnap = makeSnapshot({
        ...this.snapshot,
        revision: editResult.newRevision,
})
      this.snapshot = newSnap
      return { ok: true, state: this }
},
}
}

// ── 模拟 enqueueEdit 核心逻辑 ──
// 参考 EditorSessionCoordinator.ets 第 153-179 行
async function enqueueEdit(state, bridge, listener, bridgeCall) {
  if (!state) {
    return { success: false, errorCode: 'NO_SESSION', warnings: [], changedPaths: [], changedEntities: [] }
}
const sessionId = state.sessionId
const expectedRevision = state.snapshot.revision
const result = await bridgeCall(sessionId, expectedRevision)
  if (result.success && result.data) {
const outcome = state.withEditResult(result.data)
    if (outcome.ok) {
      listener.notify({ snapshot: state.snapshot, editResult: result.data })
} else {
      // patch 失败 → recovery 传 result.data（sourceEditResult）
await recoverFromCoreSnapshot(state, bridge, listener, result.data)
}
}
  return result
}

// ── 模拟 recoverFromCoreSnapshot 核心逻辑 ──
// 参考 EditorSessionCoordinator.ets 第 196-208 行
async function recoverFromCoreSnapshot(state, bridge, listener, sourceEditResult = null) {
  if (!state) {
    return
}
const sid = state.sessionId
const snapResult = await bridge.snapshot(sid)
  if (snapResult.success && snapResult.data) {
    state.snapshot = snapResult.data
    listener.notify({ snapshot: state.snapshot, editResult: sourceEditResult })
}
}

// ── 模拟 WritingScreen applyStateUpdate listener ──
// 参考 writing_screen_clear.test.mjs 第 154-161 行
function makeScreenListener(screenState) {
  return {
    notify(update) {
      screenState.content = update.snapshot.text
      screenState.hasUnsavedChanges = update.snapshot.text !== screenState.lastSavedContent
      if (update.editResult !== null) {
const delta = update.editResult.contentDelta
        screenState.wordCount = Math.max(
          0,
          screenState.wordCount + delta.insertedNonWhitespaceChars - delta.deletedNonWhitespaceChars,
        )
}
      screenState.lastUpdate = update
},
}
}

console.log('coordinator_recovery_wordcount 纯逻辑单测（评论 5308748920 第 4 项）')
console.log('patch 恢复后 listener 收到 sourceEditResult → wordCount 不漏 contentDelta')
console.log('---')

// ── 场景 1：Core Applied + patch 失败 → recovery 传 sourceEditResult → wordCount 不漏 ──
await testAsync('场景1: Core Applied + patch 失败 → recovery 传 sourceEditResult → wordCount 不漏', async () => {
const initialSnap = makeSnapshot({ text: 'hello', revision: 1, cursor: 5, selectionAnchor: 5 })
const state = makeState(initialSnap, { patchApplyFail: true })
  
const bridge = new MockBridge()
    bridge.insertResult = {
      success: true,
      data: makeEditResult({
        outcome: APPLIED,
        newRevision: 2,
        contentDelta: { insertedChars: 1, deletedChars: 0, insertedNonWhitespaceChars: 1, deletedNonWhitespaceChars: 0 },
}),
      warnings: [], changedPaths: [], changedEntities: [],
}
    bridge.snapshotResult = {
      success: true,
      data: makeSnapshot({ text: 'hellox', revision: 2, cursor: 6, selectionAnchor: 6 }),
      warnings: [], changedPaths: [], changedEntities: [],
}
  
const screenState = { content: 'hello', wordCount: 5, lastSavedContent: 'hello', hasUnsavedChanges: false }
const listener = makeScreenListener(screenState)
  
await enqueueEdit(state, bridge, listener, (sid, rev) => bridge.insert(sid, 0, 'x', 'Typing', rev))
  
assert.notEqual(screenState.lastUpdate.editResult, null)
assert.equal(screenState.lastUpdate.editResult.outcome, APPLIED)
assert.equal(screenState.wordCount, 6)
assert.equal(screenState.content, 'hellox')
assert.equal(bridge.snapshotCalls.length, 1)
})
  
  // ── 场景 2：对比旧行为 — recovery 传 null → wordCount 漏掉 contentDelta ──
await testAsync('场景2(对比旧行为): recovery 传 null → wordCount 漏掉 contentDelta', async () => {
    // 新行为
const initialSnap = makeSnapshot({ text: 'hello', revision: 1, cursor: 5, selectionAnchor: 5 })
const state = makeState(initialSnap, { patchApplyFail: true })
const bridge = new MockBridge()
    bridge.insertResult = {
      success: true,
      data: makeEditResult({
        outcome: APPLIED,
        newRevision: 2,
        contentDelta: { insertedChars: 1, deletedChars: 0, insertedNonWhitespaceChars: 1, deletedNonWhitespaceChars: 0 },
}),
      warnings: [], changedPaths: [], changedEntities: [],
}
    bridge.snapshotResult = {
      success: true,
      data: makeSnapshot({ text: 'hellox', revision: 2, cursor: 6, selectionAnchor: 6 }),
      warnings: [], changedPaths: [], changedEntities: [],
}
const screenStateNew = { content: 'hello', wordCount: 5, lastSavedContent: 'hello', hasUnsavedChanges: false }
const listenerNew = makeScreenListener(screenStateNew)
await enqueueEdit(state, bridge, listenerNew, (sid, rev) => bridge.insert(sid, 0, 'x', 'Typing', rev))
  
    // 旧行为：recovery 传 null
const screenStateOld = { content: 'hello', wordCount: 5, lastSavedContent: 'hello', hasUnsavedChanges: false }
const listenerOld = makeScreenListener(screenStateOld)
    listenerOld.notify({ snapshot: makeSnapshot({ text: 'hellox', revision: 2 }), editResult: null })
  
    // 新行为 wordCount=6，旧行为 wordCount=5
assert.equal(screenStateNew.wordCount, 6)
assert.equal(screenStateOld.wordCount, 5)
assert.notEqual(screenStateNew.lastUpdate.editResult, null)
assert.equal(screenStateOld.lastUpdate.editResult, null)
})
  
  // ── 场景 3：stale/invalid result → contentDelta 全 0 → wordCount 不变 ──
await testAsync('场景3: stale/invalid result → contentDelta 全 0 → wordCount 不变', async () => {
const initialSnap = makeSnapshot({ text: 'hello', revision: 1, cursor: 5, selectionAnchor: 5 })
const state = makeState(initialSnap, { patchApplyFail: true })
  
const bridge = new MockBridge()
    bridge.insertResult = {
      success: true,
      data: makeEditResult({
        outcome: STALE_REVISION,
        newRevision: 1,
        contentDelta: { insertedChars: 0, deletedChars: 0, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 0 },
}),
      warnings: [], changedPaths: [], changedEntities: [],
}
    bridge.snapshotResult = {
      success: true,
      data: makeSnapshot({ text: 'hello', revision: 1, cursor: 5, selectionAnchor: 5 }),
      warnings: [], changedPaths: [], changedEntities: [],
}
  
const screenState = { content: 'hello', wordCount: 5, lastSavedContent: 'hello', hasUnsavedChanges: false }
const listener = makeScreenListener(screenState)
  
await enqueueEdit(state, bridge, listener, (sid, rev) => bridge.insert(sid, 0, 'x', 'Typing', rev))
  
assert.equal(screenState.wordCount, 5)
assert.equal(screenState.content, 'hello')
})
  
  // ── 场景 4：删除字符的 recovery + contentDelta ──
await testAsync('场景4: 删除字符 Core Applied + patch 失败 → recovery → wordCount 递减', async () => {
const initialSnap = makeSnapshot({ text: 'helloWorld', revision: 3, cursor: 5, selectionAnchor: 5 })
const state = makeState(initialSnap, { patchApplyFail: true })
  
const bridge = new MockBridge()
    bridge.deleteResult = {
      success: true,
      data: makeEditResult({
        outcome: APPLIED,
        newRevision: 4,
        contentDelta: { insertedChars: 0, deletedChars: 3, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 3 },
}),
      warnings: [], changedPaths: [], changedEntities: [],
}
    bridge.snapshotResult = {
      success: true,
      data: makeSnapshot({ text: 'hello', revision: 4, cursor: 5, selectionAnchor: 5 }),
      warnings: [], changedPaths: [], changedEntities: [],
}
  
const screenState = { content: 'helloWorld', wordCount: 10, lastSavedContent: 'helloWorld', hasUnsavedChanges: false }
const listener = makeScreenListener(screenState)
  
await enqueueEdit(state, bridge, listener, (sid, rev) => bridge.delete(sid, 5, 8, 'Typing', rev))
  
assert.notEqual(screenState.lastUpdate.editResult, null)
assert.equal(screenState.wordCount, 7)
assert.equal(screenState.content, 'hello')
})
  
  // ── 场景 5：recovery snapshot 失败 → listener 不被调用，wordCount 不变 ──
await testAsync('场景5: recovery snapshot 失败 → listener 不被调用 → wordCount 不变', async () => {
const initialSnap = makeSnapshot({ text: 'hello', revision: 1, cursor: 5, selectionAnchor: 5 })
const state = makeState(initialSnap, { patchApplyFail: true })
  
const bridge = new MockBridge()
    bridge.insertResult = {
      success: true,
      data: makeEditResult({
        outcome: APPLIED,
        newRevision: 2,
        contentDelta: { insertedChars: 1, deletedChars: 0, insertedNonWhitespaceChars: 1, deletedNonWhitespaceChars: 0 },
}),
      warnings: [], changedPaths: [], changedEntities: [],
}
    bridge.snapshotResult = {
      success: false,
      errorCode: 'SESSION_CLOSED',
      warnings: [], changedPaths: [], changedEntities: [],
}
  
const screenState = { content: 'hello', wordCount: 5, lastSavedContent: 'hello', hasUnsavedChanges: false }
const listener = makeScreenListener(screenState)
  
await enqueueEdit(state, bridge, listener, (sid, rev) => bridge.insert(sid, 0, 'x', 'Typing', rev))
  
assert.equal(screenState.lastUpdate, undefined)
assert.equal(screenState.wordCount, 5)
assert.equal(bridge.snapshotCalls.length, 1)
})
  
  // ── 场景 6：正常应用（patch 成功）→ 不走 recovery ──
await testAsync('场景6(对照): patch 成功 → 不走 recovery → listener 直接收到 editResult', async () => {
const initialSnap = makeSnapshot({ text: 'hello', revision: 1, cursor: 5, selectionAnchor: 5 })
const state = makeState(initialSnap, { patchApplyFail: false })
  
const bridge = new MockBridge()
    bridge.insertResult = {
      success: true,
      data: makeEditResult({
        outcome: APPLIED,
        newRevision: 2,
        contentDelta: { insertedChars: 1, deletedChars: 0, insertedNonWhitespaceChars: 1, deletedNonWhitespaceChars: 0 },
}),
      warnings: [], changedPaths: [], changedEntities: [],
}
  
const screenState = { content: 'hello', wordCount: 5, lastSavedContent: 'hello', hasUnsavedChanges: false }
const listener = makeScreenListener(screenState)
  
await enqueueEdit(state, bridge, listener, (sid, rev) => bridge.insert(sid, 0, 'x', 'Typing', rev))
  
assert.notEqual(screenState.lastUpdate.editResult, null)
assert.equal(screenState.wordCount, 6)
assert.equal(bridge.snapshotCalls.length, 0)
})
  
  // ── 场景 7：中文 contentDelta recovery ──
await testAsync('场景7: 中文插入 Core Applied + patch 失败 → recovery → wordCount 按非空白字符更新', async () => {
const initialSnap = makeSnapshot({ text: '你好', revision: 1, cursor: 2, selectionAnchor: 2 })
const state = makeState(initialSnap, { patchApplyFail: true })
  
const bridge = new MockBridge()
    bridge.insertResult = {
      success: true,
      data: makeEditResult({
        outcome: APPLIED,
        newRevision: 2,
        contentDelta: { insertedChars: 2, deletedChars: 0, insertedNonWhitespaceChars: 2, deletedNonWhitespaceChars: 0 },
}),
      warnings: [], changedPaths: [], changedEntities: [],
}
    bridge.snapshotResult = {
      success: true,
      data: makeSnapshot({ text: '你好世界', revision: 2, cursor: 4, selectionAnchor: 4 }),
      warnings: [], changedPaths: [], changedEntities: [],
}
  
const screenState = { content: '你好', wordCount: 2, lastSavedContent: '你好', hasUnsavedChanges: false }
const listener = makeScreenListener(screenState)
  
await enqueueEdit(state, bridge, listener, (sid, rev) => bridge.insert(sid, 6, '世界', 'Typing', rev))
  
assert.notEqual(screenState.lastUpdate.editResult, null)
assert.equal(screenState.wordCount, 4)
assert.equal(screenState.content, '你好世界')
})
  
  // ── 场景 8：emoji contentDelta recovery ──
await testAsync('场景8: emoji 插入 Core Applied + patch 失败 → recovery → wordCount 按非空白字符更新', async () => {
const initialSnap = makeSnapshot({ text: 'a', revision: 1, cursor: 1, selectionAnchor: 1 })
const state = makeState(initialSnap, { patchApplyFail: true })
  
const bridge = new MockBridge()
    bridge.insertResult = {
      success: true,
      data: makeEditResult({
        outcome: APPLIED,
        newRevision: 2,
        contentDelta: { insertedChars: 1, deletedChars: 0, insertedNonWhitespaceChars: 1, deletedNonWhitespaceChars: 0 },
}),
      warnings: [], changedPaths: [], changedEntities: [],
}
    bridge.snapshotResult = {
      success: true,
      data: makeSnapshot({ text: 'a😀', revision: 2, cursor: 2, selectionAnchor: 2 }),
      warnings: [], changedPaths: [], changedEntities: [],
}
  
const screenState = { content: 'a', wordCount: 1, lastSavedContent: 'a', hasUnsavedChanges: false }
const listener = makeScreenListener(screenState)
  
await enqueueEdit(state, bridge, listener, (sid, rev) => bridge.insert(sid, 1, '😀', 'Typing', rev))
  
assert.notEqual(screenState.lastUpdate.editResult, null)
assert.equal(screenState.wordCount, 2)
assert.equal(screenState.content, 'a😀')
})
  
console.log('---')
console.log(`✅ coordinator_recovery_wordcount: ${passed} tests passed`)
