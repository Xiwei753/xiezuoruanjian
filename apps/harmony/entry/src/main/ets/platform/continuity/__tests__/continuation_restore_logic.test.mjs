// continuation_restore_logic.test.mjs — 接续编辑器状态恢复的纯逻辑单测。
//
// 验证 Issue #629 评论 5 第 8 节的核心行为：
//   1. WritingRouteParams 正式字段传递（不用 Record 偷塞）— 结构性检查
//   2. contentHash 不匹配时不强套旧 offset（安全恢复）
//   3. contentHash 匹配时 selection/cursor 正确恢复（UTF-16→UTF-8 转换）
//   4. selection 优先于 cursor
//   5. updateCurrentState 持续写入（UTF-8→UTF-16 转换）
//   6. offset 转换 round-trip 正确性（含多字节字符）
//   7. viewport 透传到 UI，不参与 contentHash 校验
//
// Issue #629 评论 5 第 8 节：跨设备内容身份用 contentHash（稳定内容哈希），
// 不用 TextEditSession.revision。Rust TextEditSession::with_text() 每次新建会话 revision
// 都是 EditorRevision::initial()，目标设备重新打开章节后拿到的是新会话 revision，
// 跟源设备 session revision 没有跨设备比较意义。contentHash 来自：
//   - 加载章节时：ChapterContent.meta.hash
//   - 保存成功后：ChapterSaveReceipt.contentHash
//
// 运行：node continuation_restore_logic.test.mjs
//
// 注意：.ets 依赖 ArkUI 无法用 Node 直接测，本测试验证提取的纯逻辑。
// 生产代码 WritingScreen.ets / AppNavigation.ets 调用相同逻辑，
// 需 HarmonyOS SDK 才能端到端编译——本地无 SDK，此为已知阻塞。

import { strict as assert } from 'node:assert'

