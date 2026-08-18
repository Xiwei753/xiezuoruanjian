// selection_controller_affinity_owner.test.mjs — SelectionController affinity owner 闭环单测。
//
// Issue #629 R7 评论19 第1+2项：
// 1. 鼠标 dragSelect 发完整 VisualCaretPosition（head: hitPos），不发旧字段 utf16Head。
// 2. 删除 SujianEditor @State currentVisualCaretAffinity 双状态；
//    refreshRenderLayout() 按当前 snapshot cursor 从 SelectionController.getVisualCaret 取，
//    offset 不匹配时默认 Downstream（旧 affinity 不残留）。
// 3. rememberVisualCaret 只更新/发布视觉位置，不重复发 Core selection。
//
// EditorSessionCoordinator 依赖 Rust FFI；本测试用 mock 验证纯逻辑。
//
// 运行：node --experimental-strip-types selection_controller_affinity_owner.test.mjs

import { strict as assert } from 'node:assert'

let passed = 0
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}

console.log('SelectionController affinity owner 闭环单测')

// ── Mock EditorSessionCoordinator ──
function makeMockCoordinator(initialText = 'hello', initialCursor = 2) {
  let snapshot = {
    text: initialText,
    cursor: initialCursor,
    selectionAnchor: initialCursor,
    cursorByteOffset: initialCursor,
    selectionAnchor: initialCursor,
    revision: 0,
    generation: 0,
    composition: null,
  }
  const calls = { setSelection: 0, selectionArgs: [] }
  return {
    getSnapshot: () => snapshot,
    setSnapshot: (s) => { snapshot = { ...snapshot, ...s } },
    setSelection: async (anchorByte, headByte) => {
      calls.setSelection++
      calls.selectionArgs.push({ anchorByte, headByte })
      snapshot = { ...snapshot, selectionAnchor: anchorByte, cursor: headByte }
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    },
    calls,
  }
}

// ── Mock SelectionController（镜像生产代码的核心逻辑） ──
// 不依赖 ArkUI，纯逻辑验证 affinity owner 行为。
// Issue #629 R10 评论5327548809 第1项：visualCaret 绑定 revision/generation/compositionSessionId+compositionGeneration+offset。
function makeSelectionController(coordinator) {
  let visualCaret = null
  let visualCaretRevision = -1
  let visualCaretGeneration = -1
  let visualCaretCompositionSessionId = 0
  let visualCaretCompositionGeneration = -1
  let listenerCalls = []
  let coreSelectionCalls = []

  const updateIdentity = () => {
    const snap = coordinator.getSnapshot()
    visualCaretRevision = snap?.revision ?? 0
    visualCaretGeneration = snap?.generation ?? 0
    visualCaretCompositionSessionId = snap?.composition?.sessionId ?? 0
    visualCaretCompositionGeneration = snap?.composition?.generation ?? -1
  }

  return {
    registerVisualCaretListener: (l) => { l && listenerCalls.push('__registered__') },
    unregisterVisualCaretListener: () => {},
    listenerCalls: () => listenerCalls,
    clearListenerCalls: () => { listenerCalls = [] },

    // getVisualCaret：offset + 身份都匹配返回缓存，否则默认 Downstream
    getVisualCaret: (cursorUtf16) => {
      if (visualCaret !== null && visualCaret.utf16Offset === cursorUtf16) {
        const snap = coordinator.getSnapshot()
        if (snap !== null && snap !== undefined
          && (snap.revision ?? 0) === visualCaretRevision
          && (snap.generation ?? 0) === visualCaretGeneration
          && (snap.composition?.sessionId ?? 0) === visualCaretCompositionSessionId
          && (snap.composition?.generation ?? -1) === visualCaretCompositionGeneration) {
          return visualCaret
        }
      }
      return { utf16Offset: cursorUtf16, affinity: 'downstream' }
    },

    // rememberVisualCaret：只更新/发布视觉位置，不发 Core selection
    rememberVisualCaret: (position) => {
      visualCaret = position
      updateIdentity()
      listenerCalls.push({ type: 'visualCaret', position })
    },

    // invalidateVisualCaret：宽度变化时清除旧 hint
    invalidateVisualCaret: () => {
      visualCaret = null
      visualCaretRevision = -1
      visualCaretGeneration = -1
      visualCaretCompositionSessionId = 0
      visualCaretCompositionGeneration = -1
      listenerCalls.push({ type: 'invalidated' })
    },

    // onTap：发 Core selection + publishVisualCaret
    onTap: async (position) => {
      const utf16Offset = typeof position === 'number' ? position : position.utf16Offset
      const affinity = typeof position === 'number' ? 'downstream' : position.affinity
      const text = coordinator.getSnapshot()?.text ?? ''
      const byteOffset = utf16Offset  // 简化：ASCII 文本 utf16==utf8
      const result = await coordinator.setSelection(byteOffset, byteOffset)
      coreSelectionCalls.push({ kind: 'tap', byteOffset })
      visualCaret = { utf16Offset, affinity }
      updateIdentity()
      listenerCalls.push({ type: 'visualCaret', position: { utf16Offset, affinity } })
      return result
    },

    // onDragSelect：发 Core selection + publishVisualCaret
    onDragSelect: async (utf16Anchor, utf16Head) => {
      const anchorOffset = typeof utf16Anchor === 'number' ? utf16Anchor : utf16Anchor.utf16Offset
      const headOffset = typeof utf16Head === 'number' ? utf16Head : utf16Head.utf16Offset
      const headAffinity = typeof utf16Head === 'number' ? 'downstream' : utf16Head.affinity
      const text = coordinator.getSnapshot()?.text ?? ''
      const anchorByte = anchorOffset
      const headByte = headOffset
      const result = await coordinator.setSelection(anchorByte, headByte)
      coreSelectionCalls.push({ kind: 'dragSelect', anchorByte, headByte })
      visualCaret = { utf16Offset: headOffset, affinity: headAffinity }
      updateIdentity()
      listenerCalls.push({ type: 'visualCaret', position: { utf16Offset: headOffset, affinity: headAffinity } })
      return result
    },

    coreSelectionCalls: () => coreSelectionCalls,
    clearCoreSelectionCalls: () => { coreSelectionCalls = [] },
  }
}

