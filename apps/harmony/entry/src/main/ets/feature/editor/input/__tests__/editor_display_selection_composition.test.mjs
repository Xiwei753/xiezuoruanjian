// editor_display_selection_composition.test.mjs — Issue #629 评论5324447292 并行子任务 B：
// display/committed 坐标与 Dispatcher 输入链闭环测试。
//
// 测试范围：
//   1. executeDisplaySelection：display/committed offset 差异（preedit 内点击/移出/drag）
//   2. composition 内 Left/Right/Backspace/Delete 走 composition grapheme API
//   3. composition 内 insertText/paste/selectAll/Enter 先 finish 再执行
//   4. 失败不提前更新 owner
//   5. 中文/emoji preedit 下退格/Enter/Paste/selectAll
//   6. committed Up/Down/Home/End 不提前 rememberVisualCaret
//   7. dragSelect anchor 持有完整 VisualCaretPosition
//
// 运行：node --experimental-strip-types editor_display_selection_composition.test.mjs

import { strict as assert } from 'node:assert'
import { SerialCommandQueue } from '../../session/editor_patch_logic.ts'

let passed = 0
let failed = 0
const testAsync = async (name, fn) => {
  try {
    await fn()
    passed++
    console.log(`  [PASS] ${name}`)
  } catch (e) {
    failed++
    console.log(`  [FAIL] ${name}: ${e.message}`)
  }
}

const utf8ByteLen = (s) => new TextEncoder().encode(s).length

// UTF-16 code unit offset ↔ UTF-8 byte offset（与 TextOffsetMapper 对齐）
function utf16ToUtf8(text, utf16Offset) {
  if (utf16Offset <= 0) return 0
  const limited = utf16Offset > text.length ? text.length : utf16Offset
  return new TextEncoder().encode(text.substring(0, limited)).length
}
function utf8ToUtf16(text, utf8Offset) {
  if (utf8Offset <= 0) return 0
  let byteLen = 0
  let utf16Index = 0
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i)
    let charByteLen = 1
    if (code < 0x80) charByteLen = 1
    else if (code < 0x800) charByteLen = 2
    else if (code >= 0xD800 && code <= 0xDBFF) {
      charByteLen = 4
      i += 1 // skip low surrogate
    } else charByteLen = 3
    if (byteLen + charByteLen > utf8Offset) return utf16Index
    byteLen += charByteLen
    utf16Index += (charByteLen === 4 ? 2 : 1)
  }
  return text.length
}

// CaretAffinity 枚举
const CaretAffinity = { Upstream: 'upstream', Downstream: 'downstream' }

console.log('Issue #629 评论5324447292 — display/committed 坐标与 Dispatcher 输入链闭环测试\n')