let passed = 0
const test = (name, fn) => {
  fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

// ── 纯逻辑：与 TextOffsetMapper.ets / WritingScreen.ets 对齐 ──

// UTF-16 code unit offset → UTF-8 byte offset
function utf16ToUtf8(text, utf16Offset) {
  if (utf16Offset <= 0) return 0
  const limited = utf16Offset > text.length ? text.length : utf16Offset
  const sub = text.substring(0, limited)
  return new TextEncoder().encode(sub).length
}

// UTF-8 byte offset → UTF-16 code unit offset
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

// ContinuationPayload 形状（contentHash 替代 revision）
function makePayload(overrides) {
  return {
    projectId: undefined,
    volumeId: undefined,
    chapterId: undefined,
    selection: undefined,
    cursor: undefined,
    viewport: undefined,
    contentHash: undefined,
    ...overrides,
  }
}

// Core snapshot 形状（cursor/selectionAnchor 是 UTF-8 byte offset；contentHash 是稳定内容哈希）
function makeSnapshot(overrides) {
  return {
    text: '',
    contentHash: 'hash-1',
    cursor: 0,
    selectionAnchor: 0,
    ...overrides,
  }
}

// ── decideRestoration：恢复决策（与 WritingScreen.restoreContinuationEditorState 对齐）──
// contentHash 匹配时恢复 selection/cursor；不匹配时安全恢复（不强套旧 offset）。
function decideRestoration(snap, params) {
  const defaultResult = {
    shouldRestoreSelection: false,
    anchorByte: 0,
    headByte: 0,
    hasViewport: params !== null && params.viewport !== undefined,
    viewport: params !== null ? params.viewport : undefined,
  }
  if (params === null) return defaultResult

  // contentHash 校验：undefined 时视为匹配（兼容旧 payload）；否则必须严格相等。
  const contentHashMatches =
    params.contentHash === undefined || params.contentHash === snap.contentHash
  if (!contentHashMatches) return defaultResult

  if (params.selection !== undefined && params.selection.length >= 2) {
    return {
      shouldRestoreSelection: true,
      anchorByte: utf16ToUtf8(snap.text, params.selection[0]),
      headByte: utf16ToUtf8(snap.text, params.selection[1]),
      hasViewport: params.viewport !== undefined,
      viewport: params.viewport,
    }
  }
  if (params.cursor !== undefined) {
    const cursorByte = utf16ToUtf8(snap.text, params.cursor)
    return {
      shouldRestoreSelection: true,
      anchorByte: cursorByte,
      headByte: cursorByte,
      hasViewport: params.viewport !== undefined,
      viewport: params.viewport,
    }
  }
  return defaultResult
}

// ── buildPayloadFromSnapshot：updateCurrentState 构造（与 WritingScreen.updateContinuationState 对齐）──
// contentHash 来自 snap.contentHash（加载时 ChapterContent.meta.hash，保存后 ChapterSaveReceipt.contentHash）。
function buildPayloadFromSnapshot(snap, projectId, volumeId, chapterId, viewport) {
  const cursorUtf16 = utf8ToUtf16(snap.text, snap.cursor)
  const anchorUtf16 = utf8ToUtf16(snap.text, snap.selectionAnchor)
  const payload = {
    projectId,
    volumeId,
    chapterId,
    selection: [anchorUtf16, cursorUtf16],
    cursor: cursorUtf16,
    contentHash: snap.contentHash,
  }
  if (viewport !== null) payload.viewport = viewport
  return payload
}

console.log('continuation_restore_logic 纯逻辑单测（contentHash 跨设备校验）')
console.log('---')

// ── 1. WritingRouteParams 正式字段传递 ──
test('WritingRouteParams: selection/cursor/viewport/contentHash 作为正式可选字段可传递', () => {
  // 模拟 AppNavigation.applyContinuation 构造 WritingRouteParams
  const payload = makePayload({
    projectId: 'p1',
    volumeId: 'v1',
    chapterId: 'c1',
    selection: [3, 7],
    cursor: 5,
    viewport: { scrollTop: 120, scrollLeft: 0, viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 1200 },
    contentHash: 'sha256-abc123',
  })

  const wParam = {
    chapterId: payload.chapterId,
    chapterTitle: '',
    projectId: payload.projectId,
    volumeId: payload.volumeId,
  }
  if (payload.selection !== undefined) wParam.selection = payload.selection
  if (payload.cursor !== undefined) wParam.cursor = payload.cursor
  if (payload.viewport !== undefined) wParam.viewport = payload.viewport
  if (payload.contentHash !== undefined) wParam.contentHash = payload.contentHash

  // 正式字段可访问，不是 Record<string,Object> 偷塞
  assert.equal(wParam.chapterId, 'c1')
  assert.equal(wParam.projectId, 'p1')
  assert.equal(wParam.volumeId, 'v1')
  assert.deepEqual(wParam.selection, [3, 7])
  assert.equal(wParam.cursor, 5)
  assert.equal(wParam.contentHash, 'sha256-abc123')
  // viewport 是强类型 EditorViewportState，有明确字段
  assert.equal(wParam.viewport.scrollTop, 120)
  assert.equal(wParam.viewport.scrollLeft, 0)
  assert.equal(wParam.viewport.viewportWidth, 800)
  assert.equal(wParam.viewport.viewportHeight, 600)
})

test('WritingRouteParams: 缺省时不带 selection/cursor/viewport/contentHash（undefined）', () => {
  const wParam = {
    chapterId: 'c1',
    chapterTitle: 't',
    projectId: 'p1',
    volumeId: 'v1',
  }
  assert.equal(wParam.selection, undefined)
  assert.equal(wParam.cursor, undefined)
  assert.equal(wParam.viewport, undefined)
  assert.equal(wParam.contentHash, undefined)
})

// ── 2. contentHash 不匹配时不强套旧 offset ──
test('decideRestoration: contentHash 不匹配 → shouldRestoreSelection=false（安全恢复）', () => {
  const snap = makeSnapshot({ text: 'hello world', contentHash: 'hash-A', cursor: 5, selectionAnchor: 3 })
  const params = {
    selection: [2, 8],
    cursor: 5,
    contentHash: 'hash-B',  // 不匹配
  }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, false)
  assert.equal(decision.anchorByte, 0)
  assert.equal(decision.headByte, 0)
})

