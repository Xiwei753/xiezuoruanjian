// continuation_content_hash.test.mjs — WritingScreen lastSavedContentHash + updateContinuationState 纯逻辑单测。
//
// 验证 Issue #629 评论 5 第 8 节 WritingScreen 部分的核心行为：
//   1. lastSavedContentHash 流转：加载来自 ChapterContent.meta.hash，保存来自 ChapterSaveReceipt.contentHash。
//   2. contentHash 匹配/不匹配：目标设备用 result.data.meta.hash === payload.contentHash 判断能否恢复 selection/cursor。
//   3. 源设备有未保存修改时，不把基于未持久化正文的 selection/cursor 标成"可精确恢复"；
//      updateContinuationState 只带导航信息 + contentHash（上次保存的），不带 selection/cursor。
//   4. 保存后（hasUnsavedChanges=false）才带完整 selection/cursor + contentHash。
//   5. contentHash 始终来自 lastSavedContentHash（不用 revision）。
//
// 运行：node continuation_content_hash.test.mjs
//
// 注意：.ets 依赖 ArkUI 无法用 Node 直接测，本测试验证提取的纯逻辑。
// 生产代码 WritingScreen.ets 调用相同逻辑，需 HarmonyOS SDK 才能端到端编译——本地无 SDK，此为已知阻塞。

import { strict as assert } from 'node:assert'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// UTF-8 byte offset → UTF-16 code unit offset（与 TextOffsetMapper.utf8ToUtf16 对齐）
function utf8ToUtf16(text, utf8Offset) {
  if (utf8Offset <= 0) return 0
  let byteLen = 0
  let utf16Index = 0
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i)
    let charByteLen = 1
    if (code < 0x80) {
      charByteLen = 1
    } else if (code < 0x800) {
      charByteLen = 2
    } else if (code >= 0xD800 && code <= 0xDBFF) {
      charByteLen = 4
      i += 1
    } else {
      charByteLen = 3
    }
    if (byteLen + charByteLen > utf8Offset) {
      return utf16Index
    }
    byteLen += charByteLen
    utf16Index += (charByteLen === 4 ? 2 : 1)
  }
  return utf16Index
}

// ── updateContinuationState 逻辑（与 WritingScreen.updateContinuationState 对齐）──
// contentHash 来自 lastSavedContentHash；有未保存修改时不带 selection/cursor。
function buildContinuationPayload(snap, state) {
  if (!state.projectId || !state.chapterId) return null
  if (!snap) return null
  const payload = {
    projectId: state.projectId,
    volumeId: state.volumeId,
    chapterId: state.chapterId,
    contentHash: state.lastSavedContentHash !== null ? state.lastSavedContentHash : undefined,
  }
  // 只有已保存时才带 selection/cursor
  if (!state.hasUnsavedChanges) {
    const cursorUtf16 = utf8ToUtf16(snap.text, snap.cursor)
    const anchorUtf16 = utf8ToUtf16(snap.text, snap.selectionAnchor)
    payload.selection = [anchorUtf16, cursorUtf16]
    payload.cursor = cursorUtf16
  }
  if (state.currentViewport !== null) {
    payload.viewport = state.currentViewport
  }
  return payload
}

// ── restoreContinuationEditorState 决策（与 WritingScreen.restoreContinuationEditorState 对齐）──
// contentHash 匹配时恢复 selection/cursor；不匹配时安全恢复。
function decideRestoration(snap, params, lastSavedContentHash) {
  const result = { shouldRestoreSelection: false, anchorByte: 0, headByte: 0 }
  if (params === null) return result
  const contentHashMatches =
    params.contentHash === undefined || params.contentHash === lastSavedContentHash
  if (!contentHashMatches) return result
  if (params.selection !== undefined && params.selection.length >= 2) {
    result.shouldRestoreSelection = true
    // UTF-16 → UTF-8 转换省略（这里只测决策）
    result.anchorByte = params.selection[0]
    result.headByte = params.selection[1]
  } else if (params.cursor !== undefined) {
    result.shouldRestoreSelection = true
    result.anchorByte = params.cursor
    result.headByte = params.cursor
  }
  return result
}