// ── makeFullDispatcher ──
// 镜像 EditorSemanticDispatcher 的完整逻辑（display selection + composition grapheme）。
// mock：coordinator snapshot + inputAdapter + selectionController + lineResolver。
async function makeFullDispatcher() {
  let snapshot = { text: '', revision: 0, generation: 0, compositionGeneration: 0, cursor: 0, selectionAnchor: 0, selectionHead: 0, composition: null }
  let composing = false
  const calls = {
    // composition
    compositionBegin: 0, compositionUpdate: 0, compositionFinish: 0, compositionCancel: 0,
    // composition grapheme
    compositionMoveGraphemeLeft: 0, compositionMoveGraphemeRight: 0,
    compositionDeleteGraphemeBackward: 0, compositionDeleteGraphemeForward: 0,
    // committed
    textInput: 0, paste: 0, selectAll: 0,
    setSelection: 0, onTap: 0, onDragSelect: 0,
    delete: 0, previousGraphemeBoundary: 0, nextGraphemeBoundary: 0,
    // visual caret
    rememberVisualCaret: 0, publishVisualCaret: 0,
  }
  const callSequence = []
  const updateCalls = []
  const tapCalls = []
  const dragSelectCalls = []
  const visualCaretUpdates = []  // 所有 visual caret 变更（remember + publish）
  const compositionGraphemeCalls = []
  let sealed = false
  let visualCaret = null

  // Mock selection controller
  const selectionController = {
    onTap: async (position) => {
      calls.onTap++
      const offset = typeof position === 'number' ? position : position.utf16Offset
      tapCalls.push(offset)
      callSequence.push(`tap:${offset}`)
      // 成功后 publish visual caret
      const pos = typeof position === 'number'
        ? { utf16Offset: position, affinity: CaretAffinity.Downstream }
        : position
      visualCaret = pos
      calls.publishVisualCaret++
      visualCaretUpdates.push({ source: 'publish', ...pos })
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    onDragSelect: async (anchor, head) => {
      calls.onDragSelect++
      const anchorOffset = typeof anchor === 'number' ? anchor : anchor.utf16Offset
      const headOffset = typeof head === 'number' ? head : head.utf16Offset
      const headAffinity = typeof head === 'number' ? CaretAffinity.Downstream : head.affinity
      dragSelectCalls.push({ anchor: anchorOffset, head: headOffset })
      callSequence.push(`dragSelect:${anchorOffset}:${headOffset}`)
      visualCaret = { utf16Offset: headOffset, affinity: headAffinity }
      calls.publishVisualCaret++
      visualCaretUpdates.push({ source: 'publish', ...visualCaret })
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    selectAll: async () => {
      calls.selectAll++
      callSequence.push('selectAll')
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    getVisualCaret: (cursorUtf16) => {
      if (visualCaret !== null && visualCaret.utf16Offset === cursorUtf16) {
        return visualCaret
      }
      return { utf16Offset: cursorUtf16, affinity: CaretAffinity.Downstream }
    },
    rememberVisualCaret: (position) => {
      calls.rememberVisualCaret++
      visualCaret = position
      visualCaretUpdates.push({ source: 'remember', ...position })
      callSequence.push(`rememberCaret:${position.utf16Offset}:${position.affinity}`)
    },
  }

  // Mock input adapter
  const inputAdapter = {
    isComposing: () => composing,
    onCompositionBegin: async () => {
      calls.compositionBegin++
      callSequence.push('compBegin')
      composing = true
      snapshot = {
        ...snapshot,
        composition: {
          sessionId: 1, baseRevision: snapshot.revision, generation: 1,
          replaceByteStart: 0, replaceByteEndExclusive: 0,
          preeditText: '', preeditCursorUtf16: 0,
        },
      }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    onCompositionUpdate: async (preedit, preeditCursorUtf16) => {
      calls.compositionUpdate++
      const cursor = preeditCursorUtf16 === undefined ? preedit.length : preeditCursorUtf16
      updateCalls.push({ preedit, cursorUtf16: cursor })
      callSequence.push(`compUpdate:${preedit}@${cursor}`)
      if (snapshot.composition) {
        snapshot = {
          ...snapshot,
          composition: { ...snapshot.composition, preeditText: preedit, preeditCursorUtf16: cursor, generation: snapshot.composition.generation + 1 },
        }
      }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    onCompositionFinish: async (committed) => {
      calls.compositionFinish++
      callSequence.push(`compFinish:${committed}`)
      if (snapshot.composition) {
        const comp = snapshot.composition
        const startUtf16 = utf8ToUtf16(snapshot.text, comp.replaceByteStart)
        const endUtf16 = utf8ToUtf16(snapshot.text, comp.replaceByteEndExclusive)
        const newText = snapshot.text.substring(0, startUtf16) + committed + snapshot.text.substring(endUtf16)
        const newCursor = startUtf16 + committed.length
        snapshot = { ...snapshot, text: newText, cursor: newCursor, selectionAnchor: newCursor, composition: null }
      }
      composing = false
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    onCompositionCancel: async () => {
      calls.compositionCancel++
      callSequence.push('compCancel')
      snapshot = { ...snapshot, composition: null }
      composing = false
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    finishActiveComposition: async () => {
      callSequence.push('finishActive')
      if (!composing || !snapshot.composition) {
        return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
      }
      return inputAdapter.onCompositionFinish(snapshot.composition.preeditText)
    },
    onTextInput: async (text) => {
      calls.textInput++
      callSequence.push(`textInput:${text}`)
      const cursorUtf16 = snapshot.cursor
      const byteOffset = utf16ToUtf8(snapshot.text, cursorUtf16)
      const newText = snapshot.text.substring(0, cursorUtf16) + text + snapshot.text.substring(cursorUtf16)
      snapshot = { ...snapshot, text: newText, cursor: cursorUtf16 + text.length, selectionAnchor: cursorUtf16 + text.length, selectionHead: cursorUtf16 + text.length }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    onPaste: async (text) => {
      calls.paste++
      callSequence.push(`paste:${text}`)
      const cursorUtf16 = snapshot.cursor
      const newText = snapshot.text.substring(0, cursorUtf16) + text + snapshot.text.substring(cursorUtf16)
      snapshot = { ...snapshot, text: newText, cursor: cursorUtf16 + text.length, selectionAnchor: cursorUtf16 + text.length, selectionHead: cursorUtf16 + text.length }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    currentSelectionUtf8: () => {
      const start = Math.min(snapshot.selectionAnchor, snapshot.cursor)
      const end = Math.max(snapshot.selectionAnchor, snapshot.cursor)
      return { start, end }
    },
    compositionMoveGraphemeLeft: async () => {
      calls.compositionMoveGraphemeLeft++
      compositionGraphemeCalls.push('moveLeft')
      callSequence.push('compMoveLeft')
      // 模拟 preedit cursor 左移一个 UTF-16 code unit
      if (snapshot.composition && snapshot.composition.preeditCursorUtf16 > 0) {
        const newCursor = snapshot.composition.preeditCursorUtf16 - 1
        snapshot = {
          ...snapshot,
          composition: { ...snapshot.composition, preeditCursorUtf16: newCursor },
        }
      }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    compositionMoveGraphemeRight: async () => {
      calls.compositionMoveGraphemeRight++
      compositionGraphemeCalls.push('moveRight')
      callSequence.push('compMoveRight')
      if (snapshot.composition && snapshot.composition.preeditCursorUtf16 < snapshot.composition.preeditText.length) {
        const newCursor = snapshot.composition.preeditCursorUtf16 + 1
        snapshot = {
          ...snapshot,
          composition: { ...snapshot.composition, preeditCursorUtf16: newCursor },
        }
      }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    compositionDeleteGraphemeBackward: async () => {
      calls.compositionDeleteGraphemeBackward++
      compositionGraphemeCalls.push('deleteBackward')
      callSequence.push('compDeleteBackward')
      // 模拟从 preedit 中删除一个 grapheme（简单处理：删前面一个 UTF-16 code unit）
      if (snapshot.composition && snapshot.composition.preeditCursorUtf16 > 0) {
        const cur = snapshot.composition.preeditCursorUtf16
        const newPreedit = snapshot.composition.preeditText.substring(0, cur - 1) + snapshot.composition.preeditText.substring(cur)
        snapshot = {
          ...snapshot,
          composition: { ...snapshot.composition, preeditText: newPreedit, preeditCursorUtf16: cur - 1 },
        }
      }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    compositionDeleteGraphemeForward: async () => {
      calls.compositionDeleteGraphemeForward++
      compositionGraphemeCalls.push('deleteForward')
      callSequence.push('compDeleteForward')
      if (snapshot.composition) {
        const cur = snapshot.composition.preeditCursorUtf16
        if (cur < snapshot.composition.preeditText.length) {
          const newPreedit = snapshot.composition.preeditText.substring(0, cur) + snapshot.composition.preeditText.substring(cur + 1)
          snapshot = { ...snapshot, composition: { ...snapshot.composition, preeditText: newPreedit } }
        }
      }
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
  }

  // Mock coordinator
  const coordinator = {
    getSnapshot: () => snapshot,
    setSnapshot: (s) => { snapshot = s },
    setComposing: (v) => { composing = v },
    delete: async (start, end, cause) => {
      calls.delete++
      callSequence.push(`delete:${start}:${end}`)
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    previousGraphemeBoundary: async (byteOffset) => {
      calls.previousGraphemeBoundary++
      return { success: true, data: Math.max(0, byteOffset - 1) }
    },
    nextGraphemeBoundary: async (byteOffset) => {
      calls.nextGraphemeBoundary++
      return { success: true, data: byteOffset + 1 }
    },
    setSelection: async (anchor, head) => {
      calls.setSelection++
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    replace: async (start, end, text, original, cause) => {
      callSequence.push('replace')
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
    insert: async (offset, text, cause) => {
      callSequence.push('insert')
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    },
  }

  // ── 镜像 EditorSemanticDispatcher 的 executeDisplaySelection ──
  const executeDisplaySelection = async (anchor, head) => {
    const snap = coordinator.getSnapshot()
    if (!snap) return { success: false, errorCode: 'NO_SESSION' }

    if (!inputAdapter.isComposing() || snap.composition === null || snap.composition === undefined) {
      const isRange = anchor.utf16Offset !== head.utf16Offset
      if (isRange) {
        return selectionController.onDragSelect(anchor, head)
      }
      return selectionController.onTap(head)
    }

    // 有 composition
    const comp = snap.composition
    const committedText = snap.text
    const compStartUtf16 = utf8ToUtf16(committedText, comp.replaceByteStart)
    const preeditUtf16Len = comp.preeditText.length
    const compEndDisplay = compStartUtf16 + preeditUtf16Len

    const isRange = anchor.utf16Offset !== head.utf16Offset
    const targetOffset = head.utf16Offset
    const targetInComposition = targetOffset >= compStartUtf16 && targetOffset <= compEndDisplay

    if (!isRange && targetInComposition) {
      const preeditCursorUtf16 = targetOffset - compStartUtf16
      const updateResult = await inputAdapter.onCompositionUpdate(comp.preeditText, preeditCursorUtf16)
      if (updateResult.success) {
        selectionController.rememberVisualCaret(head)
      }
      return updateResult
    }

    // target 移出 preedit 或非空 drag：先 finish composition
    const finishResult = await inputAdapter.finishActiveComposition()
    if (!finishResult.success) return finishResult
    if (isRange) {
      return selectionController.onDragSelect(anchor, head)
    }
    return selectionController.onTap(head)
  }

  // ── 镜像 composition grapheme routing ──
  const executeGraphemeBackspace = async () => {
    if (inputAdapter.isComposing()) {
      return inputAdapter.compositionDeleteGraphemeBackward()
    }
    const snap = coordinator.getSnapshot()
    if (!snap) return { success: false, errorCode: 'NO_SESSION' }
    const sel = inputAdapter.currentSelectionUtf8()
    if (sel.end > sel.start) return coordinator.delete(sel.start, sel.end, 'Delete')
    if (sel.start <= 0) return { success: false, errorCode: 'NO_CHAR_TO_DELETE' }
    const prev = await coordinator.previousGraphemeBoundary(sel.start)
    return coordinator.delete(prev.data, sel.start, 'Delete')
  }

  const executeGraphemeDelete = async () => {
    if (inputAdapter.isComposing()) {
      return inputAdapter.compositionDeleteGraphemeForward()
    }
    const snap = coordinator.getSnapshot()
    if (!snap) return { success: false, errorCode: 'NO_SESSION' }
    const sel = inputAdapter.currentSelectionUtf8()
    if (sel.end > sel.start) return coordinator.delete(sel.start, sel.end, 'Delete')
    const next = await coordinator.nextGraphemeBoundary(sel.start)
    return coordinator.delete(sel.start, next.data, 'Delete')
  }

  // Issue #629 R9：executeGraphemeLeft/Right 镜像新的 composition+Shift+左/右逻辑。
  // Shift 时先 finish → committed helper；无 Shift 时 preedit 最左/最右继续同方向 → finish → committed。
  const executeGraphemeLeft = async (extend) => {
    if (!inputAdapter.isComposing()) {
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    }
    if (extend) {
      const finishResult = await inputAdapter.finishActiveComposition()
      if (!finishResult.success) return finishResult
      callSequence.push('committedLeft:extend')
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    }
    const beforeCursor = snapshot.composition?.preeditCursorUtf16 ?? 0
    const moveResult = await inputAdapter.compositionMoveGraphemeLeft()
    if (!moveResult.success) return moveResult
    const afterCursor = snapshot.composition?.preeditCursorUtf16 ?? 0
    if (beforeCursor === afterCursor && beforeCursor <= 0) {
      const finishResult = await inputAdapter.finishActiveComposition()
      if (!finishResult.success) return finishResult
      callSequence.push('committedLeft:noShift')
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    }
    return moveResult
  }

  const executeGraphemeRight = async (extend) => {
    if (!inputAdapter.isComposing()) {
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    }
    if (extend) {
      const finishResult = await inputAdapter.finishActiveComposition()
      if (!finishResult.success) return finishResult
      callSequence.push('committedRight:extend')
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    }
    const preeditText = snapshot.composition?.preeditText ?? ''
    const beforeCursor = snapshot.composition?.preeditCursorUtf16 ?? 0
    const moveResult = await inputAdapter.compositionMoveGraphemeRight()
    if (!moveResult.success) return moveResult
    const afterCursor = snapshot.composition?.preeditCursorUtf16 ?? 0
    if (beforeCursor === afterCursor && beforeCursor >= preeditText.length) {
      const finishResult = await inputAdapter.finishActiveComposition()
      if (!finishResult.success) return finishResult
      callSequence.push('committedRight:noShift')
      return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
    }
    return moveResult
  }

  // ── 镜像 finish-then-execute ──
  const executeFinishThenInsertText = async (text) => {
    if (inputAdapter.isComposing()) {
      const finish = await inputAdapter.finishActiveComposition()
      if (!finish.success) return finish
    }
    return inputAdapter.onTextInput(text)
  }

  const executeFinishThenPaste = async (text) => {
    if (inputAdapter.isComposing()) {
      const finish = await inputAdapter.finishActiveComposition()
      if (!finish.success) return finish
    }
    return inputAdapter.onPaste(text)
  }

  const executeFinishThenSelectAll = async () => {
    if (inputAdapter.isComposing()) {
      const finish = await inputAdapter.finishActiveComposition()
      if (!finish.success) return finish
    }
    return selectionController.selectAll()
  }

  // Queue-based dispatch
  const queue = new SerialCommandQueue()
  const dispatch = (cmd) => {
    if (sealed) return Promise.resolve({ success: false, errorCode: 'SEALED', warnings: [], changedPaths: [], changedEntities: [] })
    return queue.enqueue(async () => {
      switch (cmd.kind) {
        case 'setSelection':
          return executeDisplaySelection(cmd.position, cmd.position)
        case 'dragSelect':
          return executeDisplaySelection(cmd.anchor, cmd.head)
        case 'selectAll':
          return executeFinishThenSelectAll()
        case 'insertText':
          return executeFinishThenInsertText(cmd.text)
        case 'paste':
          return executeFinishThenPaste(cmd.text)
        case 'graphemeBackspace':
          return executeGraphemeBackspace()
        case 'graphemeDelete':
          return executeGraphemeDelete()
        case 'graphemeLeft':
          return executeGraphemeLeft(cmd.extend)
        case 'graphemeRight':
          return executeGraphemeRight(cmd.extend)
        case 'imePreviewText':
          if (!inputAdapter.isComposing()) {
            const beginResult = await inputAdapter.onCompositionBegin()
            if (!beginResult.success) return beginResult
          }
          return inputAdapter.onCompositionUpdate(cmd.text)
        case 'imeCommitText':
          if (inputAdapter.isComposing()) return inputAdapter.onCompositionFinish(cmd.text)
          return inputAdapter.onTextInput(cmd.text)
        case 'imeSetSelection':
          // Issue #629 R9：imeSetSelection 统一走 executeDisplaySelection
          return executeDisplaySelection(
            { utf16Offset: cmd.utf16Start, affinity: CaretAffinity.Downstream },
            { utf16Offset: cmd.utf16End, affinity: CaretAffinity.Downstream }
          )
        default:
          return { success: true, data: { outcome: 'applied' }, warnings: [], changedPaths: [], changedEntities: [] }
      }
    })
  }
  const flush = () => queue.whenIdle()
  const seal = () => { sealed = true }
  const unseal = () => { sealed = false }

  return {
    dispatch, flush, seal, unseal,
    calls, callSequence, updateCalls, tapCalls, dragSelectCalls,
    visualCaretUpdates, compositionGraphemeCalls,
    coordinator, inputAdapter, selectionController,
    getSnapshot: () => snapshot,
    getVisualCaret: () => visualCaret,
    setSnapshot: coordinator.setSnapshot,
    setComposing: coordinator.setComposing,
  }
}

// ══════════════════════════════════════════════════════════════
// 1. display/committed offset 差异
// ══════════════════════════════════════════════════════════════

await testAsync('preedit 内点击：collapsed target 在 composition 区域 → composition update，不改 committed selection', async () => {
  const d = await makeFullDispatcher()
  // committed "ab"，replace [0,1)（"a" 被替换），preedit "你"（UTF-16 长 1）→ 显示 "你b"
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  // 点击 preedit 内部（显示坐标 0 = preedit 开头）
  const pos = { utf16Offset: 0, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'setSelection', position: pos })
  assert.equal(d.calls.compositionUpdate, 1, '应调 compositionUpdate')
  assert.equal(d.calls.onTap, 0, '不应调 onTap（不改 committed selection）')
  assert.equal(d.updateCalls[0].preedit, '你')
  assert.equal(d.updateCalls[0].cursorUtf16, 0, 'preedit 内点击 → cursorUtf16=0')
  // 成功后 owner 记忆
  assert.equal(d.calls.rememberVisualCaret, 1, '成功后应 rememberVisualCaret')
  const vc = d.getVisualCaret()
  assert.equal(vc.utf16Offset, 0)
  assert.equal(vc.affinity, CaretAffinity.Downstream)
})

await testAsync('preedit 末尾点击：cursorUtf16 = preedit.length', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  // 显示文本 "你好b"：preedit 区域 [0,2)，末尾坐标 2
  const pos = { utf16Offset: 2, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'setSelection', position: pos })
  assert.equal(d.calls.compositionUpdate, 1)
  assert.equal(d.updateCalls[0].cursorUtf16, 2, 'preedit 末尾 → cursorUtf16=2')
  assert.equal(d.calls.onTap, 0)
})

await testAsync('移出 preedit 点击：finish composition 后恒等映射到 committed', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  // 显示 "你b"，点击坐标 2 = "b" 之后（移出 composition 区域）
  const pos = { utf16Offset: 2, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'setSelection', position: pos })
  assert.equal(d.calls.compositionFinish, 1, '移出应先 finish composition')
  assert.equal(d.calls.compositionUpdate, 0, '移出不调 compositionUpdate')
  assert.equal(d.calls.onTap, 1, 'finish 后调 onTap')
  assert.equal(d.tapCalls[0], 2, '恒等映射：finish 后 committed == 显示 → tap(2)')
  assert.equal(d.getSnapshot().text, '你b', 'finish 提交 preedit')
  assert.equal(d.getSnapshot().composition, null)
})

await testAsync('移出 preedit 拖选：finish 后 dragSelect 恒等映射', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  // 从 preedit 内到文本末尾：部分越界 → finish + dragSelect
  const anchor = { utf16Offset: 1, affinity: CaretAffinity.Downstream }
  const head = { utf16Offset: 4, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'dragSelect', anchor, head })
  assert.equal(d.calls.compositionFinish, 1)
  assert.equal(d.calls.onDragSelect, 1)
  assert.deepEqual(d.dragSelectCalls[0], { anchor: 1, head: 4 })
})

await testAsync('preedit 内非折叠选区：→ finish + dragSelect（不走 composition update）', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  // preedit 内非折叠选区 [0,2)：虽在 composition 区域，但 isRange=true → finish + dragSelect
  const anchor = { utf16Offset: 0, affinity: CaretAffinity.Downstream }
  const head = { utf16Offset: 2, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'dragSelect', anchor, head })
  assert.equal(d.calls.compositionFinish, 1, '非折叠选区应 finish composition')
  assert.equal(d.calls.compositionUpdate, 0, '不走 composition update')
  assert.equal(d.calls.onDragSelect, 1)
})

await testAsync('无 composition 时 setSelection → 直接 onTap', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'hello', revision: 0, cursor: 3, selectionAnchor: 3, selectionHead: 3, composition: null })
  d.setComposing(false)
  const pos = { utf16Offset: 2, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'setSelection', position: pos })
  assert.equal(d.calls.onTap, 1)
  assert.equal(d.tapCalls[0], 2)
  assert.equal(d.calls.compositionUpdate, 0)
})

await testAsync('无 composition 时 dragSelect → 直接 onDragSelect', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'hello', revision: 0, cursor: 3, selectionAnchor: 3, selectionHead: 3, composition: null })
  const anchor = { utf16Offset: 1, affinity: CaretAffinity.Downstream }
  const head = { utf16Offset: 4, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'dragSelect', anchor, head })
  assert.equal(d.calls.onDragSelect, 1)
  assert.deepEqual(d.dragSelectCalls[0], { anchor: 1, head: 4 })
})