test('decideRestoration: contentHash 不匹配但 viewport 仍透传到 UI', () => {
  const snap = makeSnapshot({ text: 'hello', contentHash: 'hash-1' })
  const params = {
    selection: [1, 3],
    contentHash: 'hash-999',
    viewport: { scrollTop: 200 },
  }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, false)
  assert.equal(decision.hasViewport, true)
  assert.deepEqual(decision.viewport, { scrollTop: 200 })
})

// ── 3. contentHash 匹配时 selection/cursor 正确恢复（UTF-16→UTF-8）──
test('decideRestoration: contentHash 匹配 + selection → UTF-16 转 UTF-8 byte offset', () => {
  // 纯 ASCII：UTF-16 offset == UTF-8 byte offset
  const snap = makeSnapshot({ text: 'hello world', contentHash: 'hash-5' })
  const params = {
    selection: [2, 7],  // UTF-16 offsets
    contentHash: 'hash-5',
  }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, true)
  assert.equal(decision.anchorByte, 2)  // ASCII: byte offset == utf16 offset
  assert.equal(decision.headByte, 7)
})

test('decideRestoration: 多字节字符 UTF-16→UTF-8 转换正确', () => {
  // 中文：每个汉字 UTF-16 = 1 code unit，UTF-8 = 3 bytes
  const text = '你好世界'  // 4 UTF-16 code units, 12 UTF-8 bytes
  const snap = makeSnapshot({ text, contentHash: 'hash-zh' })
  const params = {
    selection: [1, 3],  // UTF-16 offsets: 第1个字符到第3个字符
    contentHash: 'hash-zh',
  }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, true)
  // UTF-16 offset 1 → UTF-8 byte offset 3（第一个汉字3字节）
  assert.equal(decision.anchorByte, 3)
  // UTF-16 offset 3 → UTF-8 byte offset 9（前三个汉字各3字节）
  assert.equal(decision.headByte, 9)
})

test('decideRestoration: emoji（surrogate pair）UTF-16→UTF-8 转换正确', () => {
  // 🎉 是 surrogate pair：UTF-16 = 2 code units，UTF-8 = 4 bytes
  const text = 'a🎉b'  // UTF-16: 4 code units, UTF-8: 1+4+1=6 bytes
  const snap = makeSnapshot({ text, contentHash: 'hash-emoji' })
  const params = {
    selection: [1, 3],  // UTF-16 offsets: 跨过 emoji
    contentHash: 'hash-emoji',
  }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, true)
  // UTF-16 offset 1 → UTF-8 byte offset 1（'a' 是 1 byte）
  assert.equal(decision.anchorByte, 1)
  // UTF-16 offset 3 → UTF-8 byte offset 5（'a' 1 + emoji 4 = 5）
  assert.equal(decision.headByte, 5)
})

// ── 4. selection 优先于 cursor ──
test('decideRestoration: 有 selection 时忽略 cursor（selection 优先）', () => {
  const snap = makeSnapshot({ text: 'hello', contentHash: 'h1' })
  const params = {
    selection: [1, 3],
    cursor: 4,  // 应被忽略
    contentHash: 'h1',
  }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, true)
  assert.equal(decision.anchorByte, 1)
  assert.equal(decision.headByte, 3)  // 来自 selection，不是 cursor
})

test('decideRestoration: 无 selection 但有 cursor → cursor 转 byte offset', () => {
  const snap = makeSnapshot({ text: 'hello', contentHash: 'h1' })
  const params = {
    cursor: 2,
    contentHash: 'h1',
  }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, true)
  assert.equal(decision.anchorByte, 2)
  assert.equal(decision.headByte, 2)  // cursor → anchor==head
})

test('decideRestoration: 无 selection 无 cursor → shouldRestoreSelection=false', () => {
  const snap = makeSnapshot({ text: 'hello', contentHash: 'h1' })
  const params = { contentHash: 'h1' }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, false)
})

test('decideRestoration: params=null → shouldRestoreSelection=false', () => {
  const snap = makeSnapshot({ text: 'hello', contentHash: 'h1' })
  const decision = decideRestoration(snap, null)
  assert.equal(decision.shouldRestoreSelection, false)
  assert.equal(decision.hasViewport, false)
})