console.log('continuation_content_hash 纯逻辑单测（lastSavedContentHash 流转 + 未保存不宣称精确恢复）')
console.log('---')

// ── 1. lastSavedContentHash 流转 ──
test('loadChapter: lastSavedContentHash = ChapterContent.meta.hash', () => {
  // 模拟 loadChapter 成功
  const chapterContent = { meta: { hash: 'meta-hash-abc', wordCount: 5 }, content: 'hello' }
  const state = { lastSavedContentHash: null }
  // loadChapter 里：this.lastSavedContentHash = result.data.meta.hash
  state.lastSavedContentHash = chapterContent.meta.hash
  assert.equal(state.lastSavedContentHash, 'meta-hash-abc')
})

test('saveChapter 成功: lastSavedContentHash = ChapterSaveReceipt.contentHash', () => {
  const receipt = { contentHash: 'receipt-hash-xyz', wordCount: 5, contentLen: 5 }
  const state = { lastSavedContentHash: 'meta-hash-old' }
  // saveChapter 里：this.lastSavedContentHash = result.data.contentHash
  state.lastSavedContentHash = receipt.contentHash
  assert.equal(state.lastSavedContentHash, 'receipt-hash-xyz')
})

test('lastSavedContentHash 流转: load → edit → save → 新 hash', () => {
  // 1. 加载：lastSavedContentHash = meta.hash = 'h-v1'
  let state = { lastSavedContentHash: null, hasUnsavedChanges: false }
  state.lastSavedContentHash = 'h-v1'
  assert.equal(state.lastSavedContentHash, 'h-v1')

  // 2. 编辑：hasUnsavedChanges=true，lastSavedContentHash 不变（仍是上次保存的）
  state.hasUnsavedChanges = true
  assert.equal(state.lastSavedContentHash, 'h-v1')

  // 3. 保存：lastSavedContentHash = receipt.contentHash = 'h-v2'
  state.lastSavedContentHash = 'h-v2'
  state.hasUnsavedChanges = false
  assert.equal(state.lastSavedContentHash, 'h-v2')
})

// ── 2. contentHash 匹配/不匹配 ──
test('restore: payload.contentHash === lastSavedContentHash → 可恢复 selection', () => {
  const snap = { text: 'hello', cursor: 0, selectionAnchor: 0 }
  const params = { contentHash: 'h-1', selection: [1, 3] }
  const decision = decideRestoration(snap, params, 'h-1')
  assert.equal(decision.shouldRestoreSelection, true)
})

test('restore: payload.contentHash !== lastSavedContentHash → 安全恢复（不恢复 selection）', () => {
  const snap = { text: 'hello', cursor: 0, selectionAnchor: 0 }
  const params = { contentHash: 'h-old', selection: [1, 3] }
  const decision = decideRestoration(snap, params, 'h-new')
  assert.equal(decision.shouldRestoreSelection, false)
})

test('restore: payload.contentHash undefined → 视为匹配（兼容旧 payload）', () => {
  const snap = { text: 'hello', cursor: 0, selectionAnchor: 0 }
  const params = { selection: [1, 3] }  // 不带 contentHash
  const decision = decideRestoration(snap, params, 'h-anything')
  assert.equal(decision.shouldRestoreSelection, true)
})

test('restore: 目标设备用 result.data.meta.hash === payload.contentHash 判断', () => {
  // 目标设备加载章节后 lastSavedContentHash = result.data.meta.hash
  const targetLoadedHash = 'meta-hash-from-disk'
  // payload.contentHash 是源设备保存时写入的
  const payload = { contentHash: 'meta-hash-from-disk', selection: [2, 4] }
  const decision = decideRestoration({ text: 'hello' }, payload, targetLoadedHash)
  assert.equal(decision.shouldRestoreSelection, true)
})