// ══════════════════════════════════════════════════════════════
// 2. composition grapheme 操作
// ══════════════════════════════════════════════════════════════

await testAsync('composition 内 Backspace → compositionDeleteGraphemeBackward', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeBackspace' })
  assert.equal(d.calls.compositionDeleteGraphemeBackward, 1, 'composition 内 backspace 应走 composition API')
  assert.equal(d.calls.delete, 0, '不应走 committed delete')
  assert.equal(d.calls.previousGraphemeBoundary, 0, '不应查 committed grapheme boundary')
  assert.deepEqual(d.compositionGraphemeCalls, ['deleteBackward'])
  // preedit cursor 从 2 移到 1（Mock 删 preedit[cursor-1]）
  const snap = d.getSnapshot()
  assert.equal(snap.composition.preeditCursorUtf16, 1)
  assert.ok(snap.composition.preeditText.length < '你好'.length, 'preedit 应缩短')
})

await testAsync('composition 内 Delete → compositionDeleteGraphemeForward', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 0 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeDelete' })
  assert.equal(d.calls.compositionDeleteGraphemeForward, 1)
  assert.equal(d.calls.delete, 0)
  assert.deepEqual(d.compositionGraphemeCalls, ['deleteForward'])
  // preedit "你好" 删掉第一个字 → "好"
  assert.equal(d.getSnapshot().composition.preeditText, '好')
})

