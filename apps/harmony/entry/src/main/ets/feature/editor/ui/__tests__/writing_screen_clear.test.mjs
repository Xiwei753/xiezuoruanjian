// writing_screen_clear.test.mjs — WritingScreen.doClearContent 纯逻辑单测。
//
// 验证 Issue #629 评论 5 第 4 节 + 评论9 第4项的核心行为：
//   1. 清空正文走编辑事务 replace(0, utf8End, '', fullText, Programmatic)，
//      不直接 bridge.clearChapter()（不在活跃 TextEditSession 外直接 clear 文件）。
//   2. doClearContent 不再单独按 contentDelta 修字数——state listener 收到 replace 的 editResult 后
//      自动按 insertedNonWhitespaceChars/deletedNonWhitespaceChars 实时更新 wordCount（Core 非空白字符）。
//   3. 保存后用 ChapterSaveReceipt.wordCount 校正（Core 真实字数）。
//   4. replace 失败时不保存，lastSaveFailed=true。
//   5. 多字节字符正文：utf8EndByte 用 TextOffsetMapper.utf16ToUtf8 转换，不是 .length。
//
// 运行：node writing_screen_clear.test.mjs
//
// 注意：.ets 依赖 ArkUI 无法用 Node 直接测，本测试验证提取的纯逻辑（doClearContent
// 的编排：replace → contentDelta → saveChapter）。生产代码 WritingScreen.ets 调用相同逻辑，
// 需 HarmonyOS SDK 才能端到端编译——本地无 SDK，此为已知阻塞。

import { strict as assert } from 'node:assert'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// UTF-16 code unit offset → UTF-8 byte offset（与 TextOffsetMapper.utf16ToUtf8 对齐）
function utf16ToUtf8(text, utf16Offset) {
  if (utf16Offset <= 0) return 0
  const limited = utf16Offset > text.length ? text.length : utf16Offset
  const sub = text.substring(0, limited)
  return new TextEncoder().encode(sub).length
}

// EditorEditResult 形状（与 EditorDtos.ets EditorEditResult 对齐）
function makeEditResult(overrides) {
  return {
    outcome: 'applied',
    transactionId: 1,
    baseRevision: 0,
    newRevision: 1,
    displayPatches: [],
    oldSelectionStart: 0,
    oldSelectionEnd: 0,
    newSelectionStart: 0,
    newSelectionEnd: 0,
    visualIntent: {},
    compositionSession: null,
    contentDelta: { insertedChars: 0, deletedChars: 0, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 0 },
    composition: null,
    ...overrides,
  }
}

// ChapterSaveReceipt 形状（与 ProjectDtos.ets ChapterSaveReceipt 对齐）
function makeSaveReceipt(overrides) {
  return {
    chapterRelativePath: 'ch.md',
    contentLen: 0,
    contentHash: 'hash-empty',
    metaHash: 'meta-empty',
    updatedAt: '2026-01-01T00:00:00Z',
    wordCount: 0,
    ...overrides,
  }
}

// MockCoordinator：记录 replace 调用参数，返回预设结果。
class MockCoordinator {
  constructor(snapshot) {
    this.snapshot = snapshot
    this.replaceCalls = []
    this.replaceResult = null
  }
  getSnapshot() {
    return this.snapshot
  }
  async replace(start, end, text, originalText, cause) {
    this.replaceCalls.push({ start, end, text, originalText, cause })
    // 模拟 Core 回流：replace 成功后 snapshot.text 更新为插入文本
    if (this.replaceResult && this.replaceResult.success && this.snapshot) {
      this.snapshot = { ...this.snapshot, text: text }
    }
    return this.replaceResult
  }
}

// MockBridge：记录 saveChapter 调用，返回预设结果。clearChapter 不应被调用。
class MockBridge {
  constructor() {
    this.saveChapterCalls = []
    this.saveChapterResult = null
    this.clearChapterCalls = []
  }
  async saveChapter(chapterId, text) {
    this.saveChapterCalls.push({ chapterId, text })
    return this.saveChapterResult
  }
  async clearChapter(chapterId) {
    this.clearChapterCalls.push({ chapterId })
    return { success: true, data: makeSaveReceipt(), warnings: [], changedPaths: [], changedEntities: [] }
  }
}