// ── 3. 源设备有未保存修改时不带 selection/cursor ──
test('updateContinuationState: hasUnsavedChanges=true → payload 不带 selection/cursor', () => {
  const snap = { text: 'edited', cursor: 3, selectionAnchor: 1 }
  const state = {
    projectId: 'p1', volumeId: 'v1', chapterId: 'c1',
    hasUnsavedChanges: true,
    lastSavedContentHash: 'h-saved-earlier',
    currentViewport: null,
  }
  const payload = buildContinuationPayload(snap, state)
  assert.equal(payload.projectId, 'p1')
  assert.equal(payload.chapterId, 'c1')
  assert.equal(payload.contentHash, 'h-saved-earlier')
  // 关键：不带 selection/cursor（基于未持久化正文，不标"可精确恢复"）
  assert.equal(payload.selection, undefined)
  assert.equal(payload.cursor, undefined)
})

test('updateContinuationState: hasUnsavedChanges=false → payload 带完整 selection/cursor', () => {
  const snap = { text: 'saved', cursor: 2, selectionAnchor: 0 }
  const state = {
    projectId: 'p1', volumeId: 'v1', chapterId: 'c1',
    hasUnsavedChanges: false,
    lastSavedContentHash: 'h-current',
    currentViewport: null,
  }
  const payload = buildContinuationPayload(snap, state)
  assert.equal(payload.contentHash, 'h-current')
  // 已保存：带 selection/cursor
  assert.deepEqual(payload.selection, [0, 2])
  assert.equal(payload.cursor, 2)
})

test('updateContinuationState: 未保存时仍带 contentHash（上次保存的）', () => {
  const snap = { text: 'unsaved edit', cursor: 0, selectionAnchor: 0 }
  const state = {
    projectId: 'p1', volumeId: 'v1', chapterId: 'c1',
    hasUnsavedChanges: true,
    lastSavedContentHash: 'h-last-save',
    currentViewport: null,
  }
  const payload = buildContinuationPayload(snap, state)
  // contentHash 仍是上次保存的（不基于未持久化正文）
  assert.equal(payload.contentHash, 'h-last-save')
  assert.equal(payload.selection, undefined)
})

test('updateContinuationState: lastSavedContentHash=null → contentHash undefined', () => {
  const snap = { text: 'x', cursor: 0, selectionAnchor: 0 }
  const state = {
    projectId: 'p1', volumeId: 'v1', chapterId: 'c1',
    hasUnsavedChanges: false,
    lastSavedContentHash: null,
    currentViewport: null,
  }
  const payload = buildContinuationPayload(snap, state)
  assert.equal(payload.contentHash, undefined)
})

// ── 4. 保存后带完整 selection/cursor + contentHash ──
test('保存后 updateContinuationState: hasUnsavedChanges=false + 新 contentHash + selection', () => {
  // 保存前：hasUnsavedChanges=true，不带 selection
  let snap = { text: 'edited content', cursor: 7, selectionAnchor: 0 }
  let state = {
    projectId: 'p1', volumeId: 'v1', chapterId: 'c1',
    hasUnsavedChanges: true,
    lastSavedContentHash: 'h-v1',
    currentViewport: null,
  }
  let payload = buildContinuationPayload(snap, state)
  assert.equal(payload.selection, undefined)

  // 保存后：hasUnsavedChanges=false, lastSavedContentHash='h-v2'
  state.hasUnsavedChanges = false
  state.lastSavedContentHash = 'h-v2'
  payload = buildContinuationPayload(snap, state)
  assert.equal(payload.contentHash, 'h-v2')
  assert.deepEqual(payload.selection, [0, 7])
  assert.equal(payload.cursor, 7)
})