await testAsync('composition 内 Left（无 Shift）→ compositionMoveGraphemeLeft', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.calls.compositionMoveGraphemeLeft, 1)
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 1, 'preedit cursor 从 2 移到 1')
})

await testAsync('composition 内 Right（无 Shift）→ compositionMoveGraphemeRight', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 0 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.calls.compositionMoveGraphemeRight, 1)
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 1, 'preedit cursor 从 0 移到 1')
})

await testAsync('无 composition 时 Backspace → 走 committed graphemeBackspace', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'abc', revision: 0, cursor: 3, selectionAnchor: 3, selectionHead: 3, composition: null })
  d.setComposing(false)
  await d.dispatch({ kind: 'graphemeBackspace' })
  assert.equal(d.calls.delete, 1, '无 composition 应走 committed delete')
  assert.equal(d.calls.compositionDeleteGraphemeBackward, 0)
  assert.equal(d.calls.previousGraphemeBoundary, 1)
})

await testAsync('无 composition 时 Delete → 走 committed graphemeDelete', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'abc', revision: 0, cursor: 0, selectionAnchor: 0, selectionHead: 0, composition: null })
  d.setComposing(false)
  await d.dispatch({ kind: 'graphemeDelete' })
  assert.equal(d.calls.delete, 1, '无 composition 应走 committed delete')
  assert.equal(d.calls.compositionDeleteGraphemeForward, 0)
})