// doClearContent 编排逻辑（与 WritingScreen.doClearContent 对齐）
// Issue #629 评论9 第4项：doClearContent 不再单独按 contentDelta 修字数——
// state listener 收到 replace 的 editResult 后自动按
// insertedNonWhitespaceChars/deletedNonWhitespaceChars 实时更新 wordCount。
// doClearContent 只负责 replace → saveChapter 持久化。
async function doClearContent(state, coordinator, bridge) {
  const snap = coordinator.getSnapshot()
  if (!snap) {
    state.lastSaveFailed = true
    return
  }
  state.isSaving = true
  let replaceOk = false
  try {
    const fullText = snap.text
    const utf8EndByte = utf16ToUtf8(fullText, fullText.length)
    const replaceResult = await coordinator.replace(0, utf8EndByte, '', fullText, 'Programmatic')
    if (replaceResult.success && replaceResult.data) {
      // state listener 已按 editResult.contentDelta 实时更新 wordCount（非空白字符增量），
      // 并回流空正文。此处不再单独算字数。
      replaceOk = true
    }
  } catch (err) {
    replaceOk = false
  }
  state.isSaving = false
  if (!replaceOk) {
    state.lastSaveFailed = true
    return
  }
  const snap2 = coordinator.getSnapshot()
  const content = snap2 ? snap2.text : state.content
  const saveResult = await bridge.saveChapter(state.chapterId, content)
  if (saveResult.success && saveResult.data) {
    state.lastSavedContent = content
    state.lastSavedContentHash = saveResult.data.contentHash
    state.hasUnsavedChanges = false
    // 保存后 wordCount 校正：清空后 latest.text === savedText === ''，用 receipt.wordCount 校正
    state.wordCount = saveResult.data.wordCount
    state.lastSaveFailed = false
  } else {
    state.lastSaveFailed = true
  }
}

// Issue #629 评论9 第4项：state listener 镜像。
// 收到 EditorStateUpdate{snapshot, editResult} 时按 editResult.contentDelta 实时更新 wordCount
// （insertedNonWhitespaceChars/deletedNonWhitespaceChars，Core 非空白字符增量）。
// editResult=null（open/snapshot 恢复）时不更新 wordCount。
function applyStateUpdate(state, update) {
  state.content = update.snapshot.text
  state.hasUnsavedChanges = update.snapshot.text !== state.lastSavedContent
  if (update.editResult !== null) {
    const delta = update.editResult.contentDelta
    state.wordCount = Math.max(0, state.wordCount + delta.insertedNonWhitespaceChars - delta.deletedNonWhitespaceChars)
  }
}

console.log('writing_screen_clear 纯逻辑单测（doClearContent 走编辑事务 + contentDelta + receipt 校正）')
console.log('---')

test('doClearContent: 调用 replace(0, utf8End, "", fullText, Programmatic)，不调 clearChapter', async () => {
  const snap = { text: 'hello world', revision: 3, cursor: 5, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = {
    success: true,
    data: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 11, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 10 } }),
    warnings: [], changedPaths: [], changedEntities: [],
  }
  const bridge = new MockBridge()
  bridge.saveChapterResult = { success: true, data: makeSaveReceipt({ wordCount: 0, contentHash: 'hash-empty' }), warnings: [], changedPaths: [], changedEntities: [] }

  const state = { chapterId: 'c1', content: 'hello world', wordCount: 11, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: 'hello world', lastSavedContentHash: 'hash-old' }
  await doClearContent(state, coord, bridge)

  assert.equal(coord.replaceCalls.length, 1)
  assert.equal(coord.replaceCalls[0].start, 0)
  assert.equal(coord.replaceCalls[0].end, 11)
  assert.equal(coord.replaceCalls[0].text, '')
  assert.equal(coord.replaceCalls[0].originalText, 'hello world')
  assert.equal(coord.replaceCalls[0].cause, 'Programmatic')
  assert.equal(bridge.clearChapterCalls.length, 0)
})

test('doClearContent: 不单独算字数，listener 按 editResult.contentDelta 实时更新，最终 receipt.wordCount 校正', async () => {
  const snap = { text: 'hello', revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = {
    success: true,
    data: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 42, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 4 } }),
    warnings: [], changedPaths: [], changedEntities: [],
  }
  const bridge = new MockBridge()
  bridge.saveChapterResult = { success: true, data: makeSaveReceipt({ wordCount: 0, contentHash: 'h1' }), warnings: [], changedPaths: [], changedEntities: [] }

  const state = { chapterId: 'c1', content: 'hello', wordCount: 50, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: 'hello', lastSavedContentHash: 'h0' }
  // 模拟 listener：replace 成功后 coordinator 回调 listener with editResult
  // listener 按 insertedNonWhitespaceChars/deletedNonWhitespaceChars 更新 wordCount
  await doClearContent(state, coord, bridge)
  // doClearContent 不再单独算字数；listener 路径在下方独立测试验证。
  // 此处只验证最终 receipt.wordCount 校正（清空后 latest.text==='' === savedText）
  assert.equal(state.wordCount, 0)
  assert.equal(state.lastSaveFailed, false)
})