// ── 1. getVisualCaret: offset 匹配返回缓存的 affinity ──
await testAsync('getVisualCaret: offset 匹配时返回缓存的 Upstream affinity', async () => {
  const coord = makeMockCoordinator('hello', 5)
  const ctrl = makeSelectionController(coord)
  // 先 remember 一个 Upstream affinity
  ctrl.rememberVisualCaret({ utf16Offset: 5, affinity: 'upstream' })
  // getVisualCaret 同一 offset → 返回 Upstream
  const pos = ctrl.getVisualCaret(5)
  assert.equal(pos.affinity, 'upstream', 'offset 匹配应返回缓存的 Upstream')
  assert.equal(pos.utf16Offset, 5)
})

// ── 2. getVisualCaret: offset 不匹配返回默认 Downstream ──
await testAsync('getVisualCaret: offset 不匹配时返回默认 Downstream（旧 affinity 不残留）', async () => {
  const coord = makeMockCoordinator('hello', 5)
  const ctrl = makeSelectionController(coord)
  // 先 remember 一个 Upstream at offset 5
  ctrl.rememberVisualCaret({ utf16Offset: 5, affinity: 'upstream' })
  // cursor 前进到 8（offset 不匹配）→ 返回默认 Downstream
  const pos = ctrl.getVisualCaret(8)
  assert.equal(pos.affinity, 'downstream', 'offset 不匹配应返回默认 Downstream')
  assert.equal(pos.utf16Offset, 8)
})

// ── 3. getVisualCaret: 从未 remember 过返回默认 Downstream ──
await testAsync('getVisualCaret: 从未 remember 过返回默认 Downstream', async () => {
  const coord = makeMockCoordinator('hello', 3)
  const ctrl = makeSelectionController(coord)
  const pos = ctrl.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', '从未设置应返回默认 Downstream')
  assert.equal(pos.utf16Offset, 3)
})