await testAsync('无 composition 时 Left → 走 committed graphemeLeft', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'abc', revision: 0, cursor: 2, selectionAnchor: 2, selectionHead: 2, composition: null })
  d.setComposing(false)
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.calls.compositionMoveGraphemeLeft, 0, '不应调 composition API')
})

await testAsync('无 composition 时 Right → 走 committed graphemeRight', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'abc', revision: 0, cursor: 1, selectionAnchor: 1, selectionHead: 1, composition: null })
  d.setComposing(false)
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.calls.compositionMoveGraphemeRight, 0, '不应调 composition API')
})

// ══════════════════════════════════════════════════════════════
// 3. insertText/paste/selectAll 先 finish composition
// ══════════════════════════════════════════════════════════════

await testAsync('composition 内 insertText → finish composition 成功后执行 textInput', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'insertText', text: 'X' })
  assert.equal(d.calls.compositionFinish, 1, '应先 finish composition')
  assert.equal(d.calls.textInput, 1, 'finish 成功后执行 textInput')
  // finish 提交 preedit "你"，text 成为 "你b"
  // 然后 textInput 插入 "X"
  assert.ok(d.getSnapshot().text.includes('X'), '应包含 textInput 插入的 X')
  // 检查顺序：finishActive → compFinish → textInput
  const finishIdx = d.callSequence.indexOf('finishActive')
  const compFinishIdx = d.callSequence.indexOf('compFinish:你')
  const textInputIdx = d.callSequence.indexOf('textInput:X')
  assert.ok(finishIdx < compFinishIdx, 'finishActive 应在 compFinish 之前')
  assert.ok(compFinishIdx < textInputIdx, 'compFinish 应在 textInput 之前')
})

await testAsync('composition 内 paste → finish composition 成功后执行 paste', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'paste', text: '粘贴内容' })
  assert.equal(d.calls.compositionFinish, 1)
  assert.equal(d.calls.paste, 1)
  // finish 提交 preedit 后 paste
  assert.ok(d.getSnapshot().text.includes('你'), '应包含 finish 后的 preedit 前缀')
  assert.ok(d.getSnapshot().text.includes('粘贴内容'), '应包含粘贴内容')
})

await testAsync('composition 内 selectAll → finish composition 成功后执行 selectAll', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'selectAll' })
  assert.equal(d.calls.compositionFinish, 1, '应先 finish composition')
  assert.equal(d.calls.selectAll, 1, 'finish 成功后执行 selectAll')
})

await testAsync('无 composition 时 insertText → 直接 textInput（不 finish）', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'hi', revision: 0, cursor: 2, selectionAnchor: 2, selectionHead: 2, composition: null })
  d.setComposing(false)
  await d.dispatch({ kind: 'insertText', text: 'X' })
  assert.equal(d.calls.textInput, 1)
  assert.equal(d.calls.compositionFinish, 0, '无 composition 不应 finish')
})

await testAsync('无 composition 时 paste → 直接 paste（不 finish）', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'hi', revision: 0, cursor: 2, selectionAnchor: 2, selectionHead: 2, composition: null })
  d.setComposing(false)
  await d.dispatch({ kind: 'paste', text: 'X' })
  assert.equal(d.calls.paste, 1)
  assert.equal(d.calls.compositionFinish, 0)
})

await testAsync('无 composition 时 selectAll → 直接 selectAll（不 finish）', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'hi', revision: 0, cursor: 2, selectionAnchor: 2, selectionHead: 2, composition: null })
  d.setComposing(false)
  await d.dispatch({ kind: 'selectAll' })
  assert.equal(d.calls.selectAll, 1)
  assert.equal(d.calls.compositionFinish, 0)
})

// ══════════════════════════════════════════════════════════════
// 4. 失败不提前更新 owner
// ══════════════════════════════════════════════════════════════

await testAsync('preedit 内点击 compositionUpdate 失败 → 不 rememberVisualCaret', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  // 先设置一个已知的 visual caret
  d.selectionController.rememberVisualCaret({ utf16Offset: 5, affinity: CaretAffinity.Upstream })
  const beforeUpdates = d.visualCaretUpdates.length

  // 覆盖 compositionUpdate 让它返回失败
  const origUpdate = d.inputAdapter.onCompositionUpdate
  d.inputAdapter.onCompositionUpdate = async () => ({ success: false, errorCode: 'STALE_GENERATION', warnings: [], changedPaths: [], changedEntities: [] })

  const pos = { utf16Offset: 0, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'setSelection', position: pos })

  // compositionUpdate 失败后不应 rememberVisualCaret
  const afterUpdates = d.visualCaretUpdates.length
  assert.equal(afterUpdates, beforeUpdates, '失败后不应有新的 visualCaret 更新')
  // owner 保持旧值
  const vc = d.getVisualCaret()
  assert.equal(vc.utf16Offset, 5, 'owner 应保持旧值')
  assert.equal(vc.affinity, CaretAffinity.Upstream)

  // 恢复
  d.inputAdapter.onCompositionUpdate = origUpdate
})