// ── 5. updateCurrentState 持续写入（UTF-8→UTF-16 转换）──
test('buildPayloadFromSnapshot: Core UTF-8 cursor/anchor 转 UTF-16 存入 payload', () => {
  const snap = makeSnapshot({
    text: '你好世界',
    contentHash: 'hash-save-7',
    cursor: 6,        // UTF-8 byte offset（前两个汉字）
    selectionAnchor: 3, // UTF-8 byte offset（第一个汉字后）
  })
  const payload = buildPayloadFromSnapshot(snap, 'p1', 'v1', 'c1', null)
  assert.equal(payload.projectId, 'p1')
  assert.equal(payload.volumeId, 'v1')
  assert.equal(payload.chapterId, 'c1')
  assert.equal(payload.contentHash, 'hash-save-7')
  // UTF-8 byte 3 → UTF-16 offset 1
  assert.equal(payload.selection[0], 1)
  // UTF-8 byte 6 → UTF-16 offset 2
  assert.equal(payload.selection[1], 2)
  assert.equal(payload.cursor, 2)
})

test('buildPayloadFromSnapshot: viewport 非空时存入 payload', () => {
  const snap = makeSnapshot({ text: 'hello', contentHash: 'h1', cursor: 3, selectionAnchor: 1 })
  const viewport = { scrollTop: 250, scrollLeft: 0, viewportWidth: 800, viewportHeight: 600, contentWidth: 800, contentHeight: 800 }
  const payload = buildPayloadFromSnapshot(snap, 'p1', 'v1', 'c1', viewport)
  assert.deepEqual(payload.viewport, viewport)
})

test('buildPayloadFromSnapshot: viewport=null 时 payload 不带 viewport', () => {
  const snap = makeSnapshot({ text: 'hello', contentHash: 'h1', cursor: 0, selectionAnchor: 0 })
  const payload = buildPayloadFromSnapshot(snap, 'p1', 'v1', 'c1', null)
  assert.equal(payload.viewport, undefined)
})

// ── 6. offset 转换 round-trip 正确性 ──
test('round-trip: UTF-16→UTF-8→UTF-16 == identity（ASCII）', () => {
  const text = 'hello world'
  for (let i = 0; i <= text.length; i++) {
    const byte = utf16ToUtf8(text, i)
    const back = utf8ToUtf16(text, byte)
    assert.equal(back, i, `round-trip failed at offset ${i}`)
  }
})

test('round-trip: UTF-16→UTF-8→UTF-16 == identity（中文）', () => {
  const text = '你好世界测试'
  for (let i = 0; i <= text.length; i++) {
    const byte = utf16ToUtf8(text, i)
    const back = utf8ToUtf16(text, byte)
    assert.equal(back, i, `round-trip failed at offset ${i}`)
  }
})

test('round-trip: UTF-16→UTF-8→UTF-16 == identity（混合含 emoji，字符边界）', () => {
  const text = 'a🎉b你c'
  // 🎉 是 surrogate pair（2 UTF-16 code units），只在字符边界测 round-trip。
  // 有效边界：0(a前), 1(a后/🎉前), 3(🎉后/b前), 4(b后/你前), 5(你后/c前), 6(c后)
  const boundaries = [0, 1, 3, 4, 5, 6]
  for (const i of boundaries) {
    const byte = utf16ToUtf8(text, i)
    const back = utf8ToUtf16(text, byte)
    assert.equal(back, i, `round-trip failed at offset ${i}`)
  }
})

test('round-trip: UTF-8→UTF-16→UTF-8 == identity（中文，byte 对齐）', () => {
  const text = '你好世界'
  const totalBytes = new TextEncoder().encode(text).length
  for (let b = 0; b <= totalBytes; b += 3) {  // 中文每3字节一个字符
    const u16 = utf8ToUtf16(text, b)
    const back = utf16ToUtf8(text, u16)
    assert.equal(back, b, `round-trip failed at byte ${b}`)
  }
})