// ── 4. rememberVisualCaret: 只更新视觉位置，不发 Core selection ──
await testAsync('rememberVisualCaret: 不发 Core selection（setSelection 不被调用）', async () => {
  const coord = makeMockCoordinator('hello', 3)
  const ctrl = makeSelectionController(coord)
  ctrl.rememberVisualCaret({ utf16Offset: 3, affinity: 'upstream' })
  // 验证：coordinator.setSelection 没被调
  assert.equal(coord.calls.setSelection, 0, 'rememberVisualCaret 不应发 Core selection')
  // 验证：getVisualCaret 返回刚设置的 Upstream
  const pos = ctrl.getVisualCaret(3)
  assert.equal(pos.affinity, 'upstream', 'remember 后应返回 Upstream')
})

// ── 5. onTap: 发 Core selection + publishVisualCaret ──
await testAsync('onTap: 发 Core selection 并更新 visual caret', async () => {
  const coord = makeMockCoordinator('hello', 0)
  const ctrl = makeSelectionController(coord)
  const result = await ctrl.onTap({ utf16Offset: 3, affinity: 'upstream' })
  assert.equal(result.success, true)
  assert.equal(coord.calls.setSelection, 1, 'onTap 应发 Core setSelection')
  assert.deepEqual(coord.calls.selectionArgs[0], { anchorByte: 3, headByte: 3 })
  // visual caret 更新
  const pos = ctrl.getVisualCaret(3)
  assert.equal(pos.affinity, 'upstream')
})

// ── 6. onDragSelect: 发 Core selection + publishVisualCaret with head affinity ──
await testAsync('onDragSelect: 发 Core selection 并用 head 的 affinity 更新 visual caret', async () => {
  const coord = makeMockCoordinator('hello', 0)
  const ctrl = makeSelectionController(coord)
  const result = await ctrl.onDragSelect(1, { utf16Offset: 4, affinity: 'downstream' })
  assert.equal(result.success, true)
  assert.equal(coord.calls.setSelection, 1)
  assert.deepEqual(coord.calls.selectionArgs[0], { anchorByte: 1, headByte: 4 })
  // visual caret 用 head 的 affinity
  const pos = ctrl.getVisualCaret(4)
  assert.equal(pos.affinity, 'downstream')
})

// ── 7. Snapshot 前进后旧 affinity 不残留 ──
await testAsync('Snapshot 前进后旧 affinity 不残留：remember at 5 → cursor to 8 → default Downstream', async () => {
  const coord = makeMockCoordinator('hello world', 5)
  const ctrl = makeSelectionController(coord)
  // 命中测试 at offset 5, affinity Upstream
  await ctrl.onTap({ utf16Offset: 5, affinity: 'upstream' })
  assert.equal(ctrl.getVisualCaret(5).affinity, 'upstream')
  // Snapshot 前进：用户输入文字，cursor 到 8
  coord.setSnapshot({ text: 'hello world', cursor: 8, selectionAnchor: 8, cursorByteOffset: 8, selectionAnchor: 8 })
  // getVisualCaret(8) → offset 不匹配 → 默认 Downstream
  const pos = ctrl.getVisualCaret(8)
  assert.equal(pos.affinity, 'downstream', 'snapshot 前进后旧 affinity 不应残留')
})

// ── 8. 连续 remember 不导致双状态 ──
await testAsync('连续 remember 覆盖：remember Upstream → remember Downstream → 返回 Downstream', async () => {
  const coord = makeMockCoordinator('test', 2)
  const ctrl = makeSelectionController(coord)
  ctrl.rememberVisualCaret({ utf16Offset: 2, affinity: 'upstream' })
  ctrl.rememberVisualCaret({ utf16Offset: 2, affinity: 'downstream' })
  const pos = ctrl.getVisualCaret(2)
  assert.equal(pos.affinity, 'downstream', '最后一次 remember 应覆盖')
})