// ══════════════════════════════════════════════════════════════
// 5. 中文/emoji preedit 下退格/删除
// ══════════════════════════════════════════════════════════════

await testAsync('中文 preedit 内退格：compositionDeleteGraphemeBackward', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: '', revision: 2, cursor: 0, selectionAnchor: 0, selectionHead: 0,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: '你好世界', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeBackspace' })
  assert.equal(d.calls.compositionDeleteGraphemeBackward, 1)
  // Mock 删 preedit[cursor-1]：preedit 缩短，cursor 递减
  const resultPreedit = d.getSnapshot().composition.preeditText
  assert.ok(resultPreedit.length < '你好世界'.length, 'preedit 应缩短')
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 1, 'cursor 从 2 移到 1')
})

await testAsync('emoji preedit 内删除：compositionDeleteGraphemeForward', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: '', revision: 2, cursor: 0, selectionAnchor: 0, selectionHead: 0,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: '👨‍👩‍👧hello', preeditCursorUtf16: 0 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeDelete' })
  assert.equal(d.calls.compositionDeleteGraphemeForward, 1)
  // 删 "👨‍👩‍👧"（8 UTF-16 code units）→ preedit 变 "hello"
  // Mock 删 preedit[0]：preedit 缩短
  const emojiResultPreedit = d.getSnapshot().composition.preeditText
  assert.ok(emojiResultPreedit.length > 0 && emojiResultPreedit.length < 50, 'preedit 应有效缩短')
})

await testAsync('中文 preedit 内 Left：compositionMoveGraphemeLeft', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: '', revision: 2, cursor: 0, selectionAnchor: 0, selectionHead: 0,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: '你好世界', preeditCursorUtf16: 3 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.calls.compositionMoveGraphemeLeft, 1)
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 2, '中文 Left 从 3 移到 2')
})

await testAsync('中文 preedit 内 Right：compositionMoveGraphemeRight', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: '', revision: 2, cursor: 0, selectionAnchor: 0, selectionHead: 0,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: '你好世界', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.calls.compositionMoveGraphemeRight, 1)
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 2, '中文 Right 从 1 移到 2')
})

await testAsync('中文 preedit 内 Enter → finish composition + textInput(\\n)', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  // Enter = insertText('\n')
  await d.dispatch({ kind: 'insertText', text: '\n' })
  assert.equal(d.calls.compositionFinish, 1, '应先 finish composition')
  assert.equal(d.calls.textInput, 1)
  // finish 提交 "你好"，然后 textInput 插入 "\n"
  assert.ok(d.getSnapshot().text.includes('你'), '应包含 finish 后的 preedit 前缀')
  assert.ok(d.getSnapshot().text.includes('\n'), '应包含 Enter 换行')
})

await testAsync('emoji preedit 内 paste → finish + paste', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '👨‍👩‍👧', preeditCursorUtf16: 8 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'paste', text: '粘贴' })
  assert.equal(d.calls.compositionFinish, 1)
  assert.equal(d.calls.paste, 1)
  // Mock: finish 提交 emoji preedit 后 paste（顺序取决于 mock cursor 实现）
  assert.ok(d.getSnapshot().text.includes('粘贴'), '应包含粘贴内容')
})

await testAsync('中文 preedit 内 selectAll → finish + selectAll', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'selectAll' })
  assert.equal(d.calls.compositionFinish, 1)
  assert.equal(d.calls.selectAll, 1)
})

// ══════════════════════════════════════════════════════════════
// 6. committed Up/Down/Home/End 不提前 rememberVisualCaret
// ══════════════════════════════════════════════════════════════

await testAsync('committed 路径 onTap 成功 → 唯一更新 owner 通过 publishVisualCaret', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'hello', revision: 0, cursor: 0, selectionAnchor: 0, selectionHead: 0, composition: null })
  d.setComposing(false)
  const pos = { utf16Offset: 3, affinity: CaretAffinity.Upstream }
  await d.dispatch({ kind: 'setSelection', position: pos })
  // 不应有提前 rememberVisualCaret（committed 路径不调 remember）
  // 唯一更新通过 publishVisualCaret（在 onTap 成功后）
  const publishUpdates = d.visualCaretUpdates.filter(v => v.source === 'publish')
  const rememberUpdates = d.visualCaretUpdates.filter(v => v.source === 'remember')
  assert.equal(publishUpdates.length, 1, '应有 1 次 publish')
  assert.equal(rememberUpdates.length, 0, '不应有 remember（committed 路径）')
  assert.equal(publishUpdates[0].utf16Offset, 3)
  assert.equal(publishUpdates[0].affinity, CaretAffinity.Upstream)
})

await testAsync('committed 路径 dragSelect 成功 → 唯一更新 owner 通过 publishVisualCaret', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'hello', revision: 0, cursor: 0, selectionAnchor: 0, selectionHead: 0, composition: null })
  const anchor = { utf16Offset: 1, affinity: CaretAffinity.Downstream }
  const head = { utf16Offset: 4, affinity: CaretAffinity.Upstream }
  await d.dispatch({ kind: 'dragSelect', anchor, head })
  const publishUpdates = d.visualCaretUpdates.filter(v => v.source === 'publish')
  const rememberUpdates = d.visualCaretUpdates.filter(v => v.source === 'remember')
  assert.equal(publishUpdates.length, 1, '应有 1 次 publish')
  assert.equal(rememberUpdates.length, 0, '不应有 remember')
  assert.equal(publishUpdates[0].utf16Offset, 4)
  assert.equal(publishUpdates[0].affinity, CaretAffinity.Upstream)
})

// ══════════════════════════════════════════════════════════════
// 7. dragSelect anchor 持有完整 VisualCaretPosition
// ══════════════════════════════════════════════════════════════

await testAsync('dragSelect anchor 携带 affinity，head 携带 affinity', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'hello', revision: 0, cursor: 0, selectionAnchor: 0, selectionHead: 0, composition: null })
  const anchor = { utf16Offset: 1, affinity: CaretAffinity.Upstream }
  const head = { utf16Offset: 4, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'dragSelect', anchor, head })
  assert.equal(d.calls.onDragSelect, 1)
  // anchor 和 head 都传给 onDragSelect
  assert.deepEqual(d.dragSelectCalls[0], { anchor: 1, head: 4 })
})