// ── 7. viewport 透传到 UI，不参与 contentHash 校验 ──
test('decideRestoration: viewport 在 contentHash 不匹配时仍透传', () => {
  const snap = makeSnapshot({ text: 'hello', contentHash: 'h1' })
  const params = {
    contentHash: 'h999',
    viewport: { scrollTop: 100 },
  }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, false)
  assert.equal(decision.hasViewport, true)
  assert.deepEqual(decision.viewport, { scrollTop: 100 })
})

test('decideRestoration: viewport 在 contentHash 匹配时也透传', () => {
  const snap = makeSnapshot({ text: 'hello', contentHash: 'h1' })
  const params = {
    contentHash: 'h1',
    selection: [1, 2],
    viewport: { scrollTop: 50 },
  }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, true)
  assert.equal(decision.hasViewport, true)
  assert.deepEqual(decision.viewport, { scrollTop: 50 })
})

// ── 8. contentHash undefined 时不校验（兼容旧 payload）──
test('decideRestoration: contentHash undefined → 视为匹配，恢复 selection', () => {
  const snap = makeSnapshot({ text: 'hello', contentHash: 'h42' })
  const params = {
    selection: [1, 3],
    // contentHash 不带
  }
  const decision = decideRestoration(snap, params)
  assert.equal(decision.shouldRestoreSelection, true)
  assert.equal(decision.anchorByte, 1)
  assert.equal(decision.headByte, 3)
})

// ── 9. 边界条件 ──
test('utf16ToUtf8: offset=0 → 0', () => {
  assert.equal(utf16ToUtf8('hello', 0), 0)
  assert.equal(utf16ToUtf8('你好', 0), 0)
  assert.equal(utf16ToUtf8('', 0), 0)
})

test('utf16ToUtf8: offset 超过 text.length → 截断到 text.length', () => {
  assert.equal(utf16ToUtf8('hello', 100), 5)
  assert.equal(utf16ToUtf8('你好', 100), 6)  // 2汉字 * 3 bytes
})

test('utf8ToUtf16: offset=0 → 0', () => {
  assert.equal(utf8ToUtf16('hello', 0), 0)
  assert.equal(utf8ToUtf16('你好', 0), 0)
})

test('utf8ToUtf16: byte offset 落在字符中间 → 回退到字符边界', () => {
  // '你好'：byte 1 落在第一个汉字中间（3字节），应回退到 0
  assert.equal(utf8ToUtf16('你好', 1), 0)
  assert.equal(utf8ToUtf16('你好', 2), 0)
  // byte 3 是第一个汉字结束
  assert.equal(utf8ToUtf16('你好', 3), 1)
})

// ── 10. 完整恢复流程模拟 ──
test('完整流程: 源设备 save → 目标设备 restore（ASCII, contentHash 匹配）', () => {
  // 源设备：Core snapshot cursor=5, anchor=3, contentHash='hash-10'
  const sourceSnap = makeSnapshot({ text: 'hello world', contentHash: 'hash-10', cursor: 5, selectionAnchor: 3 })
  const sourcePayload = buildPayloadFromSnapshot(sourceSnap, 'p1', 'v1', 'c1', null)

  // 目标设备：Core 加载同一章节，contentHash 匹配（同一份持久化内容）
  const targetSnap = makeSnapshot({ text: 'hello world', contentHash: 'hash-10' })
  const decision = decideRestoration(targetSnap, sourcePayload)

  assert.equal(decision.shouldRestoreSelection, true)
  // ASCII: UTF-16 == UTF-8，所以恢复后 byte offset 应等于原始
  assert.equal(decision.anchorByte, 3)
  assert.equal(decision.headByte, 5)
})