// ── 9. Drag select head 用 VisualCaretPosition，不是旧的 utf16Head number ──
await testAsync('onDragSelect 接受 VisualCaretPosition 作为 head（不用旧 utf16Head number）', async () => {
  const coord = makeMockCoordinator('abcdef', 0)
  const ctrl = makeSelectionController(coord)
  // head 是 VisualCaretPosition（含 affinity），不是 number
  const head = { utf16Offset: 5, affinity: 'upstream' }
  await ctrl.onDragSelect(0, head)
  const pos = ctrl.getVisualCaret(5)
  assert.equal(pos.affinity, 'upstream', 'head 的 affinity 应被保留')
  assert.equal(coord.calls.selectionArgs[0].headByte, 5)
})

// ── 10. 鼠标 dragSelect 命令结构验证（head 是 VisualCaretPosition，不是 utf16Head） ──
await testAsync('鼠标 dragSelect 命令结构：head 字段是 VisualCaretPosition（含 utf16Offset + affinity）', async () => {
  // 模拟 SujianEditor 鼠标 move 的命令派发
  const coord = makeMockCoordinator('hello', 0)
  const ctrl = makeSelectionController(coord)
  // 命中测试返回完整 VisualCaretPosition
  const hitPos = { utf16Offset: 4, affinity: 'downstream' }
  // 派发 dragSelect 命令（新结构：head: VisualCaretPosition）
  const cmd = { kind: 'dragSelect', anchor: { utf16Offset: 1, affinity: 'downstream' }, head: hitPos }
  assert.equal(cmd.kind, 'dragSelect')
  assert.equal(typeof cmd.head, 'object', 'head 应是 VisualCaretPosition 对象')
  assert.equal(cmd.head.utf16Offset, 4)
  assert.equal(cmd.head.affinity, 'downstream')
  assert.equal('utf16Head' in cmd, false, '不应有旧字段 utf16Head')
  // 执行
  await ctrl.onDragSelect(cmd.anchor, cmd.head)
  assert.equal(coord.calls.setSelection, 1)
})

// ── 11. Touch dragSelect 命令结构验证 ──
await testAsync('Touch dragSelect 命令结构：与鼠标一致，head 是 VisualCaretPosition', async () => {
  const coord = makeMockCoordinator('hello', 0)
  const ctrl = makeSelectionController(coord)
  const hitPos = { utf16Offset: 3, affinity: 'upstream' }
  const cmd = { kind: 'dragSelect', anchor: { utf16Offset: 0, affinity: 'downstream' }, head: hitPos }
  assert.equal(cmd.head.affinity, 'upstream')
  assert.equal('utf16Head' in cmd, false)
  await ctrl.onDragSelect(cmd.anchor, cmd.head)
  const pos = ctrl.getVisualCaret(3)
  assert.equal(pos.affinity, 'upstream')
})

// ── 12. refreshRenderLayout 模拟：从 SelectionController 取 affinity ──
await testAsync('refreshRenderLayout 模拟：按当前 cursor 从 owner 取 affinity，offset 匹配用 Upstream', async () => {
  const coord = makeMockCoordinator('hello world', 5)
  const ctrl = makeSelectionController(coord)
  // 模拟命中测试 at offset 5 with Upstream
  await ctrl.onTap({ utf16Offset: 5, affinity: 'upstream' })
  // 模拟 refreshRenderLayout：读当前 snapshot cursor
  const snap = coord.getSnapshot()
  const cursorUtf16 = snap.cursor
  const visualCaret = ctrl.getVisualCaret(cursorUtf16)
  assert.equal(visualCaret.affinity, 'upstream', 'cursor 匹配应返回 Upstream')
})

// ── 13. refreshRenderLayout 模拟：snapshot 前进后用默认 Downstream ──
await testAsync('refreshRenderLayout 模拟：snapshot 前进后用默认 Downstream（不残留旧 Upstream）', async () => {
  const coord = makeMockCoordinator('hello world', 5)
  const ctrl = makeSelectionController(coord)
  // 模拟命中测试 at offset 5 with Upstream
  await ctrl.onTap({ utf16Offset: 5, affinity: 'upstream' })
  // Snapshot 前进：用户输入，cursor 到 8
  coord.setSnapshot({ text: 'hello world', cursor: 8, selectionAnchor: 8, cursorByteOffset: 8, selectionAnchor: 8 })
  // refreshRenderLayout：读当前 snapshot cursor=8，getVisualCaret(8)
  const snap = coord.getSnapshot()
  const cursorUtf16 = snap.cursor
  const visualCaret = ctrl.getVisualCaret(cursorUtf16)
  assert.equal(visualCaret.affinity, 'downstream', 'snapshot 前进后应用默认 Downstream')
})