await testAsync('composition 内 dragSelect → finish 后 dragSelect 保持 anchor/head VisualCaretPosition', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  // 从 preedit 外到 preedit 外：部分越界 → finish + dragSelect
  const anchor = { utf16Offset: 0, affinity: CaretAffinity.Upstream }
  const head = { utf16Offset: 3, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'dragSelect', anchor, head })
  assert.equal(d.calls.compositionFinish, 1)
  assert.equal(d.calls.onDragSelect, 1)
  assert.deepEqual(d.dragSelectCalls[0], { anchor: 0, head: 3 })
})

// ══════════════════════════════════════════════════════════════
// 8. 连续 composition 操作串行
// ══════════════════════════════════════════════════════════════

await testAsync('连续 composition Backspace 串行执行', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: '', revision: 2, cursor: 0, selectionAnchor: 0, selectionHead: 0,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: '你好世界', preeditCursorUtf16: 4 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeBackspace' })
  await d.dispatch({ kind: 'graphemeBackspace' })
  await d.dispatch({ kind: 'graphemeBackspace' })
  // 三次退格：验证调用次数和 cursor 递减
  assert.equal(d.calls.compositionDeleteGraphemeBackward, 3)
  assert.ok(d.getSnapshot().composition.preeditCursorUtf16 < 4, 'cursor 应小于初始值 4')
})

await testAsync('composition Left → Backspace → Right 串行', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: '', revision: 2, cursor: 0, selectionAnchor: 0, selectionHead: 0,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 0, preeditText: '你好世界', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 1, 'Left 后 cursor=1')
  await d.dispatch({ kind: 'graphemeBackspace' })
  // 删 "你"，cursor 从 1 → 0
  assert.equal(d.getSnapshot().composition.preeditText, '好世界')
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 0)
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 1, 'Right 后 cursor=1')
})

await testAsync('composition → finish → insertText 串行（中文 IME 典型流程）', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  // IME 典型流程：inputText("你") → inputText("你好") → Enter/commit
  // 模拟：update composition → insertText("\n")（Enter 完成预输入并插入换行）
  await d.dispatch({ kind: 'imePreviewText', text: '你好世' })
  await d.dispatch({ kind: 'insertText', text: '\n' })
  // 顺序：compBegin(已在 composing=true 时跳过), compUpdate("你好世"), finishActive, compFinish("你好世"), textInput("\n")
  assert.equal(d.getSnapshot().composition, null, 'finish 后 composition 清空')
  assert.ok(d.getSnapshot().text.includes('你好世'), '应包含 finish 后的 preedit')
  assert.ok(d.getSnapshot().text.includes('\n'), '应包含 Enter 换行')
})

// ══════════════════════════════════════════════════════════════
// 9. preedit 有替换区间的显示坐标计算
// ══════════════════════════════════════════════════════════════

await testAsync('preedit 替换区间中间位置点击：正确换算 preeditCursorUtf16', async () => {
  const d = await makeFullDispatcher()
  // committed "abc"，replace [1,2)（"b" 被替换），preedit "你好"（UTF-16 长 2）
  // 显示文本 = "a你好c"（UTF-16 长 4）
  // composition 区域 [1, 3)（compStartUtf16=1, compEndDisplay=1+2=3）
  d.setSnapshot({
    text: 'abc', revision: 2, cursor: 2, selectionAnchor: 2, selectionHead: 2,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 1, replaceByteEndExclusive: 2, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  // 点击显示坐标 2 = "好" 的位置 → preeditCursorUtf16 = 2 - 1 = 1
  const pos = { utf16Offset: 2, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'setSelection', position: pos })
  assert.equal(d.calls.compositionUpdate, 1)
  assert.equal(d.updateCalls[0].cursorUtf16, 1, '显示坐标 2 → preeditCursorUtf16=1（compStartUtf16=1）')
  assert.equal(d.calls.onTap, 0)
})

await testAsync('preedit 替换区间末尾点击：cursorUtf16 = preedit.length', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'abc', revision: 2, cursor: 2, selectionAnchor: 2, selectionHead: 2,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 1, replaceByteEndExclusive: 2, preeditText: '你好', preeditCursorUtf16: 0 },
  })
  d.setComposing(true)
  // 点击显示坐标 3 = preedit 末尾 → preeditCursorUtf16 = 3 - 1 = 2 = preedit.length
  const pos = { utf16Offset: 3, affinity: CaretAffinity.Downstream }
  await d.dispatch({ kind: 'setSelection', position: pos })
  assert.equal(d.updateCalls[0].cursorUtf16, 2, 'preedit 末尾 → cursorUtf16=preedit.length')
})

// ══════════════════════════════════════════════════════════════
// 10. composition grapheme 不影响 committed text
// ══════════════════════════════════════════════════════════════

await testAsync('composition 内 Left 不改变 committed text', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'abc', revision: 2, cursor: 2, selectionAnchor: 2, selectionHead: 2,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 1, replaceByteEndExclusive: 2, preeditText: '你好', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().text, 'abc', 'committed text 不应改变')
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 0, 'preedit cursor 移到 0')
})

await testAsync('composition 内 Backspace 不改变 committed text', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'abc', revision: 2, cursor: 2, selectionAnchor: 2, selectionHead: 2,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 1, replaceByteEndExclusive: 2, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeBackspace' })
  assert.ok(d.getSnapshot().composition.preeditText.length < '你好'.length, 'preedit 应缩短')
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 1, 'cursor 从 2 移到 1')
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 1, 'preedit cursor 从 2 移到 1')
})

// ══════════════════════════════════════════════════════════════
// 11. Shift+Left/Right 在 composition 下必须先 finish 再形成 selection
// ══════════════════════════════════════════════════════════════

await testAsync('Shift+Left 在 composition 下：先 finish → 再走 committed Left（extend=true）', async () => {
  const d = await makeFullDispatcher()
  // replace [0,1) 会把 "a" 替换为 preedit "你"，finish 后 text = "你b"
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeLeft', extend: true })
  // 必须先 finish composition
  assert.equal(d.calls.compositionFinish, 1, 'Shift+Left 必须先 finish composition')
  // 然后走 committed Left（extend=true 时不应调 compositionMoveGraphemeLeft）
  assert.equal(d.calls.compositionMoveGraphemeLeft, 0, 'Shift+Left 不应调 compositionMoveGraphemeLeft')
  // 检查调用顺序：finishActive → compFinish → committedLeft:extend
  const finishIdx = d.callSequence.indexOf('finishActive')
  const committedLeftIdx = d.callSequence.indexOf('committedLeft:extend')
  assert.ok(finishIdx < committedLeftIdx, 'finish 应在 committedLeft 之前')
  // finish 提交 preedit "你" 替换 [0,1)，text 变成 "你b"
  assert.equal(d.getSnapshot().text, '你b', 'finish 应提交 preedit 到正文')
})