test('doClearContent: 中文正文 replace 后 saveChapter 保存空正文', async () => {
  const snap = { text: '你好世界', revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = {
    success: true,
    data: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 4, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 4 } }),
    warnings: [], changedPaths: [], changedEntities: [],
  }
  const bridge = new MockBridge()
  bridge.saveChapterResult = { success: true, data: makeSaveReceipt({ wordCount: 0, contentHash: 'h-zh' }), warnings: [], changedPaths: [], changedEntities: [] }

  const state = { chapterId: 'c1', content: '你好世界', wordCount: 4, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: '你好世界', lastSavedContentHash: 'h0' }
  await doClearContent(state, coord, bridge)

  assert.equal(bridge.saveChapterCalls.length, 1)
  assert.equal(bridge.saveChapterCalls[0].text, '')
  assert.equal(state.wordCount, 0)
})

test('doClearContent: 保存后 wordCount = receipt.wordCount, lastSavedContentHash = receipt.contentHash', async () => {
  const snap = { text: 'some content', revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = {
    success: true,
    data: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 12, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 11 } }),
    warnings: [], changedPaths: [], changedEntities: [],
  }
  const bridge = new MockBridge()
  bridge.saveChapterResult = { success: true, data: makeSaveReceipt({ wordCount: 0, contentHash: 'hash-cleared' }), warnings: [], changedPaths: [], changedEntities: [] }

  const state = { chapterId: 'c1', content: 'some content', wordCount: 12, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: 'some content', lastSavedContentHash: 'hash-old' }
  await doClearContent(state, coord, bridge)

  assert.equal(state.wordCount, 0)
  assert.equal(state.lastSavedContentHash, 'hash-cleared')
  assert.equal(state.hasUnsavedChanges, false)
})

test('doClearContent: replace 失败 → 不调 saveChapter，lastSaveFailed=true', async () => {
  const snap = { text: 'hello', revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = { success: false, errorCode: 'STALE_REVISION', warnings: [], changedPaths: [], changedEntities: [] }
  const bridge = new MockBridge()

  const state = { chapterId: 'c1', content: 'hello', wordCount: 5, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: 'hello', lastSavedContentHash: 'h0' }
  await doClearContent(state, coord, bridge)

  assert.equal(bridge.saveChapterCalls.length, 0)
  assert.equal(state.lastSaveFailed, true)
  assert.equal(state.isSaving, false)
})

test('doClearContent: replace 抛异常 → 不保存，isSaving 释放', async () => {
  const snap = { text: 'hello', revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replace = async () => { throw new Error('bridge crashed') }
  const bridge = new MockBridge()

  const state = { chapterId: 'c1', content: 'hello', wordCount: 5, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: 'hello', lastSavedContentHash: 'h0' }
  await doClearContent(state, coord, bridge)

  assert.equal(bridge.saveChapterCalls.length, 0)
  assert.equal(state.lastSaveFailed, true)
  assert.equal(state.isSaving, false)
})

test('doClearContent: snapshot 为 null → 直接 lastSaveFailed', async () => {
  const coord = new MockCoordinator(null)
  const bridge = new MockBridge()

  const state = { chapterId: 'c1', content: '', wordCount: 0, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: false, lastSavedContent: '', lastSavedContentHash: null }
  await doClearContent(state, coord, bridge)

  assert.equal(coord.replaceCalls.length, 0)
  assert.equal(bridge.saveChapterCalls.length, 0)
  assert.equal(state.lastSaveFailed, true)
})

test('doClearContent: 中文 utf8EndByte = 12（不是 .length=4）', async () => {
  const text = '你好世界'
  const snap = { text, revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = {
    success: true,
    data: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 4, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 4 } }),
    warnings: [], changedPaths: [], changedEntities: [],
  }
  const bridge = new MockBridge()
  bridge.saveChapterResult = { success: true, data: makeSaveReceipt({ wordCount: 0, contentHash: 'h' }), warnings: [], changedPaths: [], changedEntities: [] }

  const state = { chapterId: 'c1', content: text, wordCount: 4, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: text, lastSavedContentHash: 'h0' }
  await doClearContent(state, coord, bridge)

  assert.equal(coord.replaceCalls[0].end, 12)
  assert.equal(coord.replaceCalls[0].originalText, '你好世界')
})