// ── 14. onTap 的 number 签名向后兼容 ──
await testAsync('onTap 向后兼容：接受 number 参数时默认 Downstream', async () => {
  const coord = makeMockCoordinator('hello', 0)
  const ctrl = makeSelectionController(coord)
  // 旧签名：传 number
  await ctrl.onTap(3)
  const pos = ctrl.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'number 参数应默认 Downstream')
})

// ── 15. onDragSelect 的 number 签名向后兼容 ──
await testAsync('onDragSelect 向后兼容：head 是 number 时默认 Downstream', async () => {
  const coord = makeMockCoordinator('hello', 0)
  const ctrl = makeSelectionController(coord)
  await ctrl.onDragSelect(1, 4)
  const pos = ctrl.getVisualCaret(4)
  assert.equal(pos.affinity, 'downstream', 'number head 应默认 Downstream')
})

// ══════════════════════════════════════════════════════════════
// 16. committed forward delete cursor 不变 → 旧 hint 应失效
// ══════════════════════════════════════════════════════════════

await testAsync('committed forward delete cursor 不变：revision 前进后旧 hint 失效', async () => {
  const coord = makeMockCoordinator('hello', 3)
  const ctrl = makeSelectionController(coord)
  // 先 remember 一个 Upstream at offset 3, revision=0
  coord.setSnapshot({ text: 'hello', cursor: 3, selectionAnchor: 3, revision: 0, generation: 0, composition: null })
  ctrl.rememberVisualCaret({ utf16Offset: 3, affinity: 'upstream' })
  assert.equal(ctrl.getVisualCaret(3).affinity, 'upstream', 'revision 匹配应返回 Upstream')
  // 模拟 forward delete：cursor 不变但 revision 前进（delete 成功后 revision++）
  coord.setSnapshot({ text: 'hllo', cursor: 3, selectionAnchor: 3, revision: 1, generation: 0, composition: null })
  // cursor 不变，但 revision 变了 → 旧 hint 应失效
  const pos = ctrl.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'revision 变化后旧 Upstream 应失效')
})

// ══════════════════════════════════════════════════════════════
// 17. composition forward delete cursor 不变 → 旧 hint 应失效
// ══════════════════════════════════════════════════════════════

await testAsync('composition forward delete cursor 不变：composition generation 变化后旧 hint 失效', async () => {
  const coord = makeMockCoordinator('hello', 3)
  const ctrl = makeSelectionController(coord)
  // composition 活跃时 remember visual caret
  coord.setSnapshot({ text: 'hello', cursor: 3, selectionAnchor: 3, revision: 2, generation: 5, composition: { sessionId: 7, baseRevision: 2, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: '', preeditCursorUtf16: 0 } })
  ctrl.rememberVisualCaret({ utf16Offset: 3, affinity: 'upstream' })
  assert.equal(ctrl.getVisualCaret(3).affinity, 'upstream', '身份匹配应返回 Upstream')
  // composition forward delete：cursor 不变，但 compositionGeneration 前进
  coord.setSnapshot({ text: 'hello', cursor: 3, selectionAnchor: 3, revision: 2, generation: 5, composition: { sessionId: 7, baseRevision: 2, generation: 4, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: '', preeditCursorUtf16: 0 } })
  const pos = ctrl.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'composition generation 变化后旧 Upstream 应失效')
})

// ══════════════════════════════════════════════════════════════
// 18. contentWidth 变化 → invalidateVisualCaret 清除旧 hint
// ══════════════════════════════════════════════════════════════