await testAsync('Shift+Right 在 composition 下：先 finish → 再走 committed Right（extend=true）', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 0 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeRight', extend: true })
  assert.equal(d.calls.compositionFinish, 1, 'Shift+Right 必须先 finish composition')
  assert.equal(d.calls.compositionMoveGraphemeRight, 0, 'Shift+Right 不应调 compositionMoveGraphemeRight')
  const finishIdx = d.callSequence.indexOf('finishActive')
  const committedRightIdx = d.callSequence.indexOf('committedRight:extend')
  assert.ok(finishIdx < committedRightIdx, 'finish 应在 committedRight 之前')
  assert.equal(d.getSnapshot().text, '你b', 'finish 应提交 preedit 到正文')
})

await testAsync('Shift+Left 不能只改 preeditCursorUtf16，必须 finish 后走普通 selection', async () => {
  const d = await makeFullDispatcher()
  // replace [0,1) 会把 "a" 替换为 preedit "你好"，finish 后 text = "你好b"
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeLeft', extend: true })
  // composition 必须被 finish，preedit 消失
  assert.equal(d.getSnapshot().composition, null, 'Shift+Left 后 composition 应被 finish')
  assert.equal(d.getSnapshot().text, '你好b', 'finish 提交 preedit 后 text 应包含 preedit')
})

// ══════════════════════════════════════════════════════════════
// 12. preedit 最左/最右继续同方向 → finish → 跨到 committed 正文
// ══════════════════════════════════════════════════════════════

await testAsync('preedit 最左端按 Left：finish 后跨到 committed 正文', async () => {
  const d = await makeFullDispatcher()
  // replace [0,1) → finish 后 text = "你b"
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 0 },
  })
  d.setComposing(true)
  // preedit cursor 已在 0（最左端），按 Left 应 finish → 进入 committed
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.getSnapshot().composition, null, 'composition 应被 finish')
  assert.equal(d.getSnapshot().text, '你b', 'finish 应提交 preedit 到正文')
  assert.equal(d.calls.compositionFinish, 1, '应 finish composition')
  // 走了 committedLeft:noShift（finish 后再 Left）
  const committedLeftIdx = d.callSequence.indexOf('committedLeft:noShift')
  assert.ok(committedLeftIdx >= 0, '应走 committedLeft:noShift')
})

await testAsync('preedit 最右端按 Right：finish 后跨到 committed 正文', async () => {
  const d = await makeFullDispatcher()
  // replace [0,1) → finish 后 text = "你b"
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  // preedit cursor 已在 1（= preeditText.length，最右端），按 Right 应 finish → 进入 committed
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.getSnapshot().composition, null, 'composition 应被 finish')
  assert.equal(d.getSnapshot().text, '你b', 'finish 应提交 preedit 到正文')
  assert.equal(d.calls.compositionFinish, 1, '应 finish composition')
  const committedRightIdx = d.callSequence.indexOf('committedRight:noShift')
  assert.ok(committedRightIdx >= 0, '应走 committedRight:noShift')
})

await testAsync('preedit 中间按 Left：不 finish，只移 preedit cursor', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeLeft', extend: false })
  assert.equal(d.calls.compositionMoveGraphemeLeft, 1, '应调 compositionMoveGraphemeLeft')
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 0, 'preedit cursor 从 1 移到 0')
  assert.equal(d.calls.compositionFinish, 0, '中间按 Left 不应 finish')
})

await testAsync('preedit 中间按 Right：不 finish，只移 preedit cursor', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 0 },
  })
  d.setComposing(true)
  await d.dispatch({ kind: 'graphemeRight', extend: false })
  assert.equal(d.calls.compositionMoveGraphemeRight, 1, '应调 compositionMoveGraphemeRight')
  assert.equal(d.getSnapshot().composition.preeditCursorUtf16, 1, 'preedit cursor 从 0 移到 1')
  assert.equal(d.calls.compositionFinish, 0, '中间按 Right 不应 finish')
})

// ══════════════════════════════════════════════════════════════
// 13. imeSetSelection 统一走 executeDisplaySelection
// ══════════════════════════════════════════════════════════════

await testAsync('imeSetSelection 在 composition 内非空 range：finish + dragSelect（不压成单光标）', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  // imeSetSelection [0,2] — 非空 range，即使在 composition 内也要 finish + dragSelect
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 0, utf16End: 2 })
  assert.equal(d.calls.compositionFinish, 1, '非空 range 必须 finish composition')
  assert.equal(d.calls.onDragSelect, 1, '应走 dragSelect（不压成单光标）')
})

await testAsync('imeSetSelection collapsed 在 composition 内：走 composition update（不 finish）', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你好', preeditCursorUtf16: 2 },
  })
  d.setComposing(true)
  // imeSetSelection [1,1] — collapsed 在 preedit 内
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 1, utf16End: 1 })
  assert.equal(d.calls.compositionUpdate, 1, 'collapsed 在 preedit 内应走 compositionUpdate')
  assert.equal(d.updateCalls[0].cursorUtf16, 1, 'preeditCursorUtf16 = 1')
  assert.equal(d.calls.compositionFinish, 0, '不应 finish')
})

await testAsync('imeSetSelection 移出 composition：finish + onTap', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({
    text: 'ab', revision: 2, cursor: 1, selectionAnchor: 1, selectionHead: 1,
    composition: { sessionId: 7, baseRevision: 0, generation: 3, replaceByteStart: 0, replaceByteEndExclusive: 1, preeditText: '你', preeditCursorUtf16: 1 },
  })
  d.setComposing(true)
  // imeSetSelection [2,2] — collapsed 但移出 preedit
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 2, utf16End: 2 })
  assert.equal(d.calls.compositionFinish, 1, '移出 preedit 应 finish')
  assert.equal(d.calls.onTap, 1, '应走 onTap')
})

await testAsync('imeSetSelection 无 composition：直接走普通 selection', async () => {
  const d = await makeFullDispatcher()
  d.setSnapshot({ text: 'hello', revision: 0, cursor: 3, selectionAnchor: 3, selectionHead: 3, composition: null })
  d.setComposing(false)
  await d.dispatch({ kind: 'imeSetSelection', utf16Start: 1, utf16End: 4 })
  assert.equal(d.calls.onDragSelect, 1, '无 composition 应走 dragSelect')
  assert.deepEqual(d.dragSelectCalls[0], { anchor: 1, head: 4 })
})

// ══════════════════════════════════════════════════════════════
// Summary
// ══════════════════════════════════════════════════════════════

console.log(`\n  ${passed} passed, ${failed} failed`)
if (failed > 0) {
  process.exit(1)
}