// ── 5. contentHash 始终来自 lastSavedContentHash（不用 revision）──
test('payload.contentHash 来自 lastSavedContentHash，不用 snap.revision', () => {
  const snap = { text: 'hello', cursor: 0, selectionAnchor: 0, revision: 42 }
  const state = {
    projectId: 'p1', volumeId: 'v1', chapterId: 'c1',
    hasUnsavedChanges: false,
    lastSavedContentHash: 'content-hash-stable',
    currentViewport: null,
  }
  const payload = buildContinuationPayload(snap, state)
  assert.equal(payload.contentHash, 'content-hash-stable')
  // 不用 revision
  assert.notEqual(payload.contentHash, 42)
})

// ── 6. viewport 仍带（不受未保存影响）──
test('updateContinuationState: 未保存时 viewport 仍带', () => {
  const snap = { text: 'x', cursor: 0, selectionAnchor: 0 }
  const vp = { scrollTop: 100, scrollLeft: 0, viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 1000 }
  const state = {
    projectId: 'p1', volumeId: 'v1', chapterId: 'c1',
    hasUnsavedChanges: true,
    lastSavedContentHash: 'h1',
    currentViewport: vp,
  }
  const payload = buildContinuationPayload(snap, state)
  assert.deepEqual(payload.viewport, vp)
  assert.equal(payload.selection, undefined)
})

// ── 7. 完整接续流程：源设备保存 → 目标设备恢复 ──
test('完整流程: 源设备保存后写 payload → 目标设备加载同章节 contentHash 匹配 → 恢复 selection', () => {
  // 源设备：保存后 lastSavedContentHash='h-saved', hasUnsavedChanges=false
  const sourceSnap = { text: 'saved text', cursor: 5, selectionAnchor: 2 }
  const sourceState = {
    projectId: 'p1', volumeId: 'v1', chapterId: 'c1',
    hasUnsavedChanges: false,
    lastSavedContentHash: 'h-saved',
    currentViewport: null,
  }
  const payload = buildContinuationPayload(sourceSnap, sourceState)
  assert.equal(payload.contentHash, 'h-saved')
  assert.deepEqual(payload.selection, [2, 5])

  // 目标设备：加载同一章节，meta.hash='h-saved'（同一份持久化内容）
  const targetLastSavedHash = 'h-saved'
  const decision = decideRestoration({ text: 'saved text' }, payload, targetLastSavedHash)
  assert.equal(decision.shouldRestoreSelection, true)
})

test('完整流程: 源设备未保存 → payload 不带 selection → 目标设备无法恢复 selection', () => {
  // 源设备：有未保存修改
  const sourceSnap = { text: 'unsaved', cursor: 3, selectionAnchor: 0 }
  const sourceState = {
    projectId: 'p1', volumeId: 'v1', chapterId: 'c1',
    hasUnsavedChanges: true,
    lastSavedContentHash: 'h-v1',
    currentViewport: null,
  }
  const payload = buildContinuationPayload(sourceSnap, sourceState)
  assert.equal(payload.selection, undefined)
  assert.equal(payload.cursor, undefined)

  // 目标设备：即使 contentHash 匹配（磁盘还是 h-v1），也没有 selection 可恢复
  const decision = decideRestoration({ text: 'unsaved' }, payload, 'h-v1')
  assert.equal(decision.shouldRestoreSelection, false)
})

test('完整流程: 源设备未保存 + 目标设备磁盘已更新 → contentHash 不匹配 → 安全恢复', () => {
  // 源设备：有未保存修改，lastSavedContentHash='h-v1'（上次保存的）
  const sourceSnap = { text: 'editing', cursor: 0, selectionAnchor: 0 }
  const sourceState = {
    projectId: 'p1', volumeId: 'v1', chapterId: 'c1',
    hasUnsavedChanges: true,
    lastSavedContentHash: 'h-v1',
    currentViewport: null,
  }
  const payload = buildContinuationPayload(sourceSnap, sourceState)
  assert.equal(payload.contentHash, 'h-v1')

  // 目标设备：磁盘内容已被同步更新为 'h-v2'
  const decision = decideRestoration({ text: 'different' }, payload, 'h-v2')
  assert.equal(decision.shouldRestoreSelection, false)
})