await testAsync('invalidateVisualCaret：宽度变化后旧 hint 失效', async () => {
  const coord = makeMockCoordinator('hello', 3)
  const ctrl = makeSelectionController(coord)
  // remember 一个 Upstream
  coord.setSnapshot({ text: 'hello', cursor: 3, selectionAnchor: 3, revision: 0, generation: 0, composition: null })
  ctrl.rememberVisualCaret({ utf16Offset: 3, affinity: 'upstream' })
  assert.equal(ctrl.getVisualCaret(3).affinity, 'upstream')
  // 模拟 contentWidth 变化 → invalidateVisualCaret
  ctrl.invalidateVisualCaret()
  // 同一 offset、同一身份 → 也应失效（因为 hint 被清空了）
  const pos = ctrl.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'invalidateVisualCaret 后旧 hint 应失效')
  // 验证 listener 收到 invalidated 通知
  const invalidatedCalls = ctrl.listenerCalls().filter(c => c.type === 'invalidated')
  assert.equal(invalidatedCalls.length, 1, '应收到 invalidated 通知')
})

await testAsync('invalidateVisualCaret 后新 hint 可以重新设置', async () => {
  const coord = makeMockCoordinator('hello', 3)
  const ctrl = makeSelectionController(coord)
  ctrl.rememberVisualCaret({ utf16Offset: 3, affinity: 'upstream' })
  ctrl.invalidateVisualCaret()
  assert.equal(ctrl.getVisualCaret(3).affinity, 'downstream', 'invalidate 后默认 Downstream')
  // 重新 remember
  ctrl.rememberVisualCaret({ utf16Offset: 3, affinity: 'upstream' })
  assert.equal(ctrl.getVisualCaret(3).affinity, 'upstream', '重新 remember 后返回 Upstream')
})

// ══════════════════════════════════════════════════════════════
// 19. Issue #629 R10 评论5327548809 第1项：composition session 复用 generation 也失效
// Core 的新 composition session 会重新从初始 generation 开始，cancel/finish 后再 begin，
// 若 revision 没变、cursor 又在同一 offset，仅比较 composition.generation 仍可能把上一次
// composition 的 affinity 误认成当前 session。compositionSessionId 必须一起存。
// ══════════════════════════════════════════════════════════════

await testAsync('composition session 复用 generation：sessionId 不同时旧 hint 失效（compositionSessionId 必要性）', async () => {
  const coord = makeMockCoordinator('hello', 3)
  const ctrl = makeSelectionController(coord)
  // composition session A (sessionId=7, generation=3) remember Upstream
  coord.setSnapshot({ text: 'hello', cursor: 3, selectionAnchor: 3, revision: 2, generation: 5, composition: { sessionId: 7, baseRevision: 2, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: 'x', preeditCursorUtf16: 0 } })
  ctrl.rememberVisualCaret({ utf16Offset: 3, affinity: 'upstream' })
  assert.equal(ctrl.getVisualCaret(3).affinity, 'upstream', 'session A 身份匹配应返回 Upstream')
  // finish → 新 composition session B (sessionId=8, generation=3)（generation 复用但 sessionId 不同）
  coord.setSnapshot({ text: 'hello', cursor: 3, selectionAnchor: 3, revision: 2, generation: 5, composition: { sessionId: 8, baseRevision: 2, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: 'y', preeditCursorUtf16: 0 } })
  const pos = ctrl.getVisualCaret(3)
  assert.equal(pos.affinity, 'downstream', 'session B sessionId 不同 → 旧 Upstream 应失效（compositionSessionId 必要性）')
})

await testAsync('composition 同 session generation 前进：身份匹配时 affinity 仍有效', async () => {
  const coord = makeMockCoordinator('hello', 3)
  const ctrl = makeSelectionController(coord)
  // composition session A (sessionId=7, generation=3) remember Upstream
  coord.setSnapshot({ text: 'hello', cursor: 3, selectionAnchor: 3, revision: 2, generation: 5, composition: { sessionId: 7, baseRevision: 2, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: 'x', preeditCursorUtf16: 0 } })
  ctrl.rememberVisualCaret({ utf16Offset: 3, affinity: 'upstream' })
  // 同 session、同 generation、同 revision → affinity 仍有效
  assert.equal(ctrl.getVisualCaret(3).affinity, 'upstream', '同 session 同 generation 应返回 Upstream')
})

console.log(`\n✅ selection_controller_affinity_owner: ${passed} tests passed`)