test('完整流程: 源设备 save → 目标设备 restore（中文, contentHash 匹配）', () => {
  // 源设备：Core snapshot cursor=6 (byte), anchor=3 (byte), contentHash='hash-zh-10'
  const sourceSnap = makeSnapshot({ text: '你好世界', contentHash: 'hash-zh-10', cursor: 6, selectionAnchor: 3 })
  const sourcePayload = buildPayloadFromSnapshot(sourceSnap, 'p1', 'v1', 'c1', null)

  // payload 中 selection/cursor 应为 UTF-16 offset
  assert.equal(sourcePayload.cursor, 2)       // byte 6 → utf16 2
  assert.equal(sourcePayload.selection[0], 1)  // byte 3 → utf16 1
  assert.equal(sourcePayload.selection[1], 2)  // byte 6 → utf16 2

  // 目标设备：Core 加载同一章节，contentHash 匹配
  const targetSnap = makeSnapshot({ text: '你好世界', contentHash: 'hash-zh-10' })
  const decision = decideRestoration(targetSnap, sourcePayload)

  assert.equal(decision.shouldRestoreSelection, true)
  // 恢复后 byte offset 应等于原始 Core snapshot 的 byte offset
  assert.equal(decision.anchorByte, 3)
  assert.equal(decision.headByte, 6)
})

test('完整流程: 目标设备 Core 正文已变（contentHash 不匹配）→ 安全恢复', () => {
  // 源设备：contentHash='hash-original'
  const sourceSnap = makeSnapshot({ text: '你好世界', contentHash: 'hash-original', cursor: 6, selectionAnchor: 3 })
  const sourcePayload = buildPayloadFromSnapshot(sourceSnap, 'p1', 'v1', 'c1', null)

  // 目标设备：Core 正文已变（用户在别处编辑后同步过来），contentHash 不同
  const targetSnap = makeSnapshot({ text: '你好世界已修改', contentHash: 'hash-modified' })
  const decision = decideRestoration(targetSnap, sourcePayload)

  // 不强套旧 offset
  assert.equal(decision.shouldRestoreSelection, false)
  assert.equal(decision.anchorByte, 0)
  assert.equal(decision.headByte, 0)
})

// ── 11. contentHash 跨设备语义：不用 revision ──
test('contentHash 语义: 同一份持久化内容 → contentHash 相同 → 可恢复', () => {
  // 源设备保存后 ChapterSaveReceipt.contentHash = 'sha256-xyz'
  // 目标设备加载同一章节 ChapterContent.meta.hash = 'sha256-xyz'
  // 即使两设备的 TextEditSession.revision 都是 initial()（不同会话），contentHash 仍匹配。
  const sourceSnap = makeSnapshot({ text: '正文', contentHash: 'sha256-xyz', cursor: 3, selectionAnchor: 0 })
  const sourcePayload = buildPayloadFromSnapshot(sourceSnap, 'p1', 'v1', 'c1', null)

  // 目标设备新会话 revision=0（EditorRevision::initial()），但 contentHash 来自磁盘，匹配。
  const targetSnap = makeSnapshot({ text: '正文', contentHash: 'sha256-xyz' })
  const decision = decideRestoration(targetSnap, sourcePayload)

  assert.equal(decision.shouldRestoreSelection, true)
  assert.equal(decision.anchorByte, 0)
  assert.equal(decision.headByte, 3)
})

test('contentHash 语义: 源设备有未保存修改 → contentHash 仍是上次保存的 hash', () => {
  // 源设备加载时 contentHash='hash-v1'，用户编辑后未保存，
  // updateCurrentState 仍用上次保存的 contentHash（不基于未持久化正文标"可精确恢复"）。
  // 目标设备拿到 payload.contentHash='hash-v1'，但目标设备磁盘内容已是 'hash-v2'（同步更新过），
  // contentHash 不匹配 → 安全恢复。
  const sourceSnap = makeSnapshot({ text: '正文已编辑未保存', contentHash: 'hash-v1', cursor: 5, selectionAnchor: 0 })
  const sourcePayload = buildPayloadFromSnapshot(sourceSnap, 'p1', 'v1', 'c1', null)

  const targetSnap = makeSnapshot({ text: '正文已编辑未保存', contentHash: 'hash-v2' })
  const decision = decideRestoration(targetSnap, sourcePayload)

  assert.equal(decision.shouldRestoreSelection, false)
})

console.log('---')
console.log(`✅ continuation_restore_logic: ${passed} tests passed`)