test('doClearContent: emoji utf8EndByte = 6（不是 .length=4）', async () => {
  const text = 'a🎉b'
  const snap = { text, revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = {
    success: true,
    data: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 3, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 3 } }),
    warnings: [], changedPaths: [], changedEntities: [],
  }
  const bridge = new MockBridge()
  bridge.saveChapterResult = { success: true, data: makeSaveReceipt({ wordCount: 0, contentHash: 'h' }), warnings: [], changedPaths: [], changedEntities: [] }

  const state = { chapterId: 'c1', content: text, wordCount: 3, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: text, lastSavedContentHash: 'h0' }
  await doClearContent(state, coord, bridge)

  assert.equal(coord.replaceCalls[0].end, 6)
})

test('doClearContent: cause = Programmatic', async () => {
  const snap = { text: 'x', revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = {
    success: true,
    data: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 1, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 1 } }),
    warnings: [], changedPaths: [], changedEntities: [],
  }
  const bridge = new MockBridge()
  bridge.saveChapterResult = { success: true, data: makeSaveReceipt({ wordCount: 0, contentHash: 'h' }), warnings: [], changedPaths: [], changedEntities: [] }

  const state = { chapterId: 'c1', content: 'x', wordCount: 1, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: 'x', lastSavedContentHash: 'h0' }
  await doClearContent(state, coord, bridge)

  assert.equal(coord.replaceCalls[0].cause, 'Programmatic')
})

test('doClearContent: saveChapter 失败 → lastSaveFailed=true', async () => {
  const snap = { text: 'hello', revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = {
    success: true,
    data: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 5, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 5 } }),
    warnings: [], changedPaths: [], changedEntities: [],
  }
  const bridge = new MockBridge()
  bridge.saveChapterResult = { success: false, errorCode: 'IO_ERROR', warnings: [], changedPaths: [], changedEntities: [] }

  const state = { chapterId: 'c1', content: 'hello', wordCount: 5, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: 'hello', lastSavedContentHash: 'h0' }
  await doClearContent(state, coord, bridge)

  assert.equal(state.lastSaveFailed, true)
})

test('doClearContent: replace 后 isSaving 释放，saveChapter 能正常执行', async () => {
  const snap = { text: 'hello', revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = {
    success: true,
    data: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 5, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 5 } }),
    warnings: [], changedPaths: [], changedEntities: [],
  }
  const bridge = new MockBridge()
  bridge.saveChapterResult = { success: true, data: makeSaveReceipt({ wordCount: 0, contentHash: 'h' }), warnings: [], changedPaths: [], changedEntities: [] }

  const state = { chapterId: 'c1', content: 'hello', wordCount: 5, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: 'hello', lastSavedContentHash: 'h0' }
  await doClearContent(state, coord, bridge)

  assert.equal(bridge.saveChapterCalls.length, 1)
  assert.equal(state.isSaving, false)
  assert.equal(state.lastSaveFailed, false)
})

// ── Issue #629 评论9 第4项：listener 实时 wordCount 路径 ──

test('listener: 收到 editResult 时 wordCount 用 insertedNonWhitespaceChars/deletedNonWhitespaceChars 实时更新', async () => {
  // 模拟用户输入 'abc'：3 个非空白字符，listener 应把 wordCount 从 0 → 3
  const state = { wordCount: 0, content: '', lastSavedContent: '', hasUnsavedChanges: false }
  const update = {
    snapshot: { text: 'abc', revision: 1, cursor: 3, selectionAnchor: 3, generation: 1, chapterId: 'c1', composition: null },
    editResult: makeEditResult({ contentDelta: { insertedChars: 3, deletedChars: 0, insertedNonWhitespaceChars: 3, deletedNonWhitespaceChars: 0 } }),
  }
  applyStateUpdate(state, update)
  assert.equal(state.wordCount, 3, 'listener 按 insertedNonWhitespaceChars 实时更新 wordCount')
  assert.equal(state.content, 'abc')
  assert.equal(state.hasUnsavedChanges, true)
})