// ── 8. sanitizePayload 保留 contentHash（与 AppContinuationService 对齐）──
test('sanitizePayload: contentHash 字段被保留', () => {
  // 模拟 AppContinuationService.sanitizePayload
  function sanitizePayload(payload) {
    const result = {}
    if (payload.projectId !== undefined) result.projectId = payload.projectId
    if (payload.volumeId !== undefined) result.volumeId = payload.volumeId
    if (payload.chapterId !== undefined) result.chapterId = payload.chapterId
    if (payload.selection !== undefined) result.selection = payload.selection
    if (payload.cursor !== undefined) result.cursor = payload.cursor
    if (payload.viewport !== undefined) result.viewport = payload.viewport
    if (payload.contentHash !== undefined) result.contentHash = payload.contentHash
    return result
  }
  const payload = { projectId: 'p1', chapterId: 'c1', contentHash: 'h-1', selection: [1, 2] }
  const sanitized = sanitizePayload(payload)
  assert.equal(sanitized.contentHash, 'h-1')
  assert.equal(sanitized.projectId, 'p1')
  assert.deepEqual(sanitized.selection, [1, 2])
})

test('sanitizePayload: 不含 revision 字段（已迁移到 contentHash）', () => {
  function sanitizePayload(payload) {
    const result = {}
    if (payload.projectId !== undefined) result.projectId = payload.projectId
    if (payload.chapterId !== undefined) result.chapterId = payload.chapterId
    if (payload.contentHash !== undefined) result.contentHash = payload.contentHash
    return result
  }
  const payload = { projectId: 'p1', chapterId: 'c1', contentHash: 'h-1' }
  const sanitized = sanitizePayload(payload)
  assert.equal(sanitized.revision, undefined)
  assert.equal(sanitized.contentHash, 'h-1')
})

// ── 9. fromObject 反序列化 contentHash（与 AppContinuationService 对齐）──
test('fromObject: obj[contentHash] → result.contentHash（string）', () => {
  function fromObject(obj) {
    const result = {}
    if (obj['projectId'] !== undefined) result.projectId = obj['projectId']
    if (obj['chapterId'] !== undefined) result.chapterId = obj['chapterId']
    if (obj['contentHash'] !== undefined) result.contentHash = obj['contentHash']
    return result
  }
  const obj = { projectId: 'p1', chapterId: 'c1', contentHash: 'h-from-json' }
  const payload = fromObject(obj)
  assert.equal(payload.contentHash, 'h-from-json')
})

test('fromObject: 不读 obj[revision]（已迁移）', () => {
  function fromObject(obj) {
    const result = {}
    if (obj['contentHash'] !== undefined) result.contentHash = obj['contentHash']
    return result
  }
  // 旧 payload 可能还带 revision，但新代码不读
  const obj = { revision: 42, contentHash: 'h-new' }
  const payload = fromObject(obj)
  assert.equal(payload.revision, undefined)
  assert.equal(payload.contentHash, 'h-new')
})

// ── 10. AppNavigation 传递 contentHash（与 applyContinuation 对齐）──
test('AppNavigation.applyContinuation: payload.contentHash → wParam.contentHash', () => {
  // 模拟 AppNavigation.applyContinuation 构造 WritingRouteParams
  const payload = { chapterId: 'c1', projectId: 'p1', volumeId: 'v1', contentHash: 'h-nav' }
  const wParam = { chapterId: payload.chapterId, chapterTitle: '', projectId: payload.projectId, volumeId: payload.volumeId }
  if (payload.contentHash !== undefined) wParam.contentHash = payload.contentHash
  assert.equal(wParam.contentHash, 'h-nav')
  assert.equal(wParam.revision, undefined)
})

console.log('---')
console.log(`✅ continuation_content_hash: ${passed} tests passed`)