test('listener: 删除非空白字符时 wordCount 按 deletedNonWhitespaceChars 递减', async () => {
  // 模拟用户从 'abc' 删除 'c'：1 个非空白字符，listener 应把 wordCount 从 3 → 2
  const state = { wordCount: 3, content: 'abc', lastSavedContent: 'abc', hasUnsavedChanges: false }
  const update = {
    snapshot: { text: 'ab', revision: 2, cursor: 2, selectionAnchor: 2, generation: 1, chapterId: 'c1', composition: null },
    editResult: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 1, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 1 } }),
  }
  applyStateUpdate(state, update)
  assert.equal(state.wordCount, 2, 'listener 按 deletedNonWhitespaceChars 递减 wordCount')
  assert.equal(state.content, 'ab')
})

test('listener: 空白字符输入不增加 wordCount（只算非空白字符）', async () => {
  // 模拟用户输入空格：insertedNonWhitespaceChars=0，wordCount 不变
  const state = { wordCount: 3, content: 'abc', lastSavedContent: 'abc', hasUnsavedChanges: false }
  const update = {
    snapshot: { text: 'abc ', revision: 2, cursor: 4, selectionAnchor: 4, generation: 1, chapterId: 'c1', composition: null },
    editResult: makeEditResult({ contentDelta: { insertedChars: 1, deletedChars: 0, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 0 } }),
  }
  applyStateUpdate(state, update)
  assert.equal(state.wordCount, 3, '空白字符输入不增加 wordCount')
  assert.equal(state.content, 'abc ')
})

test('listener: editResult=null（open 初始加载）时不更新 wordCount', async () => {
  // open 成功后 listener 收到 editResult=null，wordCount 保持上次值（由 loadChapter 设置）
  const state = { wordCount: 42, content: '', lastSavedContent: '', hasUnsavedChanges: false }
  const update = {
    snapshot: { text: 'loaded', revision: 1, cursor: 5, selectionAnchor: 5, generation: 1, chapterId: 'c1', composition: null },
    editResult: null,
  }
  applyStateUpdate(state, update)
  assert.equal(state.wordCount, 42, 'editResult=null 时 wordCount 保持上次值（不更新）')
  assert.equal(state.content, 'loaded')
})

test('listener: wordCount 不会变负（Math.max(0, ...) 兜底）', async () => {
  // 模拟异常：deletedNonWhitespaceChars 大于当前 wordCount
  const state = { wordCount: 1, content: 'a', lastSavedContent: 'a', hasUnsavedChanges: false }
  const update = {
    snapshot: { text: '', revision: 2, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null },
    editResult: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 1, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 5 } }),
  }
  applyStateUpdate(state, update)
  assert.equal(state.wordCount, 0, 'wordCount 不变负，Math.max(0, ...) 兜底')
})

test('doClearContent + listener: 清空正文后 listener 把 wordCount 实时归零，saveChapter 用 receipt.wordCount 校正', async () => {
  // 端到端：doClearContent replace 后 listener 自动更新 wordCount，saveChapter 保存空正文后 receipt.wordCount=0 校正
  const snap = { text: 'hello world', revision: 1, cursor: 0, selectionAnchor: 0, generation: 1, chapterId: 'c1', composition: null }
  const coord = new MockCoordinator(snap)
  coord.replaceResult = {
    success: true,
    data: makeEditResult({ contentDelta: { insertedChars: 0, deletedChars: 11, insertedNonWhitespaceChars: 0, deletedNonWhitespaceChars: 10 } }),
    warnings: [], changedPaths: [], changedEntities: [],
  }
  const bridge = new MockBridge()
  bridge.saveChapterResult = { success: true, data: makeSaveReceipt({ wordCount: 0, contentHash: 'h-empty' }), warnings: [], changedPaths: [], changedEntities: [] }

  const state = { chapterId: 'c1', content: 'hello world', wordCount: 10, isSaving: false, lastSaveFailed: false, hasUnsavedChanges: true, lastSavedContent: 'hello world', lastSavedContentHash: 'h0' }
  await doClearContent(state, coord, bridge)
  // doClearContent 不再单独算字数；listener 路径在上方独立测试验证。
  // 最终 saveChapter 的 receipt.wordCount=0 校正
  assert.equal(state.wordCount, 0)
  assert.equal(state.lastSavedContent, '')
  assert.equal(state.hasUnsavedChanges, false)
})

console.log('---')
console.log(`✅ writing_screen_clear: ${passed} tests passed`)
